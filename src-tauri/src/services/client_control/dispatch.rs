use super::*;
use crate::commands::{computer, connection, debug, desktop, inputs, marketplace, mcp, sdk_config};
use crate::services::computer::ComputerConnectionTarget;
use crate::services::connection_targets::ManualSmcpTarget;
use crate::services::observability::{
    with_activity_invocation_context, ActivityInvocationContext, ActivityManagedBy,
    ActivityProvider, ActivityTrigger,
};
use a2c_smcp::smcp_computer::mcp_clients::model::BundleId;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, ClientControlError> {
    serde_json::from_value(value).map_err(|error| {
        ClientControlError::new(
            ClientControlErrorCode::InvalidArguments,
            format!("invalid tool arguments: {error}"),
        )
    })
}

fn encode<T: serde::Serialize>(value: T) -> Result<serde_json::Value, ClientControlError> {
    serde_json::to_value(value).map_err(|error| {
        ClientControlError::new(
            ClientControlErrorCode::OperationFailed,
            format!("failed to encode tool result: {error}"),
        )
    })
}

fn operation_error(
    context: &InvocationContext,
    tool: ToolId,
    target: Option<&str>,
    error: impl ToString,
) -> ClientControlError {
    ClientControlError::invocation(
        ClientControlErrorCode::OperationFailed,
        error.to_string(),
        &context.source_computer_id,
        tool,
        target,
    )
}

struct AuditCompletion {
    outcome: AuditOutcome,
    error: Option<String>,
    started: Instant,
}

fn audit(
    plane: &ClientControlPlane,
    context: &InvocationContext,
    tool: ToolId,
    target: Option<&str>,
    parameters: serde_json::Value,
    completion: AuditCompletion,
) {
    let parameters = safe_audit_parameters(tool, parameters);
    let _ = plane.audit(ControlAuditRecord {
        request_id: context.request_id.clone(),
        source_computer_id: context.source_computer_id.clone(),
        target_computer_id: target.map(str::to_string),
        tool,
        parameters,
        outcome: completion.outcome,
        error: completion.error,
        duration_ms: completion.started.elapsed().as_millis(),
    });
}

fn safe_audit_parameters(tool: ToolId, mut parameters: serde_json::Value) -> serde_json::Value {
    let Some(object) = parameters.as_object_mut() else {
        return parameters;
    };
    match tool {
        ToolId::InputValueSet | ToolId::RuntimeInputValueSet => {
            if object.contains_key("value") {
                object.insert(
                    "value".to_string(),
                    serde_json::Value::String("[REDACTED]".to_string()),
                );
            }
        }
        ToolId::ConnectionTargetUpsert => {
            if let Some(target) = object.get_mut("target") {
                let summary = serde_json::json!({
                    "id": target.get("id"),
                    "name": target.get("name"),
                    "namespace": target.get("namespace"),
                    "office_id": target.get("office_id")
                });
                *target = summary;
            }
            if object.contains_key("api_key_action") {
                object.insert(
                    "api_key_action".to_string(),
                    serde_json::Value::String("[WRITE_ONLY]".to_string()),
                );
            }
        }
        ToolId::McpServerUpsert => {
            if let Some(server) = object.get_mut("server") {
                let transport = server
                    .get("type")
                    .or_else(|| server.get("server_type"))
                    .cloned();
                let summary = serde_json::json!({
                    "name": server.get("name"),
                    "bundle_id": server.get("bundle_id").or_else(|| server.get("bundleId")),
                    "transport": transport
                });
                *server = summary;
            }
        }
        ToolId::SkillCreate => {
            if object.contains_key("files") {
                object.insert(
                    "files".to_string(),
                    serde_json::Value::String("[CONTENT_OMITTED]".to_string()),
                );
            }
        }
        ToolId::SkillUpdate if object.contains_key("changes") => {
            object.insert(
                "changes".to_string(),
                serde_json::Value::String("[CONTENT_OMITTED]".to_string()),
            );
        }
        _ => {}
    }
    parameters
}

struct PendingAudit<'a> {
    plane: &'a ClientControlPlane,
    context: InvocationContext,
    tool: ToolId,
    target: Option<String>,
    parameters: serde_json::Value,
    started: Instant,
    active: bool,
}

impl PendingAudit<'_> {
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for PendingAudit<'_> {
    fn drop(&mut self) {
        if self.active {
            audit(
                self.plane,
                &self.context,
                self.tool,
                self.target.as_deref(),
                self.parameters.clone(),
                AuditCompletion {
                    outcome: AuditOutcome::Failed,
                    error: Some(
                        "Client Control request failed before execution completed".to_string(),
                    ),
                    started: self.started,
                },
            );
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComputerIdArgs {
    computer_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComputerNameArgs {
    computer_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComputerDuplicateArgs {
    computer_id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    copy_robot_binding: bool,
    #[serde(default)]
    connection_target_id: Option<String>,
    #[serde(default)]
    skill_home_mode: computer::DuplicateSkillHomeMode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionPolicyArgs {
    computer_id: String,
    #[serde(default)]
    target: Option<ComputerConnectionTarget>,
    #[serde(default)]
    auto_connect: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillHomeArgs {
    computer_id: String,
    #[serde(default)]
    local_skills_root: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetUpsertArgs {
    target: ManualSmcpTarget,
    #[serde(default)]
    api_key_action: Option<connection::ManualSmcpApiKeyAction>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetIdArgs {
    target_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectTargetArgs {
    computer_id: String,
    target_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectRobotArgs {
    computer_id: String,
    employee_id: u64,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpUpsertArgs {
    computer_id: String,
    server: MCPServerConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleArgs {
    computer_id: String,
    bundle_id: BundleId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputIdArgs {
    computer_id: String,
    input_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputDefinitionArgs {
    computer_id: String,
    definition: inputs::InputDefinition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputValueArgs {
    computer_id: String,
    input_id: String,
    value: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillCreateArgs {
    computer_id: String,
    name: String,
    files: Vec<SkillFileInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillUpdateArgs {
    computer_id: String,
    name: String,
    changes: Vec<SkillFileChange>,
    #[serde(default)]
    expected_revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillDeleteArgs {
    computer_id: String,
    name: String,
    #[serde(default)]
    expected_revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketplaceNameArgs {
    computer_id: String,
    marketplace: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketplaceWriteArgs {
    computer_id: String,
    name: String,
    git_url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginArgs {
    computer_id: String,
    marketplace: String,
    plugin: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceArgs {
    computer_id: String,
    bundle_id: BundleId,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopListArgs {
    computer_id: String,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopReadArgs {
    computer_id: String,
    bundle_id: BundleId,
    uri: String,
}

pub(super) async fn dispatch(
    plane: &Arc<ClientControlPlane>,
    context: InvocationContext,
    tool: ToolId,
    parameters: serde_json::Value,
) -> Result<serde_json::Value, ClientControlError> {
    let started = Instant::now();
    let target_computer_id = parameters
        .get("computer_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let activity_context = ActivityInvocationContext {
        correlation_id: context.request_id.clone(),
        trigger: ActivityTrigger::ClientControl,
        provider: ActivityProvider::BuiltInMcp,
        managed_by: ActivityManagedBy::BuiltIn,
        source_computer_id: context.source_computer_id.clone(),
        target_computer_id,
    };
    with_activity_invocation_context(
        activity_context,
        dispatch_with_activity_context(plane, context, tool, parameters, started),
    )
    .await
}

async fn dispatch_with_activity_context(
    plane: &Arc<ClientControlPlane>,
    context: InvocationContext,
    tool: ToolId,
    parameters: serde_json::Value,
    started: Instant,
) -> Result<serde_json::Value, ClientControlError> {
    let target = parameters
        .get("computer_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    if plane.catalog.get(tool).target == TargetContract::Required && target.is_none() {
        let error = ClientControlError::invocation(
            ClientControlErrorCode::InvalidArguments,
            "computer_id is required",
            &context.source_computer_id,
            tool,
            None,
        );
        audit(
            plane,
            &context,
            tool,
            None,
            parameters,
            AuditCompletion {
                outcome: AuditOutcome::Denied,
                error: Some(error.message.clone()),
                started,
            },
        );
        return Err(error);
    }
    if let Err(error) = plane
        .authorize(context.clone(), tool, target.as_deref())
        .await
    {
        audit(
            plane,
            &context,
            tool,
            target.as_deref(),
            parameters,
            AuditCompletion {
                outcome: AuditOutcome::Denied,
                error: Some(error.message.clone()),
                started,
            },
        );
        return Err(error);
    }
    let mut pending_audit = PendingAudit {
        plane,
        context: context.clone(),
        tool,
        target: target.clone(),
        parameters: parameters.clone(),
        started,
        active: true,
    };

    // Skill package operations keep their own success/failure audit because their summaries must
    // contain only paths, encodings, sizes, and revisions—never file content.
    match tool {
        ToolId::SkillCreate => {
            let args: SkillCreateArgs = decode(parameters)?;
            let result = plane
                .skill_create_authorized(context, &args.computer_id, args.name, args.files, started)
                .await;
            pending_audit.disarm();
            return encode(result?);
        }
        ToolId::SkillUpdate => {
            let args: SkillUpdateArgs = decode(parameters)?;
            let result = plane
                .skill_update_authorized(
                    context,
                    &args.computer_id,
                    args.name,
                    args.changes,
                    args.expected_revision,
                    started,
                )
                .await;
            pending_audit.disarm();
            return encode(result?);
        }
        ToolId::SkillDelete => {
            let args: SkillDeleteArgs = decode(parameters)?;
            let result = plane
                .skill_delete_authorized(
                    context,
                    &args.computer_id,
                    args.name,
                    args.expected_revision,
                    started,
                )
                .await;
            pending_audit.disarm();
            return encode(result?);
        }
        _ => {}
    }

    let state = plane.command_state()?;
    let result: Result<serde_json::Value, String> = match tool {
        ToolId::ComputerList => match computer::list_computer_instances_core(&state).await {
            Ok(mut statuses) => {
                let allowed = plane
                    .discover_targets(&context.source_computer_id, tool)
                    .await?
                    .into_iter()
                    .map(|instance| instance.id)
                    .collect::<HashSet<_>>();
                statuses.retain(|status| allowed.contains(&status.id));
                serde_json::to_value(statuses).map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        },
        ToolId::ComputerGetStatus => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            computer::get_computer_instance_status_core(&state, args.computer_id)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerCreate => {
            let request: computer::CreateComputerInstanceRequest = decode(parameters.clone())?;
            computer::create_computer_instance_with_trigger(&state, request, "client_control")
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerRename => {
            let args: ComputerNameArgs = decode(parameters.clone())?;
            computer::rename_computer_instance_with_trigger(
                &state,
                computer::RenameComputerInstanceRequest {
                    id: args.computer_id,
                    name: args.name,
                    description: args.description,
                    mcp_start_concurrency: None,
                },
                "client_control",
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerDuplicate => {
            let args: ComputerDuplicateArgs = decode(parameters.clone())?;
            computer::duplicate_computer_instance_with_trigger(
                &state,
                computer::DuplicateComputerInstanceRequest {
                    source_id: args.computer_id,
                    name: args.name,
                    description: args.description,
                    copy_robot_binding: args.copy_robot_binding,
                    connection_target_id: args.connection_target_id,
                    skill_home_mode: args.skill_home_mode,
                },
                "client_control",
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerDelete => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            computer::delete_computer_instance_with_trigger(
                &state,
                args.computer_id,
                "client_control",
            )
            .await
            .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::ComputerStart => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            computer::start_computer_instance_core(None, &state, args.computer_id)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerStop => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            computer::stop_computer_instance_core(&state, args.computer_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerRestart => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            computer::restart_computer_instance_core(None, &state, args.computer_id)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerSetConnectionPolicy => {
            let args: ConnectionPolicyArgs = decode(parameters.clone())?;
            computer::update_computer_connection_policy_core(
                &state,
                computer::UpdateComputerConnectionPolicyRequest {
                    id: args.computer_id,
                    target: args.target,
                    auto_connect: args.auto_connect,
                },
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ComputerSetSkillHome => {
            let args: SkillHomeArgs = decode(parameters.clone())?;
            computer::update_computer_skill_home_core(
                &state,
                computer::UpdateComputerSkillHomeRequest {
                    id: args.computer_id,
                    local_skills_root: args.local_skills_root,
                },
            )
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ConnectionTargetList => connection::list_manual_smcp_targets_core(&state)
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        ToolId::ConnectionTargetUpsert => {
            let args: TargetUpsertArgs = decode(parameters.clone())?;
            connection::save_manual_smcp_target_core(&state, args.target, args.api_key_action)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::ConnectionTargetDelete => {
            let args: TargetIdArgs = decode(parameters.clone())?;
            connection::delete_manual_smcp_target_core(&state, &args.target_id)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::RobotListAvailable => state
            .manager_context
            .list_digital_employees()
            .await
            .map_err(|error| error.to_string())
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        ToolId::ComputerConnectTarget => {
            let args: ConnectTargetArgs = decode(parameters.clone())?;
            connection::connect_connection_target_core(&state, &args.computer_id, &args.target_id)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::ComputerConnectRobot => {
            let args: ConnectRobotArgs = decode(parameters.clone())?;
            connection::manager_connect_smcp_core(
                &state,
                &args.computer_id,
                args.employee_id,
                args.scope,
            )
            .await
            .map_err(|error| error.to_string())
            .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::ComputerDisconnect => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            connection::disconnect_smcp_core(&state, &args.computer_id)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::ComputerGetConnectionStatus => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            connection::get_connection_status_core(&state, &args.computer_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::McpServerList => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            mcp::get_mcp_servers_core(&state, &args.computer_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::McpConfigGetState => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            sdk_config::get_computer_config_state_for_client_control_core(&state, &args.computer_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::McpServerUpsert => {
            let args: McpUpsertArgs = decode(parameters.clone())?;
            sdk_config::upsert_computer_mcp_config_core(&state, &args.computer_id, args.server)
                .await
                .map_err(|error| error.to_string())
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::McpServerRemove => {
            let args: BundleArgs = decode(parameters.clone())?;
            let name = match state.computer_registry.runtime(&args.computer_id).await {
                Some(runtime) => runtime.mcp_server_display_name(&args.bundle_id).await,
                None => None,
            };
            match name {
                Some(name) => {
                    sdk_config::remove_computer_mcp_config_core(&state, &args.computer_id, &name)
                        .await
                        .map(|_| serde_json::json!({"ok": true}))
                }
                None => Err(format!("MCP server not found: {}", args.bundle_id)),
            }
        }
        ToolId::McpServerStart => {
            let args: BundleArgs = decode(parameters.clone())?;
            mcp::start_mcp_server_core(&state, &args.computer_id, &args.bundle_id)
                .await
                .map_err(|error| error.to_string())
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::McpServerStop => {
            let args: BundleArgs = decode(parameters.clone())?;
            mcp::stop_mcp_server_core(&state, &args.computer_id, &args.bundle_id)
                .await
                .map_err(|error| error.to_string())
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::McpServerStartAll => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            mcp::start_all_servers_core(&state, &args.computer_id)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::McpServerStopAll => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            mcp::stop_all_servers_core(&state, &args.computer_id)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::InputDefinitionList => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            inputs::list_inputs_core(&state, &args.computer_id)
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::InputDefinitionGet => {
            let args: InputIdArgs = decode(parameters.clone())?;
            inputs::get_input_core(&state, &args.computer_id, &args.input_id)
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::InputDefinitionUpsert => {
            let args: InputDefinitionArgs = decode(parameters.clone())?;
            inputs::add_or_update_input_core(&state, &args.computer_id, args.definition)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::InputDefinitionRemove => {
            let args: InputIdArgs = decode(parameters.clone())?;
            inputs::remove_input_core(&state, &args.computer_id, &args.input_id)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::InputValueList => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            inputs::list_input_values_for_control_core(&state, &args.computer_id)
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::InputValueGetStatus => {
            let args: InputIdArgs = decode(parameters.clone())?;
            inputs::get_input_value_core(&state, &args.computer_id, &args.input_id)
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::InputValueSet => {
            let args: InputValueArgs = decode(parameters.clone())?;
            inputs::set_input_value_core(&state, &args.computer_id, args.input_id, args.value)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::RuntimeInputValueSet => {
            let args: InputValueArgs = decode(parameters.clone())?;
            inputs::set_runtime_input_value_core(
                &state,
                &args.computer_id,
                args.input_id,
                args.value,
            )
            .await
            .map(|updated| serde_json::json!({"updated": updated}))
        }
        ToolId::InputValueRemove => {
            let args: InputIdArgs = decode(parameters.clone())?;
            inputs::remove_input_value_core(&state, &args.computer_id, &args.input_id)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::InputValueClearAll => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            inputs::clear_input_values_core(&state, &args.computer_id)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::MarketplaceGetCapabilities => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            marketplace::get_marketplace_capabilities_core(&state, &args.computer_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::MarketplaceGetGovernance => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            marketplace::get_marketplace_governance_core(&state, &args.computer_id)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::MarketplaceAdd => {
            let args: MarketplaceWriteArgs = decode(parameters.clone())?;
            marketplace::add_marketplace_core(
                &state,
                &args.computer_id,
                marketplace::AddMarketplaceRequest {
                    name: args.name,
                    source: marketplace::MarketplaceSource::RemoteGit {
                        git_url: args.git_url,
                    },
                },
            )
            .await
            .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::MarketplaceUpdate => {
            let args: MarketplaceWriteArgs = decode(parameters.clone())?;
            marketplace::update_marketplace_core(
                &state,
                &args.computer_id,
                marketplace::UpdateMarketplaceRequest {
                    name: args.name,
                    source: marketplace::MarketplaceSource::RemoteGit {
                        git_url: args.git_url,
                    },
                },
            )
            .await
            .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::MarketplaceRefresh => {
            let args: MarketplaceNameArgs = decode(parameters.clone())?;
            marketplace::refresh_marketplace_core(&state, &args.computer_id, &args.marketplace)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::MarketplaceRemove => {
            let args: MarketplaceNameArgs = decode(parameters.clone())?;
            marketplace::remove_marketplace_core(&state, &args.computer_id, &args.marketplace)
                .await
                .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::PluginInstall
        | ToolId::PluginEnable
        | ToolId::PluginDisable
        | ToolId::PluginUninstall => {
            let args: PluginArgs = decode(parameters.clone())?;
            let request = marketplace::PluginLifecycleRequest {
                marketplace: args.marketplace,
                plugin: args.plugin,
            };
            match tool {
                ToolId::PluginInstall => {
                    marketplace::install_plugin_core(&state, &args.computer_id, request).await
                }
                ToolId::PluginEnable => {
                    marketplace::enable_plugin_core(&state, &args.computer_id, request)
                        .await
                        .map_err(|error| error.to_string())
                }
                ToolId::PluginDisable => {
                    marketplace::disable_plugin_core(&state, &args.computer_id, request).await
                }
                ToolId::PluginUninstall => {
                    marketplace::uninstall_plugin_core(&state, &args.computer_id, request).await
                }
                _ => unreachable!(),
            }
            .map(|_| serde_json::json!({"ok": true}))
        }
        ToolId::McpToolList => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            debug::get_available_tools_core(&state, &args.computer_id)
                .await
                .map(|mut tools| {
                    tools.retain(|item| {
                        item.bundle_id.as_ref().map(BundleId::as_str)
                            != Some(CLIENT_CONTROL_BUNDLE_ID)
                    });
                    tools
                })
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::McpToolHistory => {
            let args: ComputerIdArgs = decode(parameters.clone())?;
            debug::get_tool_history_core(&state, &args.computer_id)
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::McpResourceList => {
            let args: ResourceArgs = decode(parameters.clone())?;
            debug::get_debug_resources_core(&state, &args.computer_id, &args.bundle_id, args.cursor)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::DesktopList => {
            let args: DesktopListArgs = decode(parameters.clone())?;
            desktop::get_desktop_core(&state, &args.computer_id, args.uri.as_deref())
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::DesktopRead => {
            let args: DesktopReadArgs = decode(parameters.clone())?;
            desktop::get_window_detail_core(&state, &args.computer_id, &args.bundle_id, &args.uri)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        ToolId::SkillCreate | ToolId::SkillUpdate | ToolId::SkillDelete => unreachable!(),
    };

    match result {
        Ok(value) => {
            pending_audit.disarm();
            audit(
                plane,
                &context,
                tool,
                target.as_deref(),
                parameters,
                AuditCompletion {
                    outcome: AuditOutcome::Succeeded,
                    error: None,
                    started,
                },
            );
            Ok(value)
        }
        Err(message) => {
            pending_audit.disarm();
            let error = operation_error(&context, tool, target.as_deref(), message);
            audit(
                plane,
                &context,
                tool,
                target.as_deref(),
                parameters,
                AuditCompletion {
                    outcome: AuditOutcome::Failed,
                    error: Some(error.message.clone()),
                    started,
                },
            );
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_only_values_are_removed_before_the_audit_sink() {
        let input = safe_audit_parameters(
            ToolId::InputValueSet,
            serde_json::json!({
                "computer_id": "target",
                "input_id": "api-key",
                "value": "top-secret"
            }),
        );
        assert_eq!(input["value"], "[REDACTED]");
        assert!(!input.to_string().contains("top-secret"));

        let target = safe_audit_parameters(
            ToolId::ConnectionTargetUpsert,
            serde_json::json!({
                "target": {"id": "one"},
                "api_key_action": {"kind": "set", "value": "secret"}
            }),
        );
        assert_eq!(target["api_key_action"], "[WRITE_ONLY]");
        assert!(!target.to_string().contains("secret"));
        assert!(target["target"].get("url").is_none());
        assert!(target["target"].get("headers").is_none());

        let mcp = safe_audit_parameters(
            ToolId::McpServerUpsert,
            serde_json::json!({
                "computer_id": "target",
                "server": {
                    "type": "stdio",
                    "name": "private-server",
                    "bundle_id": "private-server",
                    "server_parameters": {
                        "command": "private-command",
                        "args": ["--api-key", "top-secret"],
                        "env": {"SAFE_LOOKING_NAME": "another-secret"}
                    }
                }
            }),
        );
        assert_eq!(mcp["server"]["name"], "private-server");
        assert_eq!(mcp["server"]["transport"], "stdio");
        assert!(!mcp.to_string().contains("top-secret"));
        assert!(!mcp.to_string().contains("another-secret"));
        assert!(!mcp.to_string().contains("private-command"));

        let skill_create = safe_audit_parameters(
            ToolId::SkillCreate,
            serde_json::json!({
                "computer_id": "target",
                "name": "demo-skill",
                "files": [{"path": "SKILL.md", "content": "private instructions"}]
            }),
        );
        assert_eq!(skill_create["files"], "[CONTENT_OMITTED]");
        assert!(!skill_create.to_string().contains("private instructions"));

        let skill_update = safe_audit_parameters(
            ToolId::SkillUpdate,
            serde_json::json!({
                "computer_id": "target",
                "name": "demo-skill",
                "changes": [{"op": "upsert", "content": "private replacement"}]
            }),
        );
        assert_eq!(skill_update["changes"], "[CONTENT_OMITTED]");
        assert!(!skill_update.to_string().contains("private replacement"));
    }
}
