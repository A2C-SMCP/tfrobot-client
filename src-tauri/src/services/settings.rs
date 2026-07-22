use crate::services::client_computers::{ClientComputersPaths, GlobalConfigFile};
use crate::services::storage::{write_json_atomically, AtomicJsonWriteError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const MANAGER_SESSION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub language: String,
    pub log_retention_days: u32,
    pub custom_runtime_paths: CustomRuntimePaths,
    #[serde(default, skip_serializing)]
    pub manager_session: Option<ManagerSessionSettings>,
    /// User-configured PATH override. When set, takes priority over auto-detected PATH.
    #[serde(default)]
    pub custom_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomRuntimePaths {
    pub node: Option<String>,
    pub python: Option<String>,
    pub uv: Option<String>,
    pub pnpm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSessionSettings {
    pub base_url: String,
    pub user_id: u64,
    pub account_id: u64,
    pub account_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManagerSessionConfig {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<PersistedManagerSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedManagerSession {
    pub base_url: String,
    pub user_id: u64,
    pub account_id: u64,
    pub account_name: String,
}

impl From<&ManagerSessionSettings> for PersistedManagerSession {
    fn from(session: &ManagerSessionSettings) -> Self {
        Self {
            base_url: session.base_url.clone(),
            user_id: session.user_id,
            account_id: session.account_id,
            account_name: session.account_name.clone(),
        }
    }
}

impl Default for ManagerSessionConfig {
    fn default() -> Self {
        Self {
            schema_version: MANAGER_SESSION_SCHEMA_VERSION,
            session: None,
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: "en".to_string(),
            log_retention_days: 30,
            custom_runtime_paths: CustomRuntimePaths::default(),
            manager_session: None,
            custom_path: None,
        }
    }
}

pub struct SettingsService {
    settings_file: PathBuf,
    client_computers_paths: ClientComputersPaths,
}

impl SettingsService {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let client_computers_paths = ClientComputersPaths::from_app_data_dir(&app_data_dir);
        Self::new_with_client_computers_paths(app_data_dir, client_computers_paths)
    }

    pub fn new_with_client_computers_paths(
        app_data_dir: PathBuf,
        client_computers_paths: ClientComputersPaths,
    ) -> Self {
        Self {
            settings_file: app_data_dir.join("settings.json"),
            client_computers_paths,
        }
    }

    pub fn load(&self) -> AppSettings {
        if !self.settings_file.exists() {
            return AppSettings::default();
        }
        let mut settings: AppSettings = fs::read_to_string(&self.settings_file)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();
        // Legacy Manager metadata is migration-only and is never an active settings source.
        settings.manager_session = None;
        settings
    }

    pub fn load_legacy_for_migration(&self) -> Result<AppSettings, ManagerSessionConfigError> {
        if !self.settings_file.exists() {
            return Ok(AppSettings::default());
        }
        let content = fs::read_to_string(&self.settings_file)?;
        if content.trim().is_empty() {
            return Err(ManagerSessionConfigError::EmptyFile(
                self.settings_file.clone(),
            ));
        }
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        write_json_atomically(&self.settings_file, settings).map_err(std::io::Error::other)
    }

    pub fn load_global_manager_session(
        &self,
    ) -> Result<ManagerSessionConfig, ManagerSessionConfigError> {
        let path = self.global_manager_session_path();
        if !path.exists() {
            return Ok(ManagerSessionConfig::default());
        }
        let content = fs::read_to_string(&path)?;
        if content.trim().is_empty() {
            return Err(ManagerSessionConfigError::EmptyFile(path));
        }
        let config: ManagerSessionConfig = serde_json::from_str(&content)?;
        validate_manager_session_schema(&config)?;
        Ok(config)
    }

    pub fn save_global_manager_session(
        &self,
        config: &ManagerSessionConfig,
    ) -> Result<(), ManagerSessionConfigError> {
        validate_manager_session_schema(config)?;
        write_json_atomically(&self.global_manager_session_path(), config)?;
        Ok(())
    }

    pub fn global_manager_session_path(&self) -> PathBuf {
        self.client_computers_paths
            .global_config(GlobalConfigFile::ManagerSession)
    }

    pub fn legacy_settings_path(&self) -> &std::path::Path {
        &self.settings_file
    }
}

fn validate_manager_session_schema(
    config: &ManagerSessionConfig,
) -> Result<(), ManagerSessionConfigError> {
    if config.schema_version != MANAGER_SESSION_SCHEMA_VERSION {
        return Err(ManagerSessionConfigError::UnsupportedSchemaVersion {
            expected: MANAGER_SESSION_SCHEMA_VERSION,
            actual: config.schema_version,
        });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerSessionConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    AtomicJsonWrite(#[from] AtomicJsonWriteError),

    #[error("manager session config is empty: {0}")]
    EmptyFile(PathBuf),

    #[error("unsupported manager session schema version {actual}; expected {expected}")]
    UnsupportedSchemaVersion { expected: u32, actual: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (SettingsService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = SettingsService::new(tmp.path().to_path_buf());
        (svc, tmp)
    }

    #[test]
    fn test_load_default_settings() {
        let (svc, _tmp) = setup();
        let settings = svc.load();
        assert_eq!(settings.language, "en");
        assert_eq!(settings.log_retention_days, 30);
        assert!(matches!(settings.theme, ThemeMode::System));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let (svc, _tmp) = setup();
        let mut settings = svc.load();
        settings.language = "zh".to_string();
        settings.log_retention_days = 7;
        settings.theme = ThemeMode::Dark;
        svc.save(&settings).unwrap();

        let loaded = svc.load();
        assert_eq!(loaded.language, "zh");
        assert_eq!(loaded.log_retention_days, 7);
        assert!(matches!(loaded.theme, ThemeMode::Dark));
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let (svc, _tmp) = setup();
        let settings = svc.load();
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_load_corrupted_file_returns_defaults() {
        let (svc, tmp) = setup();
        fs::write(tmp.path().join("settings.json"), "invalid json").unwrap();
        let settings = svc.load();
        // Should fall back to defaults
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_custom_runtime_paths() {
        let (svc, _tmp) = setup();
        let mut settings = svc.load();
        settings.custom_runtime_paths.node = Some("/usr/local/bin/node".to_string());
        svc.save(&settings).unwrap();

        let loaded = svc.load();
        assert_eq!(
            loaded.custom_runtime_paths.node.as_deref(),
            Some("/usr/local/bin/node")
        );
    }

    #[test]
    fn normal_settings_save_does_not_recreate_legacy_manager_session() {
        let (svc, _tmp) = setup();
        let mut settings = svc.load();
        settings.manager_session = Some(ManagerSessionSettings {
            base_url: "https://manager.example.com".to_string(),
            user_id: 7,
            account_id: 42,
            account_name: "client_uat".to_string(),
        });
        svc.save(&settings).unwrap();

        assert!(svc.load().manager_session.is_none());
        assert!(!std::fs::read_to_string(svc.legacy_settings_path())
            .unwrap()
            .contains("manager_session"));
    }

    #[test]
    fn strict_legacy_loader_reads_manager_session_and_rejects_corruption() {
        let (svc, _tmp) = setup();
        std::fs::write(
            svc.legacy_settings_path(),
            r#"{
                "theme":"system",
                "language":"en",
                "log_retention_days":30,
                "custom_runtime_paths":{},
                "manager_session":{
                    "baseUrl":"https://manager.example.com",
                    "userId":7,
                    "accountId":42,
                    "accountName":"client_uat"
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            svc.load_legacy_for_migration()
                .unwrap()
                .manager_session
                .unwrap()
                .account_id,
            42
        );

        std::fs::write(svc.legacy_settings_path(), "not json").unwrap();
        assert!(matches!(
            svc.load_legacy_for_migration().unwrap_err(),
            ManagerSessionConfigError::Json(_)
        ));
    }

    #[test]
    fn global_manager_session_roundtrip_is_separate_from_app_settings() {
        let (svc, tmp) = setup();
        let config = ManagerSessionConfig {
            schema_version: MANAGER_SESSION_SCHEMA_VERSION,
            session: Some(PersistedManagerSession {
                base_url: "https://manager.example.com".to_string(),
                user_id: 7,
                account_id: 42,
                account_name: "client_uat".to_string(),
            }),
        };

        svc.save_global_manager_session(&config).unwrap();

        assert_eq!(svc.load_global_manager_session().unwrap(), config);
        assert_eq!(
            svc.global_manager_session_path(),
            tmp.path()
                .join("client_computers/global/manager_session.json")
        );
        assert!(!tmp.path().join("settings.json").exists());
    }

    #[test]
    fn global_manager_session_rejects_unknown_schema_version() {
        let (svc, _tmp) = setup();
        let config = ManagerSessionConfig {
            schema_version: MANAGER_SESSION_SCHEMA_VERSION + 1,
            session: None,
        };

        assert!(matches!(
            svc.save_global_manager_session(&config).unwrap_err(),
            ManagerSessionConfigError::UnsupportedSchemaVersion { .. }
        ));

        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{"schema_version": 2, "session": null}"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_global_manager_session().unwrap_err(),
            ManagerSessionConfigError::UnsupportedSchemaVersion { .. }
        ));
    }

    #[test]
    fn global_manager_session_rejects_nested_secret_fields() {
        let (svc, _tmp) = setup();
        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{
              "schema_version": 1,
              "session": {
                "baseUrl": "https://manager.example.com",
                "userId": 7,
                "accountId": 42,
                "accountName": "client_uat",
                "jwt": "plaintext"
              }
            }"#,
        )
        .unwrap();

        assert!(matches!(
            svc.load_global_manager_session().unwrap_err(),
            ManagerSessionConfigError::Json(_)
        ));
    }
}
