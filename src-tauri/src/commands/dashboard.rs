use crate::services::logger::LogFilter;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub name: String,
    pub path: Option<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub connected: bool,
    pub connection_url: Option<String>,
    pub connection_profile: Option<String>,
    pub mcp_total: usize,
    pub mcp_running: usize,
    pub mcp_stopped: usize,
    pub tools_count: usize,
    pub recent_logs: Vec<crate::services::logger::LogEntry>,
    pub runtimes: Vec<RuntimeInfo>,
}

fn detect_runtime(name: &str, cmd: &str) -> RuntimeInfo {
    match which::which(cmd) {
        Ok(path) => RuntimeInfo {
            name: name.to_string(),
            path: Some(path.to_string_lossy().to_string()),
            available: true,
        },
        Err(_) => RuntimeInfo {
            name: name.to_string(),
            path: None,
            available: false,
        },
    }
}

#[tauri::command]
pub async fn get_dashboard_data(state: State<'_, AppState>) -> Result<DashboardData, String> {
    // Connection status
    let connection = state.runtime.connection_status().await;
    let connected = connection.connected;
    let connection_url = connection.url;
    let connection_profile = connection.profile_name;

    // MCP server stats
    let computer = state.runtime.computer();
    let statuses = computer.get_server_status().await;
    let mcp_total = statuses.len();
    let mcp_running = statuses.iter().filter(|(_, running, _)| *running).count();
    let mcp_stopped = mcp_total - mcp_running;
    let tools_count = computer
        .get_available_tools()
        .await
        .map(|tools| tools.len())
        .unwrap_or_default();

    // Recent logs
    let recent_logs = state
        .log_service
        .query(&LogFilter {
            limit: Some(10),
            ..Default::default()
        })
        .unwrap_or_default();

    // Runtime detection
    let runtimes = vec![
        detect_runtime("Node.js", "node"),
        detect_runtime("Python", "python3"),
        detect_runtime("uv", "uv"),
        detect_runtime("pnpm", "pnpm"),
    ];

    Ok(DashboardData {
        connected,
        connection_url,
        connection_profile,
        mcp_total,
        mcp_running,
        mcp_stopped,
        tools_count,
        recent_logs,
        runtimes,
    })
}
