use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmcpConnectionConfig {
    pub server_url: String,
    pub computer_name: String,
    pub office_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Connected {
        server_url: String,
        office_id: String,
    },
    Disconnected,
    Connecting,
    Error {
        message: String,
    },
}

#[tauri::command]
pub async fn connect_smcp(config: SmcpConnectionConfig) -> Result<(), String> {
    tracing::info!("Connecting to SMCP server: {}", config.server_url);
    // TODO: Integrate with smcp-computer SmcpComputerClient
    Ok(())
}

#[tauri::command]
pub async fn disconnect_smcp() -> Result<(), String> {
    tracing::info!("Disconnecting from SMCP server");
    // TODO: Integrate with smcp-computer SmcpComputerClient
    Ok(())
}

#[tauri::command]
pub async fn get_connection_status() -> Result<ConnectionStatus, String> {
    // TODO: Integrate with smcp-computer SmcpComputerClient
    Ok(ConnectionStatus::Disconnected)
}
