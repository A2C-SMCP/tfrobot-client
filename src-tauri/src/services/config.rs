use crate::commands::connection::ConnectionProfile;
use crate::commands::inputs::InputDefinition;
use crate::services::computer::{ComputerInstance, ComputerInstancesConfig};
use crate::services::connection_targets::{ConnectionTargetsConfig, ManualSmcpTarget};
use sha2::{Digest, Sha256};
use smcp_computer::mcp_clients::MCPServerConfig;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Service for persisting all configuration data to disk
pub struct ConfigService {
    config_dir: PathBuf,
    computer_instances_file: PathBuf,
    connection_targets_file: PathBuf,
}

impl ConfigService {
    /// Create a new ConfigService with the given app data directory
    pub fn new(app_data_dir: PathBuf) -> Result<Self, std::io::Error> {
        fs::create_dir_all(&app_data_dir)?;

        Ok(Self {
            computer_instances_file: app_data_dir.join("computer_instances.json"),
            connection_targets_file: app_data_dir.join("connection_targets.json"),
            config_dir: app_data_dir,
        })
    }

    // --- MCP Server Configs ---

    pub fn load_configs_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<MCPServerConfig>, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.mcp_servers.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    pub fn save_configs_for_instance(
        &self,
        instance_id: &str,
        configs: &[MCPServerConfig],
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(instance_id, |instance| {
            instance.mcp_servers = configs.to_vec();
        })
    }

    pub fn add_config_for_instance(
        &self,
        instance_id: &str,
        config: MCPServerConfig,
    ) -> Result<ComputerInstance, ConfigError> {
        let mut configs = self.load_configs_for_instance(instance_id)?;
        let name = config.name().to_string();
        configs.retain(|c| c.name() != name);
        configs.push(config);
        self.save_configs_for_instance(instance_id, &configs)
    }

    pub fn remove_config_for_instance(
        &self,
        instance_id: &str,
        name: &str,
    ) -> Result<ComputerInstance, ConfigError> {
        let mut configs = self.load_configs_for_instance(instance_id)?;
        let original_len = configs.len();
        configs.retain(|c| c.name() != name);
        if configs.len() == original_len {
            return Err(ConfigError::NotFound(name.to_string()));
        }
        self.save_configs_for_instance(instance_id, &configs)
    }

    // --- Input Definitions ---

    pub fn load_inputs_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<InputDefinition>, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.inputs.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    pub fn save_inputs_for_instance(
        &self,
        instance_id: &str,
        inputs: &[InputDefinition],
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(instance_id, |instance| {
            instance.inputs = inputs.to_vec();
        })
    }

    // --- Input Values ---

    pub fn load_input_values_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.input_values.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    pub fn save_input_values_for_instance(
        &self,
        instance_id: &str,
        values: &HashMap<String, serde_json::Value>,
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(instance_id, |instance| {
            instance.input_values = values.clone();
        })
    }

    // --- Connection Profiles ---

    pub fn load_profiles_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<ConnectionProfile>, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.connection_profiles.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    pub fn save_profiles_for_instance(
        &self,
        instance_id: &str,
        profiles: &[ConnectionProfile],
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(instance_id, |instance| {
            instance.connection_profiles = profiles.to_vec();
        })
    }

    // --- Computer Instances ---

    pub fn load_computer_instances(&self) -> Result<ComputerInstancesConfig, ConfigError> {
        let mut config: ComputerInstancesConfig = load_json_file(&self.computer_instances_file)?;
        config.normalize();
        Ok(config)
    }

    pub fn save_computer_instances(
        &self,
        instances: &ComputerInstancesConfig,
    ) -> Result<(), ConfigError> {
        let mut instances = instances.clone();
        instances.normalize();
        save_json_file(&self.computer_instances_file, &instances)
    }

    pub fn get_computer_instance(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .into_iter()
            .find(|instance| instance.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))
    }

    pub fn add_computer_instance(&self, instance: ComputerInstance) -> Result<(), ConfigError> {
        let mut instances = self.load_computer_instances()?;
        if instances
            .instances
            .iter()
            .any(|existing| existing.id == instance.id)
        {
            return Err(ConfigError::AlreadyExists(instance.id));
        }
        instances.instances.push(instance);
        self.save_computer_instances(&instances)
    }

    pub fn rename_computer_instance(
        &self,
        id: &str,
        name: String,
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(id, |instance| {
            instance.name = name;
        })
    }

    pub fn update_computer_instance<F>(
        &self,
        id: &str,
        update: F,
    ) -> Result<ComputerInstance, ConfigError>
    where
        F: FnOnce(&mut ComputerInstance),
    {
        let mut instances = self.load_computer_instances()?;
        let instance = instances
            .instances
            .iter_mut()
            .find(|instance| instance.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        update(instance);
        let updated = instance.clone();
        self.save_computer_instances(&instances)?;
        Ok(updated)
    }

    pub fn remove_computer_instance(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let mut instances = self.load_computer_instances()?;
        let index = instances
            .instances
            .iter()
            .position(|instance| instance.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        let removed = instances.instances.remove(index);
        self.save_computer_instances(&instances)?;
        Ok(removed)
    }

    // --- Connection Targets ---

    pub fn load_connection_targets(&self) -> Result<ConnectionTargetsConfig, ConfigError> {
        load_json_file(&self.connection_targets_file)
    }

    pub fn save_connection_targets(
        &self,
        targets: &ConnectionTargetsConfig,
    ) -> Result<(), ConfigError> {
        save_json_file(&self.connection_targets_file, targets)
    }

    pub fn list_manual_smcp_targets(&self) -> Result<Vec<ManualSmcpTarget>, ConfigError> {
        Ok(self.load_connection_targets()?.manual_smcp_targets)
    }

    pub fn save_manual_smcp_target(
        &self,
        mut target: ManualSmcpTarget,
    ) -> Result<ManualSmcpTarget, ConfigError> {
        target.id = stable_or_existing_manual_target_id(&target.id, &target);
        let mut targets = self.load_connection_targets()?;
        targets
            .manual_smcp_targets
            .retain(|existing| existing.id != target.id);
        targets.manual_smcp_targets.push(target.clone());
        self.save_connection_targets(&targets)?;
        Ok(target)
    }

    pub fn get_manual_smcp_target(&self, id: &str) -> Result<ManualSmcpTarget, ConfigError> {
        self.load_connection_targets()?
            .manual_smcp_targets
            .into_iter()
            .find(|target| target.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))
    }

    pub fn delete_manual_smcp_target(&self, id: &str) -> Result<ManualSmcpTarget, ConfigError> {
        let mut targets = self.load_connection_targets()?;
        let index = targets
            .manual_smcp_targets
            .iter()
            .position(|target| target.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        let removed = targets.manual_smcp_targets.remove(index);
        self.save_connection_targets(&targets)?;
        Ok(removed)
    }

    pub fn migrate_legacy_profiles_to_manual_targets(
        &self,
    ) -> Result<Vec<(String, String, String)>, ConfigError> {
        let instances = self.load_computer_instances()?;
        let mut targets = self.load_connection_targets()?;
        let mut migrated_keys = Vec::new();

        for instance in instances.instances {
            for profile in instance.connection_profiles {
                let target = ManualSmcpTarget {
                    id: stable_manual_target_id(&profile),
                    name: profile.name.clone(),
                    url: profile.url,
                    namespace: profile.namespace,
                    office_id: profile.office_id,
                    computer_name: profile.computer_name,
                    headers: profile.headers,
                };
                if !targets
                    .manual_smcp_targets
                    .iter()
                    .any(|existing| existing.id == target.id)
                {
                    targets.manual_smcp_targets.push(target.clone());
                }
                migrated_keys.push((instance.id.clone(), profile.name, target.id));
            }
        }

        self.save_connection_targets(&targets)?;
        Ok(migrated_keys)
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }
}

pub fn normalize_manual_smcp_target(mut target: ManualSmcpTarget) -> ManualSmcpTarget {
    target.id = stable_or_existing_manual_target_id(&target.id, &target);
    target
}

fn stable_or_existing_manual_target_id(id: &str, target: &ManualSmcpTarget) -> String {
    let id = id.trim();
    if id.is_empty() {
        stable_manual_target_id_from_parts(
            &target.name,
            &target.url,
            &target.office_id,
            &target.computer_name,
            &target.headers,
        )
    } else {
        id.to_string()
    }
}

fn stable_manual_target_id(profile: &ConnectionProfile) -> String {
    stable_manual_target_id_from_parts(
        &profile.name,
        &profile.url,
        &profile.office_id,
        &profile.computer_name,
        &profile.headers,
    )
}

fn stable_manual_target_id_from_parts(
    name: &str,
    url: &str,
    office_id: &str,
    computer_name: &str,
    headers: &HashMap<String, String>,
) -> String {
    let mut hasher = Sha256::new();
    hash_string_field(&mut hasher, name);
    hash_string_field(&mut hasher, url);
    hash_string_field(&mut hasher, office_id);
    hash_string_field(&mut hasher, computer_name);
    let mut header_pairs: Vec<_> = headers.iter().collect();
    header_pairs.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in header_pairs {
        hash_string_field(&mut hasher, key);
        hash_string_field(&mut hasher, value);
    }
    let digest = hasher.finalize();
    format!("manual-{}", hex::encode(&digest[..16]))
}

fn hash_string_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_be_bytes());
    hasher.update(value.as_bytes());
}

fn load_json_file<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, ConfigError> {
    if !path.exists() {
        return Ok(T::default());
    }
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(T::default());
    }
    let data: T = serde_json::from_str(&content)?;
    Ok(data)
}

fn save_json_file<T: serde::Serialize + ?Sized>(path: &Path, data: &T) -> Result<(), ConfigError> {
    let content = serde_json::to_string_pretty(data)?;
    fs::write(path, content)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TEST_INSTANCE_ID: &str = "computer-a";

    fn setup() -> (ConfigService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = ConfigService::new(tmp.path().to_path_buf()).unwrap();
        svc.add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer"))
            .unwrap();
        (svc, tmp)
    }

    fn setup_empty() -> (ConfigService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = ConfigService::new(tmp.path().to_path_buf()).unwrap();
        (svc, tmp)
    }

    // --- MCP Server Configs ---

    #[test]
    fn test_load_empty_configs() {
        let (svc, _tmp) = setup();
        let configs = svc.load_configs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert!(configs.is_empty());
    }

    #[test]
    fn test_empty_computer_instances_remain_empty() {
        let (svc, _tmp) = setup_empty();
        let instances = svc.load_computer_instances().unwrap();

        assert!(instances.instances.is_empty());
    }

    #[test]
    fn test_save_and_load_configs_roundtrip() {
        let (svc, _tmp) = setup();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "test-server",
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap();

        svc.save_configs_for_instance(TEST_INSTANCE_ID, std::slice::from_ref(&config))
            .unwrap();
        let loaded = svc.load_configs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name(), "test-server");
    }

    #[test]
    fn test_legacy_config_files_are_ignored() {
        let (svc, tmp) = setup_empty();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "legacy-server",
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap();

        save_json_file(&tmp.path().join("mcp_servers.json"), &[config]).unwrap();

        let instances = svc.load_computer_instances().unwrap();
        assert!(instances.instances.is_empty());
    }

    #[test]
    fn test_corrupted_legacy_files_are_ignored() {
        let (svc, tmp) = setup_empty();
        std::fs::write(tmp.path().join("mcp_servers.json"), "not json").unwrap();
        std::fs::write(tmp.path().join("inputs.json"), "not json").unwrap();

        let instances = svc.load_computer_instances().unwrap();
        assert!(instances.instances.is_empty());
    }

    #[test]
    fn test_empty_new_instances_file_uses_empty_config() {
        let (svc, tmp) = setup_empty();
        std::fs::write(tmp.path().join("computer_instances.json"), "").unwrap();

        let loaded = svc.load_computer_instances().unwrap();

        assert!(loaded.instances.is_empty());
    }

    #[test]
    fn test_save_and_load_multiple_computer_instances_roundtrip() {
        let (svc, _tmp) = setup();
        let first_config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "first-server",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();
        let second_config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "second-server",
            "server_parameters": { "command": "python", "args": [], "env": {} }
        }))
        .unwrap();
        let instances = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![
                ComputerInstance {
                    id: "first".to_string(),
                    name: "First".to_string(),
                    mcp_servers: vec![first_config],
                    ..ComputerInstance::new("", "")
                },
                ComputerInstance {
                    id: "second".to_string(),
                    name: "Second".to_string(),
                    mcp_servers: vec![second_config],
                    ..ComputerInstance::new("", "")
                },
            ],
        };

        svc.save_computer_instances(&instances).unwrap();
        let loaded = svc.load_computer_instances().unwrap();

        assert_eq!(loaded.instances.len(), 2);
        let first = loaded
            .instances
            .iter()
            .find(|instance| instance.id == "first")
            .unwrap();
        assert_eq!(first.mcp_servers[0].name(), "first-server");
        let second = loaded
            .instances
            .iter()
            .find(|instance| instance.id == "second")
            .unwrap();
        assert_eq!(second.mcp_servers[0].name(), "second-server");
    }

    #[test]
    fn test_add_get_rename_and_remove_computer_instance() {
        let (svc, _tmp) = setup();
        let instance = ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::new("", "")
        };

        svc.add_computer_instance(instance).unwrap();
        assert_eq!(svc.get_computer_instance("second").unwrap().name, "Second");

        let renamed = svc
            .rename_computer_instance("second", "Renamed".to_string())
            .unwrap();
        assert_eq!(renamed.name, "Renamed");
        assert_eq!(svc.get_computer_instance("second").unwrap().name, "Renamed");

        let removed = svc.remove_computer_instance("second").unwrap();
        assert_eq!(removed.id, "second");
        assert!(svc.get_computer_instance("second").is_err());
    }

    #[test]
    fn test_add_duplicate_computer_instance_returns_error() {
        let (svc, _tmp) = setup();
        let instance = ComputerInstance {
            id: "dup".to_string(),
            name: "Duplicate".to_string(),
            ..ComputerInstance::new("", "")
        };

        svc.add_computer_instance(instance.clone()).unwrap();
        let error = svc.add_computer_instance(instance).unwrap_err();

        assert!(error.to_string().contains("Already exists"));
    }

    #[test]
    fn test_remove_legacy_default_id_computer_instance_succeeds() {
        let (svc, _tmp) = setup();

        let removed = svc.remove_computer_instance(TEST_INSTANCE_ID).unwrap();

        assert_eq!(removed.id, TEST_INSTANCE_ID);
        assert!(svc.get_computer_instance(TEST_INSTANCE_ID).is_err());
    }

    #[test]
    fn test_add_config() {
        let (svc, _tmp) = setup();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "added-server",
            "server_parameters": {
                "command": "node",
                "args": [],
                "env": {}
            }
        }))
        .unwrap();

        svc.add_config_for_instance(TEST_INSTANCE_ID, config)
            .unwrap();
        let loaded = svc.load_configs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name(), "added-server");
    }

    #[test]
    fn test_add_duplicate_config_replaces() {
        let (svc, _tmp) = setup();
        let config1: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "dup-server",
            "server_parameters": { "command": "node", "args": ["v1"], "env": {} }
        }))
        .unwrap();
        let config2: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "dup-server",
            "server_parameters": { "command": "python", "args": ["v2"], "env": {} }
        }))
        .unwrap();

        svc.add_config_for_instance(TEST_INSTANCE_ID, config1)
            .unwrap();
        svc.add_config_for_instance(TEST_INSTANCE_ID, config2)
            .unwrap();

        let loaded = svc.load_configs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name(), "dup-server");
        // Verify content was actually replaced (command: "node" → "python")
        match &loaded[0] {
            MCPServerConfig::Stdio(c) => {
                assert_eq!(
                    c.server_parameters.command, "python",
                    "Config should be replaced, not appended"
                );
                assert_eq!(c.server_parameters.args, vec!["v2"]);
            }
            other => panic!("Expected Stdio variant, got: {:?}", other),
        }
    }

    #[test]
    fn test_instance_scoped_configs_are_isolated() {
        let (svc, _tmp) = setup();
        svc.add_computer_instance(ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::new("", "")
        })
        .unwrap();

        let first_config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "first-server",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();
        let second_config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "second-server",
            "server_parameters": { "command": "python", "args": [], "env": {} }
        }))
        .unwrap();

        svc.add_config_for_instance(TEST_INSTANCE_ID, first_config)
            .unwrap();
        svc.add_config_for_instance("second", second_config)
            .unwrap();

        let first_configs = svc.load_configs_for_instance(TEST_INSTANCE_ID).unwrap();
        let second_configs = svc.load_configs_for_instance("second").unwrap();

        assert_eq!(first_configs.len(), 1);
        assert_eq!(first_configs[0].name(), "first-server");
        assert_eq!(second_configs.len(), 1);
        assert_eq!(second_configs[0].name(), "second-server");
    }

    #[test]
    fn test_remove_config() {
        let (svc, _tmp) = setup();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "to-remove",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();

        svc.add_config_for_instance(TEST_INSTANCE_ID, config)
            .unwrap();
        svc.remove_config_for_instance(TEST_INSTANCE_ID, "to-remove")
            .unwrap();

        let loaded = svc.load_configs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_config_returns_error() {
        let (svc, _tmp) = setup();
        let result = svc.remove_config_for_instance(TEST_INSTANCE_ID, "nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_load_corrupted_json_file() {
        let (svc, tmp) = setup();
        std::fs::write(tmp.path().join("computer_instances.json"), "not json").unwrap();
        let result = svc.load_configs_for_instance(TEST_INSTANCE_ID);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_empty_json_file_returns_empty_config() {
        let (svc, tmp) = setup_empty();
        std::fs::write(tmp.path().join("computer_instances.json"), "").unwrap();
        let instances = svc.load_computer_instances().unwrap();
        assert!(instances.instances.is_empty());
    }

    // --- Input Definitions ---

    #[test]
    fn test_load_empty_inputs() {
        let (svc, _tmp) = setup();
        let inputs = svc.load_inputs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_save_and_load_inputs_roundtrip() {
        let (svc, _tmp) = setup();
        let input: InputDefinition = serde_json::from_value(serde_json::json!({
            "type": "PromptString",
            "id": "api-key",
            "label": "API Key",
            "password": true
        }))
        .unwrap();

        svc.save_inputs_for_instance(TEST_INSTANCE_ID, &[input])
            .unwrap();
        let loaded = svc.load_inputs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    // --- Input Values ---

    #[test]
    fn test_load_empty_input_values() {
        let (svc, _tmp) = setup();
        let values = svc
            .load_input_values_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert!(values.is_empty());
    }

    #[test]
    fn test_save_and_load_input_values_roundtrip() {
        let (svc, _tmp) = setup();
        let mut values = HashMap::new();
        values.insert("key1".to_string(), serde_json::json!("value1"));
        values.insert("key2".to_string(), serde_json::json!(42));

        svc.save_input_values_for_instance(TEST_INSTANCE_ID, &values)
            .unwrap();
        let loaded = svc
            .load_input_values_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["key1"], serde_json::json!("value1"));
        assert_eq!(loaded["key2"], serde_json::json!(42));
    }

    // --- Connection Profiles ---

    #[test]
    fn test_load_empty_profiles() {
        let (svc, _tmp) = setup();
        let profiles = svc.load_profiles_for_instance(TEST_INSTANCE_ID).unwrap();
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_save_and_load_profiles_roundtrip() {
        let (svc, _tmp) = setup();
        let profile: ConnectionProfile = serde_json::from_value(serde_json::json!({
            "name": "prod",
            "url": "https://smcp.example.com",
            "namespace": "default",
            "office_id": "office-1",
            "computer_name": "my-pc",
            "headers": {}
        }))
        .unwrap();

        svc.save_profiles_for_instance(TEST_INSTANCE_ID, &[profile])
            .unwrap();
        let loaded = svc.load_profiles_for_instance(TEST_INSTANCE_ID).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "prod");
        assert_eq!(loaded[0].url, "https://smcp.example.com");
    }

    #[test]
    fn test_legacy_profiles_migrate_to_global_manual_targets() {
        let (svc, _tmp) = setup();
        let profile: ConnectionProfile = serde_json::from_value(serde_json::json!({
            "name": "prod",
            "url": "https://smcp.example.com",
            "namespace": "/smcp",
            "office_id": "office-1",
            "computer_name": "my-pc",
            "headers": { "X-TF-Namespace": "ns" }
        }))
        .unwrap();
        svc.save_profiles_for_instance(TEST_INSTANCE_ID, std::slice::from_ref(&profile))
            .unwrap();

        let migrated = svc.migrate_legacy_profiles_to_manual_targets().unwrap();
        let targets = svc.list_manual_smcp_targets().unwrap();

        assert_eq!(migrated.len(), 1);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "prod");
        assert_eq!(targets[0].office_id, "office-1");
        assert_eq!(
            targets[0].headers.get("X-TF-Namespace").map(String::as_str),
            Some("ns")
        );

        svc.migrate_legacy_profiles_to_manual_targets().unwrap();
        assert_eq!(svc.list_manual_smcp_targets().unwrap().len(), 1);
    }

    #[test]
    fn test_manual_target_id_is_stable_and_independent_of_header_order() {
        let mut headers_a = HashMap::new();
        headers_a.insert("X-TF-Namespace".to_string(), "ns".to_string());
        headers_a.insert("X-TF-Region".to_string(), "cn".to_string());

        let mut headers_b = HashMap::new();
        headers_b.insert("X-TF-Region".to_string(), "cn".to_string());
        headers_b.insert("X-TF-Namespace".to_string(), "ns".to_string());

        let target_a = ManualSmcpTarget {
            id: String::new(),
            name: "prod".to_string(),
            url: "https://smcp.example.com".to_string(),
            namespace: "/smcp".to_string(),
            office_id: "office-1".to_string(),
            computer_name: "my-pc".to_string(),
            headers: headers_a,
        };
        let target_b = ManualSmcpTarget {
            headers: headers_b,
            ..target_a.clone()
        };

        let id_a = normalize_manual_smcp_target(target_a).id;
        let id_b = normalize_manual_smcp_target(target_b).id;

        assert_eq!(id_a, id_b);
        assert!(id_a.starts_with("manual-"));
        assert_eq!(id_a.len(), "manual-".len() + 32);
    }

    // --- File Permissions (Unix only) ---

    #[cfg(unix)]
    #[test]
    fn test_config_file_permissions_error() {
        use std::os::unix::fs::PermissionsExt;
        let (svc, tmp) = setup();
        let path = tmp.path().join("computer_instances.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "test",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();
        let result = svc.add_config_for_instance(TEST_INSTANCE_ID, config);
        assert!(result.is_err());
    }
}
