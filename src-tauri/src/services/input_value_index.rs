use crate::services::config::ConfigService;
use crate::services::storage::write_json_atomically;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

const FILE_NAME: &str = "input_value_ids.json";
const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum InputValueStorageKind {
    Value,
    Secret,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputValueIdIndex {
    schema_version: u32,
    #[serde(default)]
    entries: BTreeMap<String, BTreeSet<InputValueStorageKind>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyInputValueIdIndexV1 {
    schema_version: u32,
    #[serde(default)]
    ids: BTreeSet<String>,
}

fn path(config: &ConfigService, instance_id: &str) -> PathBuf {
    config
        .computer_instance_storage_root(instance_id)
        .join(FILE_NAME)
}

pub fn load(
    config: &ConfigService,
    instance_id: &str,
) -> Result<BTreeMap<String, BTreeSet<InputValueStorageKind>>, String> {
    let path = path(config, instance_id);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "Failed to read input value ID index {}: {error}",
            path.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "Failed to parse input value ID index {}: {error}",
            path.display()
        )
    })?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    if version == 1 {
        let legacy: LegacyInputValueIdIndexV1 = serde_json::from_value(value).map_err(|error| {
            format!(
                "Failed to parse legacy input value ID index {}: {error}",
                path.display()
            )
        })?;
        debug_assert_eq!(legacy.schema_version, 1);
        // V1 did not record a storage kind. Treat its IDs as plain values so a non-secret
        // lifecycle never acquires Keychain authority. Current Secret definitions add their
        // precise Secret kind at the Computer lifecycle boundary.
        return Ok(legacy
            .ids
            .into_iter()
            .map(|id| (id, BTreeSet::from([InputValueStorageKind::Value])))
            .collect());
    }
    let index: InputValueIdIndex = serde_json::from_value(value).map_err(|error| {
        format!(
            "Failed to parse input value ID index {}: {error}",
            path.display()
        )
    })?;
    if index.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "Unsupported input value ID index schema version {} in {}",
            index.schema_version,
            path.display()
        ));
    }
    Ok(index.entries)
}

/// Records only the non-sensitive logical ID. Historical IDs are deliberately retained so a
/// later Computer deletion can remove values from both the plain value store and Keychain secret
/// namespace even after the SDK definition itself has been deleted or changed kind.
pub fn record(
    config: &ConfigService,
    instance_id: &str,
    input_id: &str,
    kind: InputValueStorageKind,
) -> Result<(), String> {
    let mut entries = load(config, instance_id)?;
    if !entries
        .entry(input_id.to_string())
        .or_default()
        .insert(kind)
    {
        return Ok(());
    }
    let path = path(config, instance_id);
    write_json_atomically(
        &path,
        &InputValueIdIndex {
            schema_version: SCHEMA_VERSION,
            entries,
        },
    )
    .map_err(|error| {
        format!(
            "Failed to persist input value ID index {}: {error}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_unique_historical_ids_without_values() {
        let directory = tempfile::tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();

        record(&config, "one", "token", InputValueStorageKind::Secret).unwrap();
        record(&config, "one", "token", InputValueStorageKind::Secret).unwrap();
        record(&config, "one", "token", InputValueStorageKind::Value).unwrap();
        record(&config, "one", "region", InputValueStorageKind::Value).unwrap();

        assert_eq!(
            load(&config, "one").unwrap(),
            BTreeMap::from([
                (
                    "region".to_string(),
                    BTreeSet::from([InputValueStorageKind::Value]),
                ),
                (
                    "token".to_string(),
                    BTreeSet::from([InputValueStorageKind::Value, InputValueStorageKind::Secret,]),
                ),
            ])
        );
        let raw = fs::read_to_string(path(&config, "one")).unwrap();
        assert!(!raw.contains("top-secret"));
        assert!(!raw.contains("input value"));
    }

    #[test]
    fn legacy_untyped_ids_upgrade_without_granting_keychain_access() {
        let directory = tempfile::tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let path = path(&config, "one");
        write_json_atomically(
            &path,
            &serde_json::json!({"schema_version": 1, "ids": ["token"]}),
        )
        .unwrap();

        assert_eq!(
            load(&config, "one").unwrap(),
            BTreeMap::from([(
                "token".to_string(),
                BTreeSet::from([InputValueStorageKind::Value]),
            )])
        );
        record(&config, "one", "token", InputValueStorageKind::Secret).unwrap();
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("\"schema_version\": 2"));
    }
}
