use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::time::{timeout, Duration};

const SMCP_CONNECTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

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
    state.config.load_profiles().map_err(|e| e.to_string())
}

/// Save (create or update) a connection profile
#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    api_key: Option<String>,
) -> Result<(), String> {
    let name = profile.name.clone();
    log::info!("Saving connection profile: {}", name);

    // Store API key in keychain if provided
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

/// Connect to SMCP server using a saved profile
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

    // Retrieve API key from keychain
    let keychain_id = format!("profile:{}", profile.name);
    let api_key =
        crate::services::keychain::get_credential(&keychain_id).map_err(|e| e.to_string())?;

    // Share the same manager Arc with SmcpComputerClient
    let manager = state.manager.clone();
    let inputs = state.inputs.clone();

    let client = smcp_computer::socketio_client::SmcpComputerClient::new(
        &profile.url,
        manager,
        profile.computer_name.clone(),
        api_key,
        inputs,
        Some(profile.headers.clone()),
    )
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = client.join_office(&profile.office_id).await {
        disconnect_smcp_client(client, "after join_office failure").await;
        return Err(e.to_string());
    }

    let new_connection = ConnectionState {
        client,
        profile_name: profile.name.clone(),
        url: profile.url.clone(),
        office_id: profile.office_id.clone(),
        computer_name: profile.computer_name.clone(),
        connected_at: chrono::Utc::now(),
    };

    let previous_connection = {
        let mut conn = state.connection.write().await;
        conn.replace(new_connection)
    };
    if let Some(connection) = previous_connection {
        close_smcp_connection(connection).await;
    }

    log::info!("Connected to SMCP server: {}", profile.url);
    let _ = state.log_service.write(
        "info",
        "connection",
        &format!("Connected to {}", profile.url),
        None,
    );
    Ok(())
}

/// Disconnect from SMCP server
#[tauri::command]
pub async fn disconnect_smcp(state: State<'_, AppState>) -> Result<(), String> {
    disconnect_smcp_core(&state).await
}

pub async fn disconnect_smcp_core(state: &AppState) -> Result<(), String> {
    log::info!("Disconnecting from SMCP server");

    let existing_connection = {
        let mut conn = state.connection.write().await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(connection).await;
    }

    let _ = state
        .log_service
        .write("info", "connection", "Disconnected from SMCP server", None);
    Ok(())
}

pub async fn close_smcp_connection(connection: ConnectionState) {
    let ConnectionState {
        client, office_id, ..
    } = connection;

    match timeout(
        SMCP_CONNECTION_CLOSE_TIMEOUT,
        client.leave_office(&office_id),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("Error leaving office: {}", e),
        Err(_) => log::warn!(
            "Timed out leaving SMCP office after {:?}",
            SMCP_CONNECTION_CLOSE_TIMEOUT
        ),
    }

    disconnect_smcp_client(client, "from SMCP server").await;
}

async fn disconnect_smcp_client(
    client: smcp_computer::socketio_client::SmcpComputerClient,
    context: &str,
) {
    match timeout(SMCP_CONNECTION_CLOSE_TIMEOUT, client.disconnect()).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("Error disconnecting {}: {}", context, e),
        Err(_) => log::warn!(
            "Timed out disconnecting {} after {:?}",
            context,
            SMCP_CONNECTION_CLOSE_TIMEOUT,
        ),
    }
}

/// Get current connection status
#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, AppState>,
) -> Result<ConnectionStatusInfo, String> {
    let conn = state.connection.read().await;
    match conn.as_ref() {
        Some(c) => Ok(ConnectionStatusInfo {
            connected: true,
            url: Some(c.url.clone()),
            office_id: Some(c.office_id.clone()),
            computer_name: Some(c.computer_name.clone()),
            connected_at: Some(c.connected_at.to_rfc3339()),
            profile_name: Some(c.profile_name.clone()),
        }),
        None => Ok(ConnectionStatusInfo {
            connected: false,
            url: None,
            office_id: None,
            computer_name: None,
            connected_at: None,
            profile_name: None,
        }),
    }
}

/// Active connection state held in AppState
pub struct ConnectionState {
    pub client: smcp_computer::socketio_client::SmcpComputerClient,
    pub profile_name: String,
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
}
