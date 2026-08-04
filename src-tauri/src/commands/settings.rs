use crate::services::observability::ObservabilityRetention;
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
    // Persist first: retention cleanup is destructive and must never run for a setting that could
    // not be committed atomically.
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
