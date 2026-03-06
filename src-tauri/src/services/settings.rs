use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub language: String,
    pub log_retention_days: u32,
    pub custom_runtime_paths: CustomRuntimePaths,
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

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: "en".to_string(),
            log_retention_days: 30,
            custom_runtime_paths: CustomRuntimePaths::default(),
        }
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
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
}
