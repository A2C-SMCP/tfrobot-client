use crate::services::client_computers::{ClientComputersPaths, GlobalConfigFile};
use crate::services::manager_context::ManagerContextKey;
use crate::services::manager_environment::ManagerEnvironment;
use crate::services::storage::{write_json_atomically, AtomicJsonWriteError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

fn default_diagnostic_retention_days() -> u32 {
    7
}

fn default_activity_retention_days() -> u32 {
    30
}

fn default_tool_history_retention_days() -> u32 {
    90
}

pub const MANAGER_SESSION_SCHEMA_VERSION: u32 = 3;
const CHAT_PREFERENCES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ChatPreferences {
    schema_version: u32,
    #[serde(default)]
    entries: Vec<ChatPreferenceEntry>,
}

impl Default for ChatPreferences {
    fn default() -> Self {
        Self {
            schema_version: CHAT_PREFERENCES_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChatPreferenceEntry {
    environment: ManagerEnvironment,
    account_id: String,
    organization_id: String,
    employee_id: u64,
}

impl ChatPreferenceEntry {
    fn matches(&self, context: &ManagerContextKey) -> bool {
        self.environment == context.environment
            && self.account_id == context.account_id
            && self.organization_id == context.organization_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub language: String,
    #[serde(default)]
    pub diagnostic_log_level: crate::services::observability::DiagnosticLevel,
    #[serde(default = "default_diagnostic_retention_days")]
    pub diagnostic_retention_days: u32,
    #[serde(default = "default_activity_retention_days")]
    pub activity_retention_days: u32,
    #[serde(default = "default_tool_history_retention_days")]
    pub tool_history_retention_days: u32,
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
    pub environment: ManagerEnvironment,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_phone: Option<String>,
    pub account_id: String,
    pub account_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_avatar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_no: Option<String>,
    /// Schema v2 did not persist the complete redacted identity. It is accepted only as a restore
    /// hint; `/auth/me` must fill every optional field before schema v3 is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
}

impl PersistedManagerSession {
    pub fn has_complete_identity(&self) -> bool {
        !self.user_id.trim().is_empty()
            && self.user_nickname.is_some()
            && self.user_email.is_some()
            && self.user_phone.is_some()
            && !self.account_id.trim().is_empty()
            && !self.account_name.trim().is_empty()
            && self.account_nickname.is_some()
            && self.account_avatar.is_some()
            && self.employee_no.is_some()
            && self
                .organization_id
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && self
                .organization_name
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && self
                .organization_type
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && self.permissions.is_some()
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
            diagnostic_log_level: crate::services::observability::DiagnosticLevel::Info,
            diagnostic_retention_days: default_diagnostic_retention_days(),
            activity_retention_days: default_activity_retention_days(),
            tool_history_retention_days: default_tool_history_retention_days(),
            custom_runtime_paths: CustomRuntimePaths::default(),
            manager_session: None,
            custom_path: None,
        }
    }
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.diagnostic_retention_days = self.diagnostic_retention_days.max(1);
        self.activity_retention_days = self.activity_retention_days.max(1);
        self.tool_history_retention_days = self.tool_history_retention_days.max(1);
    }
}

#[derive(Debug)]
pub struct SettingsService {
    settings_file: PathBuf,
    client_computers_paths: ClientComputersPaths,
    chat_preferences_lock: Mutex<()>,
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
            chat_preferences_lock: Mutex::new(()),
        }
    }

    pub fn load(&self) -> AppSettings {
        if !self.settings_file.exists() {
            return AppSettings::default();
        }
        let mut settings = fs::read_to_string(&self.settings_file)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|value| {
                let legacy_retention = value
                    .get("log_retention_days")
                    .and_then(|value| value.as_u64());
                let has_activity_retention = value.get("activity_retention_days").is_some();
                serde_json::from_value::<AppSettings>(value)
                    .ok()
                    .map(|mut settings| {
                        if !has_activity_retention {
                            if let Some(days) =
                                legacy_retention.and_then(|days| u32::try_from(days).ok())
                            {
                                settings.activity_retention_days = days.max(1);
                            }
                        }
                        settings.normalize();
                        settings
                    })
            })
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
        let value: serde_json::Value = serde_json::from_str(&content)?;
        let stored_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64);
        if stored_version == Some(1) {
            // Schema v1 stored an arbitrary base URL and numeric database account ID. Both are
            // incompatible with the environment-scoped, opaque-ID auth contract, so fail closed
            // and require one fresh login instead of reviving ambiguous credentials.
            return Ok(ManagerSessionConfig::default());
        }
        let mut config: ManagerSessionConfig = serde_json::from_value(value)?;
        if stored_version == Some(2) {
            // Schema v2 is not authenticated context: it has no organization identity. Preserve
            // it only long enough for restore to validate the JWT against live `/auth/me`.
            return Ok(config);
        }
        validate_manager_session_schema(&config)?;
        if config
            .session
            .as_ref()
            .is_some_and(|session| !session.has_complete_identity())
        {
            // Some pre-release schema-v3 clients persisted only the scoped identity fields. Treat
            // that metadata exactly like a v2 restore hint: it grants no authority by itself and
            // must be replaced from a successful live `/auth/me` response before use.
            config.schema_version = 2;
            return Ok(config);
        }
        validate_complete_manager_session(&config)?;
        Ok(config)
    }

    pub fn save_global_manager_session(
        &self,
        config: &ManagerSessionConfig,
    ) -> Result<(), ManagerSessionConfigError> {
        validate_manager_session_schema(config)?;
        validate_complete_manager_session(config)?;
        write_json_atomically(&self.global_manager_session_path(), config)?;
        Ok(())
    }

    pub fn global_manager_session_path(&self) -> PathBuf {
        self.client_computers_paths
            .global_config(GlobalConfigFile::ManagerSession)
    }

    pub fn load_recent_chat_employee(
        &self,
        context: &ManagerContextKey,
    ) -> Result<Option<u64>, ChatPreferencesError> {
        let _guard = self
            .chat_preferences_lock
            .lock()
            .map_err(|_| ChatPreferencesError::LockPoisoned)?;
        Ok(self
            .load_chat_preferences_unlocked()?
            .entries
            .into_iter()
            .find(|entry| entry.matches(context))
            .map(|entry| entry.employee_id))
    }

    pub fn save_recent_chat_employee(
        &self,
        context: &ManagerContextKey,
        employee_id: u64,
    ) -> Result<(), ChatPreferencesError> {
        let _guard = self
            .chat_preferences_lock
            .lock()
            .map_err(|_| ChatPreferencesError::LockPoisoned)?;
        let mut preferences = self.load_chat_preferences_unlocked()?;
        preferences.entries.retain(|entry| !entry.matches(context));
        preferences.entries.push(ChatPreferenceEntry {
            environment: context.environment,
            account_id: context.account_id.clone(),
            organization_id: context.organization_id.clone(),
            employee_id,
        });
        write_json_atomically(&self.chat_preferences_path(), &preferences)?;
        Ok(())
    }

    fn load_chat_preferences_unlocked(&self) -> Result<ChatPreferences, ChatPreferencesError> {
        let path = self.chat_preferences_path();
        if !path.exists() {
            return Ok(ChatPreferences::default());
        }
        let preferences: ChatPreferences = serde_json::from_str(&fs::read_to_string(path)?)?;
        if preferences.schema_version != CHAT_PREFERENCES_SCHEMA_VERSION {
            return Err(ChatPreferencesError::UnsupportedSchemaVersion {
                expected: CHAT_PREFERENCES_SCHEMA_VERSION,
                actual: preferences.schema_version,
            });
        }
        Ok(preferences)
    }

    fn chat_preferences_path(&self) -> PathBuf {
        self.client_computers_paths
            .global_config(GlobalConfigFile::ChatPreferences)
    }

    pub fn legacy_settings_path(&self) -> &std::path::Path {
        &self.settings_file
    }
}

fn validate_manager_session_schema(
    config: &ManagerSessionConfig,
) -> Result<(), ManagerSessionConfigError> {
    // Schema v2 remains writable only so the startup configuration migration can preserve an
    // existing restore hint. The Manager Context upgrades it to v3 after live `/auth/me` checks.
    if !matches!(config.schema_version, 2 | MANAGER_SESSION_SCHEMA_VERSION) {
        return Err(ManagerSessionConfigError::UnsupportedSchemaVersion {
            expected: MANAGER_SESSION_SCHEMA_VERSION,
            actual: config.schema_version,
        });
    }
    Ok(())
}

fn validate_complete_manager_session(
    config: &ManagerSessionConfig,
) -> Result<(), ManagerSessionConfigError> {
    if config.schema_version == MANAGER_SESSION_SCHEMA_VERSION
        && config
            .session
            .as_ref()
            .is_some_and(|session| !session.has_complete_identity())
    {
        return Err(ManagerSessionConfigError::IncompleteContext);
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

    #[error("Manager session metadata is missing complete user, account, organization, or permission identity")]
    IncompleteContext,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatPreferencesError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    AtomicJsonWrite(#[from] AtomicJsonWriteError),

    #[error("unsupported chat preferences schema version {actual}; expected {expected}")]
    UnsupportedSchemaVersion { expected: u32, actual: u32 },

    #[error("chat preferences lock is poisoned")]
    LockPoisoned,
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
        assert_eq!(settings.activity_retention_days, 30);
        assert!(matches!(settings.theme, ThemeMode::System));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let (svc, _tmp) = setup();
        let mut settings = svc.load();
        settings.language = "zh".to_string();
        settings.activity_retention_days = 7;
        settings.theme = ThemeMode::Dark;
        svc.save(&settings).unwrap();

        let loaded = svc.load();
        assert_eq!(loaded.language, "zh");
        assert_eq!(loaded.activity_retention_days, 7);
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
    fn migrates_legacy_log_retention_to_activity_retention() {
        let (svc, tmp) = setup();
        fs::write(
            tmp.path().join("settings.json"),
            r#"{"theme":"system","language":"en","log_retention_days":7,"custom_runtime_paths":{}}"#,
        )
        .unwrap();

        assert_eq!(svc.load().activity_retention_days, 7);
    }

    #[test]
    fn clamps_zero_retention_from_persisted_settings() {
        let (svc, tmp) = setup();
        fs::write(
            tmp.path().join("settings.json"),
            r#"{"theme":"system","language":"en","diagnostic_retention_days":0,"activity_retention_days":0,"tool_history_retention_days":0,"custom_runtime_paths":{}}"#,
        )
        .unwrap();

        let settings = svc.load();
        assert_eq!(settings.diagnostic_retention_days, 1);
        assert_eq!(settings.activity_retention_days, 1);
        assert_eq!(settings.tool_history_retention_days, 1);
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

    fn chat_context(
        environment: ManagerEnvironment,
        account_id: &str,
        organization_id: &str,
    ) -> ManagerContextKey {
        ManagerContextKey {
            environment,
            account_id: account_id.into(),
            organization_id: organization_id.into(),
        }
    }

    #[test]
    fn recent_chat_employee_roundtrips_and_isolates_manager_contexts() {
        let (svc, _tmp) = setup();
        let account_a = chat_context(ManagerEnvironment::Staging, "account-a", "organization-1");
        let account_b = chat_context(ManagerEnvironment::Staging, "account-b", "organization-1");
        let organization_b =
            chat_context(ManagerEnvironment::Staging, "account-a", "organization-2");
        let production = chat_context(ManagerEnvironment::Prod, "account-a", "organization-1");

        assert_eq!(svc.load_recent_chat_employee(&account_a).unwrap(), None);
        svc.save_recent_chat_employee(&account_a, 42).unwrap();
        svc.save_recent_chat_employee(&account_b, 77).unwrap();
        svc.save_recent_chat_employee(&organization_b, 88).unwrap();

        assert_eq!(svc.load_recent_chat_employee(&account_a).unwrap(), Some(42));
        assert_eq!(svc.load_recent_chat_employee(&account_b).unwrap(), Some(77));
        assert_eq!(
            svc.load_recent_chat_employee(&organization_b).unwrap(),
            Some(88)
        );
        assert_eq!(svc.load_recent_chat_employee(&production).unwrap(), None);

        svc.save_recent_chat_employee(&account_a, 43).unwrap();
        assert_eq!(svc.load_recent_chat_employee(&account_a).unwrap(), Some(43));
    }

    #[test]
    fn recent_chat_employee_rejects_unsupported_or_corrupt_preferences() {
        let (svc, _tmp) = setup();
        let context = chat_context(ManagerEnvironment::Beta, "account-a", "organization-1");
        std::fs::create_dir_all(svc.chat_preferences_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.chat_preferences_path(),
            r#"{"schema_version":99,"entries":[]}"#,
        )
        .unwrap();

        assert!(matches!(
            svc.load_recent_chat_employee(&context),
            Err(ChatPreferencesError::UnsupportedSchemaVersion { actual: 99, .. })
        ));

        std::fs::write(svc.chat_preferences_path(), "not json").unwrap();
        assert!(matches!(
            svc.load_recent_chat_employee(&context),
            Err(ChatPreferencesError::Json(_))
        ));
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
                environment: ManagerEnvironment::Staging,
                user_id: "7".to_string(),
                user_nickname: Some("Ada".to_string()),
                user_email: Some("ada@example.com".to_string()),
                user_phone: Some(String::new()),
                account_id: "org-legacy-1:account-7".to_string(),
                account_name: "client_uat".to_string(),
                account_nickname: Some("Ada".to_string()),
                account_avatar: Some("https://example.com/avatar.png".to_string()),
                employee_no: Some("E-7".to_string()),
                organization_id: Some("org-legacy-1".to_string()),
                organization_name: Some("Example Org".to_string()),
                organization_type: Some("enterprise".to_string()),
                permissions: Some(vec!["robot:read".to_string()]),
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
            r#"{"schema_version": 4, "session": null}"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_global_manager_session().unwrap_err(),
            ManagerSessionConfigError::UnsupportedSchemaVersion { .. }
        ));
    }

    #[test]
    fn global_manager_session_v1_requires_a_fresh_environment_scoped_login() {
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
                "accountName": "client_uat"
              }
            }"#,
        )
        .unwrap();

        assert_eq!(
            svc.load_global_manager_session().unwrap(),
            ManagerSessionConfig::default()
        );
    }

    #[test]
    fn global_manager_session_v2_is_only_a_restore_hint() {
        let (svc, _tmp) = setup();
        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{
              "schema_version": 2,
              "session": {
                "environment": "staging",
                "userId": 7,
                "accountId": "org-legacy-1:account-7",
                "accountName": "client_uat"
              }
            }"#,
        )
        .unwrap();

        let config = svc.load_global_manager_session().unwrap();
        assert_eq!(config.schema_version, 2);
        let session = config.session.unwrap();
        assert_eq!(session.user_id, "7");
        assert!(!session.has_complete_identity());
    }

    #[test]
    fn global_manager_session_incomplete_v3_becomes_restore_hint() {
        let (svc, _tmp) = setup();
        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{
              "schema_version": 3,
              "session": {
                "environment": "staging",
                "userId": "7",
                "userNickname": "Ada",
                "userEmail": "ada@example.com",
                "userPhone": "",
                "accountId": "org-legacy-1:account-7",
                "accountName": "client_uat",
                "accountNickname": "Ada",
                "accountAvatar": "",
                "employeeNo": "E-7",
                "permissions": []
              }
            }"#,
        )
        .unwrap();

        let config = svc.load_global_manager_session().unwrap();
        assert_eq!(config.schema_version, 2);
        assert!(!config.session.unwrap().has_complete_identity());
    }

    #[test]
    fn global_manager_session_v3_with_empty_scope_has_no_authority() {
        let (svc, _tmp) = setup();
        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{
              "schema_version": 3,
              "session": {
                "environment": "staging",
                "userId": "7",
                "userNickname": "Ada",
                "userEmail": "ada@example.com",
                "userPhone": "",
                "accountId": "  ",
                "accountName": "client_uat",
                "accountNickname": "Ada",
                "accountAvatar": "",
                "employeeNo": "E-7",
                "organizationId": "org-legacy-1",
                "organizationName": "Example Org",
                "organizationType": "enterprise",
                "permissions": []
              }
            }"#,
        )
        .unwrap();

        let config = svc.load_global_manager_session().unwrap();
        assert_eq!(config.schema_version, 2);
        assert!(!config.session.unwrap().has_complete_identity());
    }

    #[test]
    fn global_manager_session_partial_v3_becomes_restore_hint() {
        let (svc, _tmp) = setup();
        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{
              "schema_version": 3,
              "session": {
                "environment": "staging",
                "userId": "7",
                "userNickname": "Ada",
                "userEmail": "ada@example.com",
                "userPhone": "",
                "accountId": "org-legacy-1:account-7",
                "accountName": "client_uat",
                "accountNickname": "Ada",
                "accountAvatar": "",
                "employeeNo": "E-7",
                "organizationId": "org-legacy-1",
                "organizationName": "Example Org",
                "organizationType": "enterprise"
              }
            }"#,
        )
        .unwrap();

        let config = svc.load_global_manager_session().unwrap();
        assert_eq!(config.schema_version, 2);
        assert!(!config.session.unwrap().has_complete_identity());
    }

    #[test]
    fn global_manager_session_rejects_nested_secret_fields() {
        let (svc, _tmp) = setup();
        std::fs::create_dir_all(svc.global_manager_session_path().parent().unwrap()).unwrap();
        std::fs::write(
            svc.global_manager_session_path(),
            r#"{
              "schema_version": 2,
              "session": {
                "environment": "staging",
                "userId": 7,
                "accountId": "org-legacy-1:",
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
