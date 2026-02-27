use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

/// Desktop window resource info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopWindow {
    pub uri: String,
    pub title: String,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Get desktop resources (window:// URIs)
/// Note: Desktop/resource listing APIs are not directly exposed on MCPServerManager.
/// This is a placeholder that will be implemented when the smcp-computer crate
/// exposes list_windows/get_window_detail on the manager level.
#[tauri::command]
pub async fn get_desktop(
    state: State<'_, AppState>,
    _size: Option<String>,
    _uri: Option<String>,
) -> Result<Vec<DesktopWindow>, String> {
    let _ = state;
    // TODO: Implement when smcp-computer exposes window listing on MCPServerManager
    tracing::info!("get_desktop called - awaiting smcp-computer API support");
    Ok(vec![])
}
