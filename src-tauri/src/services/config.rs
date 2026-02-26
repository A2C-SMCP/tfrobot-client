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

fn save_json_file<T: serde::Serialize + ?Sized>(path: &PathBuf, data: &T) -> Result<(), ConfigError> {
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
