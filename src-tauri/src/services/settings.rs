use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub language: String,
    pub log_retention_days: u32,
    pub custom_runtime_paths: CustomRuntimePaths,
    /// Local root directory where user skills are stored.
    #[serde(default = "default_skills_root_dir")]
    pub skills_root_dir: String,
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

fn default_skills_root_dir() -> String {
    "~/.a2c/skills".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: "en".to_string(),
            log_retention_days: 30,
            custom_runtime_paths: CustomRuntimePaths::default(),
            skills_root_dir: default_skills_root_dir(),
            custom_path: None,
        }
    }
}

impl AppSettings {
    fn normalized(mut self) -> Self {
        if self.skills_root_dir.trim().is_empty() {
            self.skills_root_dir = default_skills_root_dir();
        }
        self
    }
}

pub struct SettingsService {
    settings_file: PathBuf,
}

impl SettingsService {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            settings_file: app_data_dir.join("settings.json"),
        }
    }

    pub fn load(&self) -> AppSettings {
        if !self.settings_file.exists() {
            return AppSettings::default();
        }
        fs::read_to_string(&self.settings_file)
            .ok()
            .and_then(|content| serde_json::from_str::<AppSettings>(&content).ok())
            .unwrap_or_default()
            .normalized()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(&settings.clone().normalized())
            .map_err(std::io::Error::other)?;
        fs::write(&self.settings_file, content)
    }
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
        assert_eq!(settings.skills_root_dir, "~/.a2c/skills");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let (svc, _tmp) = setup();
        let mut settings = svc.load();
        settings.language = "zh".to_string();
        settings.log_retention_days = 7;
        settings.theme = ThemeMode::Dark;
        settings.skills_root_dir = "/Users/test/.codex/skills".to_string();
        svc.save(&settings).unwrap();

        let loaded = svc.load();
        assert_eq!(loaded.language, "zh");
        assert_eq!(loaded.log_retention_days, 7);
        assert!(matches!(loaded.theme, ThemeMode::Dark));
        assert_eq!(loaded.skills_root_dir, "/Users/test/.codex/skills");
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
    fn test_load_legacy_settings_uses_default_skills_root_dir() {
        let (svc, tmp) = setup();
        fs::write(
            tmp.path().join("settings.json"),
            r#"{
                "theme": "light",
                "language": "zh",
                "log_retention_days": 14,
                "custom_runtime_paths": {}
            }"#,
        )
        .unwrap();

        let settings = svc.load();
        assert_eq!(settings.language, "zh");
        assert_eq!(settings.log_retention_days, 14);
        assert!(matches!(settings.theme, ThemeMode::Light));
        assert_eq!(settings.skills_root_dir, "~/.a2c/skills");
    }

    #[test]
    fn test_load_empty_skills_root_dir_uses_default() {
        let (svc, tmp) = setup();
        fs::write(
            tmp.path().join("settings.json"),
            r#"{
                "theme": "system",
                "language": "en",
                "log_retention_days": 30,
                "custom_runtime_paths": {},
                "skills_root_dir": "   "
            }"#,
        )
        .unwrap();

        let settings = svc.load();
        assert_eq!(settings.skills_root_dir, "~/.a2c/skills");
    }
}
