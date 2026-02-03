use smcp_computer::mcp_clients::MCPServerConfig;
use std::fs;
use std::path::PathBuf;

/// Service for persisting MCP server configurations to disk
pub struct ConfigService {
    config_dir: PathBuf,
    config_file: PathBuf,
}

impl ConfigService {
    /// Create a new ConfigService with the given app data directory
    pub fn new(app_data_dir: PathBuf) -> Result<Self, std::io::Error> {
        // Ensure the config directory exists
        fs::create_dir_all(&app_data_dir)?;

        let config_file = app_data_dir.join("mcp_servers.json");

        Ok(Self {
            config_dir: app_data_dir,
            config_file,
        })
    }

    /// Load MCP server configurations from disk
    pub fn load_configs(&self) -> Result<Vec<MCPServerConfig>, ConfigError> {
        if !self.config_file.exists() {
            return Ok(vec![]);
        }

        let content = fs::read_to_string(&self.config_file)?;
        let configs: Vec<MCPServerConfig> = serde_json::from_str(&content)?;
        Ok(configs)
    }

    /// Save MCP server configurations to disk
    pub fn save_configs(&self, configs: &[MCPServerConfig]) -> Result<(), ConfigError> {
        let content = serde_json::to_string_pretty(configs)?;
        fs::write(&self.config_file, content)?;
        Ok(())
    }

    /// Add a new server configuration (or update if exists)
    pub fn add_config(&self, config: MCPServerConfig) -> Result<(), ConfigError> {
        let mut configs = self.load_configs()?;
        let name = config.name().to_string();

        // Remove existing config with same name if present
        configs.retain(|c| c.name() != name);
        configs.push(config);

        self.save_configs(&configs)
    }

    /// Remove a server configuration by name
    pub fn remove_config(&self, name: &str) -> Result<(), ConfigError> {
        let mut configs = self.load_configs()?;
        let original_len = configs.len();
        configs.retain(|c| c.name() != name);

        if configs.len() == original_len {
            return Err(ConfigError::NotFound(name.to_string()));
        }

        self.save_configs(&configs)
    }

    /// Get the config directory path
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }
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
