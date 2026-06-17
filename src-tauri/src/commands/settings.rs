use crate::services::{settings::AppSettings, skills};
use crate::AppState;
use serde::Serialize;
use tauri::State;

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings_service.load())
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    update_settings_core(&state, settings).await
}

pub async fn update_settings_core(state: &AppState, settings: AppSettings) -> Result<(), String> {
    let previous_settings = state.settings_service.load();
    let previous_skill_root = skills::expand_home(&previous_settings.skills_root_dir);

    state
        .settings_service
        .save(&settings)
        .map_err(|e| e.to_string())?;
    let saved_settings = state.settings_service.load();

    if previous_skill_root != skills::expand_home(&saved_settings.skills_root_dir) {
        if let Err(e) = state
            .runtime
            .refresh_skill_sync_summary(&saved_settings)
            .await
        {
            if let Err(restore_error) = state.settings_service.save(&previous_settings) {
                log::warn!(
                    "Failed to restore previous settings after skill sync error: {}",
                    restore_error
                );
            }
            apply_path_setting(&previous_settings);
            return Err(e);
        }
    }

    state.runtime.store_settings(&saved_settings);
    apply_path_setting(&saved_settings);

    Ok(())
}

fn apply_path_setting(settings: &AppSettings) {
    match &settings.custom_path {
        Some(path) if !path.is_empty() => {
            std::env::set_var("PATH", path);
            log::info!("PATH updated by user setting");
        }
        _ => {
            // Revert to auto-detected PATH
            let detected = crate::services::shell_env::get_detected_path();
            std::env::set_var("PATH", &detected);
            log::info!("PATH reverted to auto-detected");
        }
    }
}

#[tauri::command]
pub async fn get_detected_path() -> Result<String, String> {
    Ok(crate::services::shell_env::get_detected_path())
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub available: bool,
}

#[tauri::command]
pub async fn detect_runtimes() -> Result<Vec<RuntimeInfo>, String> {
    Ok(vec![
        detect_one("Node.js", "node", &["--version"]),
        detect_one("Python", "python3", &["--version"]),
        detect_one("uv", "uv", &["--version"]),
        detect_one("pnpm", "pnpm", &["--version"]),
    ])
}

fn detect_one(name: &str, cmd: &str, version_args: &[&str]) -> RuntimeInfo {
    let path = which::which(cmd)
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let version = if path.is_some() {
        std::process::Command::new(cmd)
            .args(version_args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|v| v.trim().to_string())
    } else {
        None
    };

    RuntimeInfo {
        name: name.to_string(),
        available: path.is_some(),
        path,
        version,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: String,
    pub smcp_computer_version: String,
}

#[tauri::command]
pub async fn get_app_info() -> Result<AppInfo, String> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        smcp_computer_version: smcp_computer::VERSION.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::logger::LogService;
    use crate::services::settings::{SettingsService, ThemeMode};

    fn test_state(tmp_path: &std::path::Path) -> AppState {
        AppState::new(
            ConfigService::new(tmp_path.to_path_buf()).expect("config service"),
            LogService::new(tmp_path).expect("log service"),
            SettingsService::new(tmp_path.to_path_buf()),
        )
    }

    #[tokio::test]
    async fn update_settings_syncs_skill_root_without_rebuilding_computer() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let old_home = tmp.path().join("old-skills");
        let new_home = tmp.path().join("new-skills");
        std::fs::create_dir_all(&old_home).expect("old skills root");
        std::fs::create_dir_all(&new_home).expect("new skills root");
        let settings_service = SettingsService::new(tmp.path().to_path_buf());
        settings_service
            .save(&AppSettings {
                skills_root_dir: old_home.to_string_lossy().to_string(),
                ..AppSettings::default()
            })
            .expect("save initial settings");

        let state = test_state(tmp.path());
        let original_computer = state.runtime.computer();
        let next = AppSettings {
            theme: ThemeMode::System,
            language: "en".to_string(),
            log_retention_days: 30,
            custom_runtime_paths: Default::default(),
            computer_name: "tfrobot-client".to_string(),
            skills_root_dir: new_home.to_string_lossy().to_string(),
            custom_path: None,
        };

        update_settings_core(&state, next.clone())
            .await
            .expect("update settings");

        assert_eq!(
            state.settings_service.load().skills_root_dir,
            next.skills_root_dir
        );
        assert_eq!(state.runtime.local_skill_root(), new_home);
        assert!(std::sync::Arc::ptr_eq(
            &original_computer,
            &state.runtime.computer()
        ));
    }

    #[tokio::test]
    async fn update_settings_ignores_custom_computer_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let settings_service = SettingsService::new(tmp.path().to_path_buf());
        settings_service
            .save(&AppSettings::default())
            .expect("save initial settings");

        let state = test_state(tmp.path());
        let next = AppSettings {
            computer_name: "local-computer".to_string(),
            ..AppSettings::default()
        };

        update_settings_core(&state, next.clone())
            .await
            .expect("update settings");

        assert_eq!(
            state.settings_service.load().computer_name,
            "tfrobot-client"
        );
        assert_eq!(state.runtime.computer_name(), "tfrobot-client");
    }

    #[tokio::test]
    async fn update_settings_skill_sync_failure_restores_persisted_settings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let old_home = tmp.path().join("old-skills");
        let invalid_home = tmp.path().join("not-a-directory");
        std::fs::write(&invalid_home, "file blocks skill home directory").expect("write file");
        let settings_service = SettingsService::new(tmp.path().to_path_buf());
        let previous = AppSettings {
            skills_root_dir: old_home.to_string_lossy().to_string(),
            ..AppSettings::default()
        };
        settings_service
            .save(&previous)
            .expect("save initial settings");

        let state = test_state(tmp.path());
        let next = AppSettings {
            skills_root_dir: invalid_home.to_string_lossy().to_string(),
            ..previous.clone()
        };

        let err = update_settings_core(&state, next)
            .await
            .expect_err("invalid skill root should fail skill sync");

        assert!(err.contains("Not a directory") || err.contains("not a directory"));
        assert_eq!(
            state.settings_service.load().skills_root_dir,
            previous.skills_root_dir
        );
        assert_eq!(state.runtime.local_skill_root(), old_home);
    }
}
