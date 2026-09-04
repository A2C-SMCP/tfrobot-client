use crate::commands::activity_support::{record_computer_activity, ComputerActivitySpec};
use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::built_in_tools::{
    command_line_server_config, CommandLineRuntimeAssets, CommandLineToolPolicy,
    COMMAND_LINE_BUNDLE_ID,
};
use crate::services::observability::{
    ActivityManagedBy, ActivityProvider, ActivityTrigger, ComputerActivityCategory,
};
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::model::{
    MCPServerActivationState, MCPServerConnectionState,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandLineToolRuntimeState {
    Disabled,
    Pending,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandLineToolState {
    pub policy: CommandLineToolPolicy,
    pub effective_workspace: PathBuf,
    pub runtime_state: CommandLineToolRuntimeState,
    pub assets_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateCommandLineToolPolicyRequest {
    pub computer_id: String,
    pub policy: CommandLineToolPolicy,
}

#[tauri::command]
pub async fn get_command_line_tool_state(
    state: State<'_, AppState>,
    computer_id: String,
) -> Result<CommandLineToolState, String> {
    get_command_line_tool_state_core(&state, &computer_id).await
}

pub async fn get_command_line_tool_state_core(
    state: &AppState,
    computer_id: &str,
) -> Result<CommandLineToolState, String> {
    let instance = state
        .config
        .get_computer_instance(computer_id)
        .map_err(|error| error.to_string())?;
    let storage_root = state.config.computer_instance_storage_root(computer_id);
    let effective_workspace = instance.command_line.effective_workspace(&storage_root);
    let asset_error = CommandLineRuntimeAssets::discover().err();

    if !instance.command_line.enabled {
        return Ok(CommandLineToolState {
            policy: instance.command_line,
            effective_workspace,
            runtime_state: CommandLineToolRuntimeState::Disabled,
            assets_available: asset_error.is_none(),
            error: asset_error,
        });
    }
    if let Some(error) = asset_error {
        return Ok(CommandLineToolState {
            policy: instance.command_line,
            effective_workspace,
            runtime_state: CommandLineToolRuntimeState::Error,
            assets_available: false,
            error: Some(error),
        });
    }

    let runtime = state
        .computer_registry
        .runtime(computer_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {computer_id}"))?;
    let status = runtime
        .mcp_server_runtime_statuses()
        .await
        .into_iter()
        .find(|status| status.bundle_id.as_str() == COMMAND_LINE_BUNDLE_ID);
    let diagnostic = runtime
        .mcp_start_diagnostics()
        .await
        .into_iter()
        .find(|(bundle_id, _)| bundle_id.as_str() == COMMAND_LINE_BUNDLE_ID)
        .map(|(_, message)| message);
    let (runtime_state, error) = match status {
        Some(status)
            if status.activation == MCPServerActivationState::Started
                && status.connection == MCPServerConnectionState::Connected =>
        {
            (CommandLineToolRuntimeState::Running, None)
        }
        Some(status) if status.connection == MCPServerConnectionState::Error => (
            CommandLineToolRuntimeState::Error,
            diagnostic.or_else(|| Some("command line MCP connection failed".to_string())),
        ),
        Some(status) if status.activation == MCPServerActivationState::Started => {
            (CommandLineToolRuntimeState::Starting, diagnostic)
        }
        _ if runtime.is_running().await => (
            CommandLineToolRuntimeState::Error,
            diagnostic.or_else(|| Some("command line MCP is not mounted".to_string())),
        ),
        _ => (CommandLineToolRuntimeState::Pending, diagnostic),
    };

    Ok(CommandLineToolState {
        policy: instance.command_line,
        effective_workspace,
        runtime_state,
        assets_available: true,
        error,
    })
}

#[tauri::command]
pub async fn update_command_line_tool_policy(
    state: State<'_, AppState>,
    request: UpdateCommandLineToolPolicyRequest,
) -> Result<CommandLineToolState, String> {
    update_command_line_tool_policy_core(&state, request).await
}

pub async fn update_command_line_tool_policy_core(
    state: &AppState,
    request: UpdateCommandLineToolPolicyRequest,
) -> Result<CommandLineToolState, String> {
    let started = std::time::Instant::now();
    let computer_id = request.computer_id.clone();
    let enabled = request.policy.enabled;
    let mut policy_rolled_back = false;
    let result = async {
        let _operation_guard = state
            .computer_registry
            .operation_lease(&request.computer_id)
            .await;
        let previous = state
            .config
            .get_computer_instance(&request.computer_id)
            .map_err(|error| error.to_string())?;
        let previous_policy = previous.command_line.clone();
        let current_state = get_command_line_tool_state_core(state, &request.computer_id).await?;
        validate_workspace_update(
            &previous_policy,
            &request.policy,
            current_state.runtime_state,
        )?;
        request.policy.validate_existing_workspace()?;
        if request.policy.enabled {
            command_line_server_config(
                &request.policy,
                &state
                    .config
                    .computer_instance_storage_root(&request.computer_id),
            )?;
        }
        if previous_policy == request.policy {
            return Ok((current_state, false));
        }
        let updated = state
            .config
            .update_computer_instance(&request.computer_id, |instance| {
                instance.command_line = request.policy.clone();
            })
            .map_err(|error| error.to_string())?;
        if let Err(error) = apply_updated_computer_instance(state, previous, updated).await {
            policy_rolled_back = state
                .config
                .get_computer_instance(&request.computer_id)
                .is_ok_and(|instance| instance.command_line == previous_policy);
            return Err(error);
        }
        get_command_line_tool_state_core(state, &request.computer_id)
            .await
            .map(|state| (state, true))
    }
    .await;
    if result.as_ref().is_err() || result.as_ref().is_ok_and(|(_, changed)| *changed) {
        record_computer_activity(
            state,
            ComputerActivitySpec {
                computer_id: &computer_id,
                category: ComputerActivityCategory::Mcp,
                event_type: "built_in_mcp_policy",
                operation: "update",
                trigger: ActivityTrigger::User,
                managed_by: Some(ActivityManagedBy::BuiltIn),
                provider: Some(ActivityProvider::BuiltInMcp),
                message_subject: "Built-in command line MCP policy update",
                fields: serde_json::json!({
                    "bundle_id": COMMAND_LINE_BUNDLE_ID,
                    "enabled": enabled,
                    "policy_rolled_back": policy_rolled_back,
                }),
            },
            started,
            &result,
        )
        .await;
    }
    result.map(|(state, _)| state)
}

fn validate_workspace_update(
    previous: &CommandLineToolPolicy,
    requested: &CommandLineToolPolicy,
    runtime_state: CommandLineToolRuntimeState,
) -> Result<(), String> {
    let workspace_changed = previous.workspace_root != requested.workspace_root;
    if workspace_changed
        && matches!(
            runtime_state,
            CommandLineToolRuntimeState::Starting | CommandLineToolRuntimeState::Running
        )
    {
        return Err(
            "command line workspace cannot be changed while the tool is starting or running"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::observability::{
        ActivityQuery, ActivityScopeFilter, ObservabilityService,
    };
    use crate::services::settings::SettingsService;

    #[test]
    fn workspace_change_is_rejected_while_runtime_is_active() {
        let previous = CommandLineToolPolicy {
            enabled: true,
            workspace_root: Some(PathBuf::from("/old/workspace")),
        };
        let requested = CommandLineToolPolicy {
            workspace_root: Some(PathBuf::from("/new/workspace")),
            ..previous.clone()
        };

        for runtime_state in [
            CommandLineToolRuntimeState::Starting,
            CommandLineToolRuntimeState::Running,
        ] {
            let error = validate_workspace_update(&previous, &requested, runtime_state)
                .expect_err("active runtime must reject workspace changes");
            assert!(error.contains("cannot be changed"));
        }
    }

    #[test]
    fn workspace_change_is_allowed_while_runtime_is_inactive() {
        let previous = CommandLineToolPolicy::default();
        let requested = CommandLineToolPolicy {
            enabled: false,
            workspace_root: Some(PathBuf::from("/new/workspace")),
        };

        for runtime_state in [
            CommandLineToolRuntimeState::Disabled,
            CommandLineToolRuntimeState::Pending,
            CommandLineToolRuntimeState::Error,
        ] {
            validate_workspace_update(&previous, &requested, runtime_state)
                .expect("inactive runtime must allow workspace changes");
        }
    }

    #[test]
    fn unchanged_workspace_is_allowed_while_runtime_is_active() {
        let previous = CommandLineToolPolicy {
            enabled: true,
            workspace_root: Some(PathBuf::from("/workspace")),
        };
        let requested = CommandLineToolPolicy {
            enabled: false,
            ..previous.clone()
        };

        validate_workspace_update(&previous, &requested, CommandLineToolRuntimeState::Running)
            .expect("turning the tool off must remain allowed");
    }

    #[tokio::test]
    async fn unchanged_command_line_policy_does_not_emit_activity() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let state = AppState::new(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
        );

        update_command_line_tool_policy_core(
            &state,
            UpdateCommandLineToolPolicyRequest {
                computer_id: "computer-a".to_string(),
                policy: CommandLineToolPolicy::default(),
            },
        )
        .await
        .unwrap();

        let page = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "computer-a".to_string(),
                },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 0);
    }
}
