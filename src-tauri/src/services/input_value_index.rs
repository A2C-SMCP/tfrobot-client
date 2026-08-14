use crate::services::config::ConfigService;
use crate::services::storage::write_json_atomically;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

const FILE_NAME: &str = "input_value_ids.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputValueIdIndex {
    schema_version: u32,
    #[serde(default)]
    ids: BTreeSet<String>,
}

fn path(config: &ConfigService, instance_id: &str) -> PathBuf {
    config
        .computer_instance_storage_root(instance_id)
        .join(FILE_NAME)
}

pub fn load(config: &ConfigService, instance_id: &str) -> Result<BTreeSet<String>, String> {
    let path = path(config, instance_id);
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "Failed to read input value ID index {}: {error}",
            path.display()
        )
    })?;
    let index: InputValueIdIndex = serde_json::from_slice(&bytes).map_err(|error| {
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
    Ok(index.ids)
}

/// Records only the non-sensitive logical ID. Historical IDs are deliberately retained so a
/// later Computer deletion can remove values from both keychain namespaces even after the SDK
/// definition itself has been deleted or changed kind.
pub fn record(config: &ConfigService, instance_id: &str, input_id: &str) -> Result<(), String> {
    let mut ids = load(config, instance_id)?;
    if !ids.insert(input_id.to_string()) {
        return Ok(());
    }
    let path = path(config, instance_id);
    write_json_atomically(
        &path,
        &InputValueIdIndex {
            schema_version: SCHEMA_VERSION,
            ids,
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

        record(&config, "one", "token").unwrap();
        record(&config, "one", "token").unwrap();
        record(&config, "one", "region").unwrap();

        assert_eq!(
            load(&config, "one").unwrap(),
            BTreeSet::from(["region".to_string(), "token".to_string()])
        );
        let raw = fs::read_to_string(path(&config, "one")).unwrap();
        assert!(!raw.contains("secret"));
    }
}
