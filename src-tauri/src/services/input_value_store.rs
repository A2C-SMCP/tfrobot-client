use crate::services::config::ConfigService;
use crate::services::storage::write_json_atomically;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "input_values.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputValuesDocument {
    schema_version: u32,
    #[serde(default)]
    values: BTreeMap<String, Value>,
}

/// Plain, per-Computer persistence for non-secret MCP Input values.
///
/// Secret PromptString values deliberately use the OS Keychain instead. Keeping this store in
/// the Computer's client-owned storage directory makes that security boundary explicit.
#[derive(Debug, Clone)]
pub struct InputValueStore {
    path: PathBuf,
}

impl InputValueStore {
    pub fn for_computer(config: &ConfigService, instance_id: &str) -> Self {
        Self::from_storage_root(config.computer_instance_storage_root(instance_id))
    }

    pub fn from_storage_root(storage_root: impl AsRef<Path>) -> Self {
        Self {
            path: storage_root.as_ref().join(FILE_NAME),
        }
    }

    pub fn get(&self, input_id: &str) -> Result<Option<Value>, String> {
        Ok(self.load()?.values.get(input_id).cloned())
    }

    pub fn list(&self) -> Result<BTreeMap<String, Value>, String> {
        Ok(self.load()?.values)
    }

    pub fn set(&self, input_id: &str, value: &Value) -> Result<(), String> {
        let mut document = self.load()?;
        document.values.insert(input_id.to_string(), value.clone());
        self.save(&document)
    }

    pub fn delete(&self, input_id: &str) -> Result<(), String> {
        let mut document = self.load()?;
        if document.values.remove(input_id).is_none() {
            return Ok(());
        }
        self.save(&document)
    }

    fn load(&self) -> Result<InputValuesDocument, String> {
        if !self.path.exists() {
            return Ok(InputValuesDocument {
                schema_version: SCHEMA_VERSION,
                values: BTreeMap::new(),
            });
        }
        let bytes = fs::read(&self.path).map_err(|error| {
            format!(
                "Failed to read input values {}: {error}",
                self.path.display()
            )
        })?;
        let document: InputValuesDocument = serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "Failed to parse input values {}: {error}",
                self.path.display()
            )
        })?;
        if document.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Unsupported input values schema version {} in {}",
                document.schema_version,
                self.path.display()
            ));
        }
        Ok(document)
    }

    fn save(&self, document: &InputValuesDocument) -> Result<(), String> {
        write_json_atomically(&self.path, document).map_err(|error| {
            format!(
                "Failed to persist input values {}: {error}",
                self.path.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_secret_values_round_trip_in_plain_per_computer_storage() {
        let directory = tempfile::tempdir().unwrap();
        let first = InputValueStore::from_storage_root(directory.path().join("first"));
        let second = InputValueStore::from_storage_root(directory.path().join("second"));

        first.set("region", &serde_json::json!("cn")).unwrap();
        assert_eq!(first.get("region").unwrap(), Some(serde_json::json!("cn")));
        assert_eq!(second.get("region").unwrap(), None);

        let raw = fs::read_to_string(&first.path).unwrap();
        assert!(raw.contains("region"));
        assert!(raw.contains("cn"));

        first.delete("region").unwrap();
        assert_eq!(first.get("region").unwrap(), None);
    }
}
