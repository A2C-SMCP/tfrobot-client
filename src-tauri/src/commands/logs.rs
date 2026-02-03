use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFriendlyLog {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub status: CallStatus,
    pub summary: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallStatus {
    Pending,
    Success,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFilter {
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub tool_name: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
}

#[tauri::command]
pub async fn get_logs(filter: Option<LogFilter>) -> Result<Vec<UserFriendlyLog>, String> {
    tracing::debug!("Getting logs with filter: {:?}", filter);
    // TODO: Implement log retrieval from log storage
    Ok(vec![])
}

#[tauri::command]
pub async fn export_logs(path: String) -> Result<(), String> {
    tracing::info!("Exporting logs to: {}", path);
    // TODO: Implement log export functionality
    Ok(())
}
