use crate::services::computer::{
    ClientConnectionStateSnapshot, ClientConnectionStatus, ConnectionStateSummary,
};
use crate::services::computer_runtime_events::ComputerRuntimeSnapshot;
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
    pub computer_total: usize,
    pub computer_running: usize,
    pub computer_stopped: usize,
    pub computer_connected: usize,
    pub computers: Vec<DashboardComputerSummary>,
    pub recent_logs: Vec<crate::services::logger::LogEntry>,
    pub runtimes: Vec<RuntimeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardComputerSummary {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub runtime: ComputerRuntimeSnapshot,
    pub connection_state: ClientConnectionStateSnapshot,
    pub connected: bool,
    pub client_connection_present: bool,
    pub connection_revision: u64,
    pub connection_context: Option<ConnectionStateSummary>,
    pub mcp_server_count: usize,
    pub robot_name: Option<String>,
    pub connection_profile: Option<String>,
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
    get_dashboard_data_core(&state).await
}

pub async fn get_dashboard_data_core(state: &AppState) -> Result<DashboardData, String> {
    let runtimes = state.computer_registry.list_runtimes().await;
    let mut computer_running = 0;
    let mut computer_connected = 0;
    let mut computers = Vec::with_capacity(runtimes.len());

    for runtime in runtimes {
        let runtime_snapshot = runtime.runtime_snapshot().await;
        let running = runtime_snapshot.is_running();
        if running {
            computer_running += 1;
        }
        let connection_state = runtime.connection_snapshot().await;
        let connection_context = connection_state.context.clone();
        let connected = connection_state.status == ClientConnectionStatus::Connected;
        if connected {
            computer_connected += 1;
        }
        let connection_profile = connected
            .then(|| {
                connection_context
                    .as_ref()
                    .map(|connection| connection.profile_name.clone())
            })
            .flatten();

        computers.push(DashboardComputerSummary {
            id: runtime.instance.id.clone(),
            name: runtime.instance.name.clone(),
            running,
            runtime: runtime_snapshot.clone(),
            connection_state: connection_state.clone(),
            connected,
            client_connection_present: connection_context.is_some(),
            connection_revision: connection_state.revision,
            connection_context,
            mcp_server_count: runtime_snapshot.mcp_servers,
            robot_name: runtime
                .instance
                .robot_binding
                .as_ref()
                .and_then(|binding| binding.robot_name.clone()),
            connection_profile,
        });
    }

    let computer_total = computers.len();
    let computer_stopped = computer_total.saturating_sub(computer_running);

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
        computer_total,
        computer_running,
        computer_stopped,
        computer_connected,
        computers,
        recent_logs,
        runtimes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::logger::LogService;
    use crate::services::settings::SettingsService;
    use crate::AppState;
    use a2c_smcp::smcp_computer::settings::config::{ConfigEdit, ConfigEntity, EditIntent};
    use tempfile::TempDir;

    fn test_state() -> (AppState, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-b", "Computer B"))
            .unwrap();
        let log_service = LogService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());
        (AppState::new(config, log_service, settings_service), dir)
    }

    #[tokio::test]
    async fn dashboard_data_summarizes_computer_runtimes() {
        let (state, _dir) = test_state();
        state
            .sdk_config
            .update(
                "computer-a",
                &[ConfigEdit::new(
                    ConfigEntity::McpServer("snapshot-only".to_string()),
                    EditIntent::Upsert(serde_json::json!({
                        "type": "stdio",
                        "server_parameters": {"command": "node"}
                    })),
                )],
            )
            .unwrap();
        state
            .computer_registry
            .runtime("computer-a")
            .await
            .unwrap()
            .restart()
            .await
            .unwrap();
        state
            .log_service
            .write_for_instance("error", "mcp", "Server failed", None, Some("computer-a"))
            .unwrap();
        state
            .log_service
            .write_for_instance("info", "mcp", "Server ok", None, Some("computer-b"))
            .unwrap();

        let data = get_dashboard_data_core(&state).await.unwrap();

        assert_eq!(data.computer_total, 2);
        assert_eq!(data.computer_running, 1);
        assert_eq!(data.computer_stopped, 1);
        assert!(data
            .recent_logs
            .iter()
            .any(|log| log.message == "Server failed"));
        let computer_a = data
            .computers
            .iter()
            .find(|computer| computer.id == "computer-a")
            .unwrap();
        assert_eq!(computer_a.mcp_server_count, 1);
        assert!(data
            .runtimes
            .iter()
            .any(|runtime| runtime.name == "Node.js"));
    }
}
