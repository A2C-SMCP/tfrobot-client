use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityOutcome, ObservabilityRetention,
};
use crate::services::settings::AppSettings;
use crate::AppState;
use serde::Serialize;
use tauri::{Manager, State};

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings_service.load())
}

#[tauri::command]
pub async fn update_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> Result<(), String> {
    settings.normalize();
    update_settings_core(&state, settings.clone()).await?;
    if let Ok(log_dir) = app.path().app_log_dir() {
        crate::cleanup_old_log_files(&log_dir, settings.diagnostic_retention_days as u64);
    }

    // Apply custom PATH change immediately (no restart needed)
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
    Ok(())
}

async fn update_settings_core(state: &AppState, mut settings: AppSettings) -> Result<(), String> {
    let started = std::time::Instant::now();
    let correlation_id = uuid::Uuid::new_v4().to_string();
    settings.normalize();
    let previous = state.settings_service.load();
    let changed_keys = changed_setting_keys(&previous, &settings);
    let result = async {
        // Persist first: retention cleanup is destructive and must never run for a setting that
        // could not be committed atomically.
        state
            .settings_service
            .save(&settings)
            .map_err(|error| error.to_string())?;
        state.diagnostics.set_level(settings.diagnostic_log_level);
        let retention = ObservabilityRetention {
            activity_days: settings.activity_retention_days,
            tool_history_days: settings.tool_history_retention_days,
        };
        let observability = state.observability.as_ref().clone();
        tauri::async_runtime::spawn_blocking(move || observability.apply_retention(retention))
            .await
            .map_err(|error| error.to_string())??;
        Ok::<(), String>(())
    }
    .await;

    if !changed_keys.is_empty() || result.is_err() {
        let (level, outcome, message, error) = match &result {
            Ok(()) => (
                ActivityLevel::Info,
                ActivityOutcome::Succeeded,
                "Application settings updated",
                None,
            ),
            Err(error) => (
                ActivityLevel::Warn,
                ActivityOutcome::Failed,
                "Application settings update failed",
                Some(redact_text(error)),
            ),
        };
        let mut activity = ActivityEventDraft::client(
            level,
            "config",
            "application_settings",
            "update",
            outcome,
            message,
        );
        activity.fields = Some(serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "trigger": "user",
            "changed_keys": changed_keys,
            "duration_ms": started.elapsed().as_millis(),
            "error": error,
        }));
        activity.correlation_id = Some(correlation_id);
        if let Err(error) = state.observability.record_activity_async(activity).await {
            log::error!("failed to persist settings activity: {error}");
        }
    }
    result
}

fn changed_setting_keys(previous: &AppSettings, next: &AppSettings) -> Vec<String> {
    let previous = serde_json::to_value(previous).unwrap_or_default();
    let next = serde_json::to_value(next).unwrap_or_default();
    let (Some(previous), Some(next)) = (previous.as_object(), next.as_object()) else {
        return Vec::new();
    };
    let mut keys = previous
        .keys()
        .chain(next.keys())
        .filter(|key| previous.get(*key) != next.get(*key))
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
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
        smcp_computer_version: a2c_smcp::smcp_computer::VERSION.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::keychain::InMemorySecretStore;
    use crate::services::observability::{
        ActivityQuery, ActivityScopeFilter, ObservabilityService,
    };
    use crate::services::settings::SettingsService;
    use tempfile::tempdir;

    #[test]
    fn changed_setting_keys_reports_names_without_values() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.activity_retention_days = 7;
        next.custom_path = Some("/sensitive/custom/path".to_string());

        assert_eq!(
            changed_setting_keys(&previous, &next),
            vec!["activity_retention_days", "custom_path"]
        );
    }

    #[test]
    fn changed_setting_keys_ignores_unchanged_settings() {
        let settings = AppSettings::default();
        assert!(changed_setting_keys(&settings, &settings).is_empty());
    }

    #[tokio::test]
    async fn settings_update_persists_changed_keys_and_retention_activity() {
        let dir = tempdir().unwrap();
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );
        let settings = AppSettings {
            activity_retention_days: 7,
            ..AppSettings::default()
        };

        update_settings_core(&state, settings).await.unwrap();

        let activity = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::ClientOnly,
                ..ActivityQuery::default()
            })
            .unwrap();
        assert_eq!(activity.total, 1);
        assert_eq!(activity.items[0].event_type, "application_settings");
        assert_eq!(activity.items[0].outcome, ActivityOutcome::Succeeded);
        assert_eq!(
            activity.items[0]
                .fields
                .as_ref()
                .and_then(|fields| fields["changed_keys"].as_array())
                .and_then(|keys| keys.first())
                .and_then(serde_json::Value::as_str),
            Some("activity_retention_days")
        );
    }
}
