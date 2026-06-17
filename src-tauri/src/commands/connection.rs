use crate::services::settings::default_computer_name;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Connection Profile stored to disk (API Key stored separately in Keychain)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub url: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub office_id: String,
    pub computer_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
}

fn default_true() -> bool {
    true
}

fn default_namespace() -> String {
    "/smcp".to_string()
}

/// Connection status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatusInfo {
    pub connected: bool,
    pub url: Option<String>,
    pub office_id: Option<String>,
    pub computer_name: Option<String>,
    pub connected_at: Option<String>,
    pub profile_name: Option<String>,
}

/// List all saved connection profiles
#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ConnectionProfile>, String> {
    Ok(state
        .config
        .load_profiles()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(normalize_profile)
        .collect())
}

/// Save (create or update) a connection profile
#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    api_key: Option<String>,
) -> Result<(), String> {
    let profile = normalize_profile(profile);
    let name = profile.name.clone();
    log::info!("Saving connection profile: {}", name);

    if let Some(key) = &api_key {
        if !key.is_empty() {
            let keychain_id = format!("profile:{}", name);
            crate::services::keychain::save_credential(&keychain_id, key)
                .map_err(|e| e.to_string())?;
        }
    }

    let mut profiles = state.config.load_profiles().map_err(|e| e.to_string())?;
    profiles.retain(|p| p.name != name);
    profiles.push(profile);
    state
        .config
        .save_profiles(&profiles)
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn normalize_profile(mut profile: ConnectionProfile) -> ConnectionProfile {
    profile.computer_name = default_computer_name();
    profile
}

/// Delete a connection profile
#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Deleting connection profile: {}", name);

    let keychain_id = format!("profile:{}", name);
    let _ = crate::services::keychain::delete_credential(&keychain_id);

    let mut profiles = state.config.load_profiles().map_err(|e| e.to_string())?;
    profiles.retain(|p| p.name != name);
    state
        .config
        .save_profiles(&profiles)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Connect the shared app Computer to SMCP using a saved profile.
#[tauri::command]
pub async fn connect_smcp(state: State<'_, AppState>, profile_name: String) -> Result<(), String> {
    connect_smcp_core(&state, profile_name).await
}

pub async fn connect_smcp_core(state: &AppState, profile_name: String) -> Result<(), String> {
    log::info!("Connecting with profile: {}", profile_name);

    let profiles = state.config.load_profiles().map_err(|e| e.to_string())?;
    let profile = profiles
        .iter()
        .find(|p| p.name == profile_name)
        .ok_or_else(|| format!("Profile not found: {}", profile_name))?
        .clone();

    let keychain_id = format!("profile:{}", profile.name);
    let api_key =
        crate::services::keychain::get_credential(&keychain_id).map_err(|e| e.to_string())?;

    let settings = state.settings_service.load();
    let configs = state.config.load_configs().map_err(|e| e.to_string())?;
    state
        .runtime
        .connect(&profile, &api_key, &settings, configs)
        .await?;

    log::info!("Connected to SMCP server: {}", profile.url);
    let _ = state.log_service.write(
        "info",
        "connection",
        &format!("Connected to {}", profile.url),
        None,
    );
    Ok(())
}

/// Disconnect from SMCP and rebuild a clean local Computer runtime from saved MCP configs.
#[tauri::command]
pub async fn disconnect_smcp(state: State<'_, AppState>) -> Result<(), String> {
    disconnect_smcp_core(&state).await
}

async fn disconnect_smcp_core(state: &AppState) -> Result<(), String> {
    log::info!("Disconnecting from SMCP server");

    state.runtime.disconnect().await;

    let _ = state
        .log_service
        .write("info", "connection", "Disconnected from SMCP server", None);
    Ok(())
}

/// Get current connection status
#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, AppState>,
) -> Result<ConnectionStatusInfo, String> {
    Ok(state.runtime.connection_status().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_defaults_are_explicit_in_fixture() {
        let profile = ConnectionProfile {
            name: "test-profile".to_string(),
            url: "http://127.0.0.1:9".to_string(),
            namespace: default_namespace(),
            office_id: "test-office".to_string(),
            computer_name: "test-computer".to_string(),
            api_key_ref: None,
            headers: std::collections::HashMap::new(),
            auto_connect: default_true(),
            auto_reconnect: default_true(),
        };

        assert_eq!(profile.namespace, "/smcp");
        assert!(profile.auto_connect);
        assert!(profile.auto_reconnect);
    }

    #[test]
    fn normalize_profile_forces_default_computer_name() {
        let profile = ConnectionProfile {
            name: "test-profile".to_string(),
            url: "http://127.0.0.1:9".to_string(),
            namespace: default_namespace(),
            office_id: "test-office".to_string(),
            computer_name: "custom-computer".to_string(),
            api_key_ref: None,
            headers: std::collections::HashMap::new(),
            auto_connect: default_true(),
            auto_reconnect: default_true(),
        };

        assert_eq!(
            normalize_profile(profile).computer_name,
            crate::services::settings::default_computer_name()
        );
    }
}
