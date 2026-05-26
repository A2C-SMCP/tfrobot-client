use crate::commands::connection::ConnectionProfile;
use crate::commands::inputs::InputDefinition;
use smcp_computer::mcp_clients::MCPServerConfig;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Service for persisting all configuration data to disk
pub struct ConfigService {
    config_dir: PathBuf,
    servers_file: PathBuf,
    inputs_file: PathBuf,
    input_values_file: PathBuf,
    profiles_file: PathBuf,
}

impl ConfigService {
    /// Create a new ConfigService with the given app data directory
    pub fn new(app_data_dir: PathBuf) -> Result<Self, std::io::Error> {
        fs::create_dir_all(&app_data_dir)?;

        Ok(Self {
            servers_file: app_data_dir.join("mcp_servers.json"),
            inputs_file: app_data_dir.join("inputs.json"),
            input_values_file: app_data_dir.join("input_values.json"),
            profiles_file: app_data_dir.join("connection_profiles.json"),
            config_dir: app_data_dir,
        })
    }

    // --- MCP Server Configs ---

    pub fn load_configs(&self) -> Result<Vec<MCPServerConfig>, ConfigError> {
        load_json_file(&self.servers_file)
    }

    pub fn save_configs(&self, configs: &[MCPServerConfig]) -> Result<(), ConfigError> {
        save_json_file(&self.servers_file, configs)
    }

    pub fn add_config(&self, config: MCPServerConfig) -> Result<(), ConfigError> {
        let mut configs = self.load_configs()?;
        let name = config.name().to_string();
        configs.retain(|c| c.name() != name);
        configs.push(config);
        self.save_configs(&configs)
    }

    pub fn remove_config(&self, name: &str) -> Result<(), ConfigError> {
        let mut configs = self.load_configs()?;
        let original_len = configs.len();
        configs.retain(|c| c.name() != name);
        if configs.len() == original_len {
            return Err(ConfigError::NotFound(name.to_string()));
        }
        self.save_configs(&configs)
    }

    // --- Input Definitions ---

    pub fn load_inputs(&self) -> Result<Vec<InputDefinition>, ConfigError> {
        load_json_file(&self.inputs_file)
    }

    pub fn save_inputs(&self, inputs: &[InputDefinition]) -> Result<(), ConfigError> {
        save_json_file(&self.inputs_file, inputs)
    }

    // --- Input Values ---

    pub fn load_input_values(&self) -> Result<HashMap<String, serde_json::Value>, ConfigError> {
        load_json_file(&self.input_values_file)
    }

    pub fn save_input_values(
        &self,
        values: &HashMap<String, serde_json::Value>,
    ) -> Result<(), ConfigError> {
        save_json_file(&self.input_values_file, values)
    }

    // --- Connection Profiles ---

    pub fn load_profiles(&self) -> Result<Vec<ConnectionProfile>, ConfigError> {
        load_json_file(&self.profiles_file)
    }

    pub fn save_profiles(&self, profiles: &[ConnectionProfile]) -> Result<(), ConfigError> {
        save_json_file(&self.profiles_file, profiles)
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }
}

fn load_json_file<T: serde::de::DeserializeOwned + Default>(
    path: &PathBuf,
) -> Result<T, ConfigError> {
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

fn save_json_file<T: serde::Serialize + ?Sized>(
    path: &PathBuf,
    data: &T,
) -> Result<(), ConfigError> {
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

    #[error("Server not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (ConfigService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = ConfigService::new(tmp.path().to_path_buf()).unwrap();
        (svc, tmp)
    }

    // --- MCP Server Configs ---

    #[test]
    fn test_load_empty_configs() {
        let (svc, _tmp) = setup();
        let configs = svc.load_configs().unwrap();
        assert!(configs.is_empty());
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

        svc.save_configs(std::slice::from_ref(&config)).unwrap();
        let loaded = svc.load_configs().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name(), "test-server");
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

        svc.add_config(config).unwrap();
        let loaded = svc.load_configs().unwrap();
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

        svc.add_config(config1).unwrap();
        svc.add_config(config2).unwrap();

        let loaded = svc.load_configs().unwrap();
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
    fn test_remove_config() {
        let (svc, _tmp) = setup();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "to-remove",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();

        svc.add_config(config).unwrap();
        svc.remove_config("to-remove").unwrap();

        let loaded = svc.load_configs().unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_config_returns_error() {
        let (svc, _tmp) = setup();
        let result = svc.remove_config("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_load_corrupted_json_file() {
        let (svc, tmp) = setup();
        std::fs::write(tmp.path().join("mcp_servers.json"), "not json").unwrap();
        let result = svc.load_configs();
        assert!(result.is_err());
    }

    #[test]
    fn test_load_empty_json_file_returns_default() {
        let (svc, tmp) = setup();
        std::fs::write(tmp.path().join("mcp_servers.json"), "").unwrap();
        let configs = svc.load_configs().unwrap();
        assert!(configs.is_empty());
    }

    // --- Input Definitions ---

    #[test]
    fn test_load_empty_inputs() {
        let (svc, _tmp) = setup();
        let inputs = svc.load_inputs().unwrap();
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

        svc.save_inputs(&[input]).unwrap();
        let loaded = svc.load_inputs().unwrap();
        assert_eq!(loaded.len(), 1);
    }

    // --- Input Values ---

    #[test]
    fn test_load_empty_input_values() {
        let (svc, _tmp) = setup();
        let values = svc.load_input_values().unwrap();
        assert!(values.is_empty());
    }

    #[test]
    fn test_save_and_load_input_values_roundtrip() {
        let (svc, _tmp) = setup();
        let mut values = HashMap::new();
        values.insert("key1".to_string(), serde_json::json!("value1"));
        values.insert("key2".to_string(), serde_json::json!(42));

        svc.save_input_values(&values).unwrap();
        let loaded = svc.load_input_values().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["key1"], serde_json::json!("value1"));
        assert_eq!(loaded["key2"], serde_json::json!(42));
    }

    // --- Connection Profiles ---

    #[test]
    fn test_load_empty_profiles() {
        let (svc, _tmp) = setup();
        let profiles = svc.load_profiles().unwrap();
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
            "headers": {},
            "auto_connect": true,
            "auto_reconnect": true
        }))
        .unwrap();

        svc.save_profiles(&[profile]).unwrap();
        let loaded = svc.load_profiles().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "prod");
        assert_eq!(loaded[0].url, "https://smcp.example.com");
    }

    // --- File Permissions (Unix only) ---

    #[cfg(unix)]
    #[test]
    fn test_config_file_permissions_error() {
        use std::os::unix::fs::PermissionsExt;
        let (svc, tmp) = setup();
        let path = tmp.path().join("mcp_servers.json");
        std::fs::write(&path, "[]").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "test",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();
        let result = svc.add_config(config);
        assert!(result.is_err());
    }
}
