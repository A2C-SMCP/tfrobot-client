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
    let runtimes = state.computer_registry.list_runtimes().await;
    let mut connected = false;
    let mut connection_url = None;
    let mut connection_profile = None;
    let mut mcp_total = 0;
    let mut mcp_running = 0;
    let mut tools_count = 0;

    for runtime in runtimes {
        let conn = runtime.connection.read().await;
        if !connected {
            connected = conn.is_some();
            connection_url = conn.as_ref().map(|c| c.url.clone());
            connection_profile = conn.as_ref().map(|c| c.profile_name.clone());
        }
        drop(conn);

        let lock = runtime.manager.read().await;
        if let Some(mgr) = lock.as_ref() {
            let statuses = mgr.get_server_status().await;
            mcp_total += statuses.len();
            mcp_running += statuses.iter().filter(|(_, running, _)| *running).count();
            tools_count += mgr.list_available_tools().await.len();
        }
    }
    let mcp_stopped = mcp_total.saturating_sub(mcp_running);

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
