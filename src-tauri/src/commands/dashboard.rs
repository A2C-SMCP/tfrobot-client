use crate::services::computer::{ComputerRuntimeState, ConnectionStateSummary};
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
    pub connected: bool,
    pub client_connection_present: bool,
    pub connection_revision: u64,
    pub connection_context: Option<ConnectionStateSummary>,
    pub mcp_server_count: usize,
    pub robot_name: Option<String>,
    pub connection_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerOverviewData {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub runtime: ComputerRuntimeSnapshot,
    pub connected: bool,
    pub client_connection_present: bool,
    pub connection_revision: u64,
    pub connection_context: Option<ConnectionStateSummary>,
    pub connection_url: Option<String>,
    pub connection_profile: Option<String>,
    pub robot_name: Option<String>,
    pub mcp_total: usize,
    pub mcp_running: usize,
    pub mcp_stopped: usize,
    pub tools_count: usize,
    pub recent_logs: Vec<crate::services::logger::LogEntry>,
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
        let connection_authority = runtime.connection_authority_snapshot().await;
        let connection_context = connection_authority.context;
        let connected = runtime_snapshot.lifecycle == ComputerRuntimeState::JoinedOffice
            && connection_context.is_some();
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
            connected,
            client_connection_present: connection_context.is_some(),
            connection_revision: connection_authority.revision,
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

#[tauri::command]
pub async fn get_computer_overview_data(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<ComputerOverviewData, String> {
    get_computer_overview_data_core(&state, &instance_id).await
}

pub async fn get_computer_overview_data_core(
    state: &AppState,
    instance_id: &str,
) -> Result<ComputerOverviewData, String> {
    if instance_id.trim().is_empty() {
        return Err("instance_id is required".to_string());
    }
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;

    let runtime_snapshot = runtime.runtime_snapshot().await;
    let running = runtime_snapshot.is_running();
    let connection_authority = runtime.connection_authority_snapshot().await;
    let connection_context = connection_authority.context;
    let connected = runtime_snapshot.lifecycle == ComputerRuntimeState::JoinedOffice
        && connection_context.is_some();
    let connection_url = connected
        .then(|| connection_context.as_ref().map(|c| c.url.clone()))
        .flatten();
    let connection_profile = connected
        .then(|| connection_context.as_ref().map(|c| c.profile_name.clone()))
        .flatten();

    let mcp_total = runtime_snapshot.mcp_servers;
    let mcp_running = runtime_snapshot.active_mcp_servers;
    let tools_count = runtime_snapshot.tools;
    let mcp_stopped = mcp_total.saturating_sub(mcp_running);

    let recent_logs = state
        .log_service
        .query(&LogFilter {
            computer_instance_id: Some(instance_id.to_string()),
            limit: Some(10),
            ..Default::default()
        })
        .unwrap_or_default();

    Ok(ComputerOverviewData {
        id: runtime.instance.id.clone(),
        name: runtime.instance.name.clone(),
        running,
        runtime: runtime_snapshot,
        connected,
        client_connection_present: connection_context.is_some(),
        connection_revision: connection_authority.revision,
        connection_context,
        connection_url,
        connection_profile,
        robot_name: runtime
            .instance
            .robot_binding
            .as_ref()
            .and_then(|binding| binding.robot_name.clone()),
        mcp_total,
        mcp_running,
        mcp_stopped,
        tools_count,
        recent_logs,
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

    #[tokio::test]
    async fn computer_overview_data_is_scoped_to_instance() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();
        state
            .log_service
            .write_for_instance("info", "mcp", "A log", None, Some("computer-a"))
            .unwrap();
        state
            .log_service
            .write_for_instance("info", "mcp", "B log", None, Some("computer-b"))
            .unwrap();

        let data = get_computer_overview_data_core(&state, "computer-a")
            .await
            .unwrap();

        assert_eq!(data.id, "computer-a");
        assert!(data.running);
        assert_eq!(data.recent_logs.len(), 1);
        assert_eq!(data.recent_logs[0].message, "A log");
    }

    #[tokio::test]
    async fn computer_overview_requires_existing_instance() {
        let (state, _dir) = test_state();

        let err = get_computer_overview_data_core(&state, "missing")
            .await
            .unwrap_err();

        assert!(err.contains("Computer instance not found"));
    }
}
