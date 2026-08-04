use crate::commands::runtime_error::RuntimeActionError;
use crate::services::client_control::CLIENT_CONTROL_BUNDLE_ID;
use crate::services::sdk_config::{is_writable_provenance, normalize_mcp_input_references};
use crate::AppState;
use a2c_smcp::smcp_computer::inputs::env_var_name;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::{ComputerConfigSnapshot, ProvenanceScope};
use a2c_smcp::smcp_computer::settings::SettingsValidationError;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use tauri::State;

/// Client-facing projection of SDK-owned configuration.
///
/// SDK input definitions are intentionally omitted: tfrobot-client owns per-Computer input
/// definitions, values, and secrets, so the SDK snapshot must not become their UI source of truth.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkConfigSnapshotView {
    pub version: u32,
    pub revision: String,
    pub mcp: SdkMcpConfigView,
    pub skills: SdkSkillConfigView,
    pub marketplace: SdkMarketplaceConfigView,
    pub plugins: SdkPluginConfigView,
    pub runtime: SdkRuntimeDefaultsView,
    pub provenance: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkMcpConfigView {
    pub servers: Vec<SdkMcpServerView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkMcpServerView {
    pub bundle_id: String,
    pub name: String,
    pub origin: String,
    pub writable: bool,
    pub trusted_origin: bool,
    pub bundled: bool,
    pub config: MCPServerConfig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkSkillConfigView {
    pub skill_home: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkMarketplaceConfigView {
    pub known: Vec<SdkMarketplaceView>,
    pub strict: Option<bool>,
    pub trusted: Vec<String>,
    pub blocked: Vec<String>,
    pub extra_known: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkMarketplaceView {
    pub name: String,
    pub source: Value,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkPluginConfigView {
    pub installed: Vec<SdkPluginRecordView>,
    pub enabled: Vec<SdkPluginEnablementView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkPluginRecordView {
    pub id: String,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkPluginEnablementView {
    pub id: String,
    pub enabled: bool,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkRuntimeDefaultsView {
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkConfigValidationView {
    pub valid: bool,
    pub errors: Vec<SettingsValidationError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkConfigStateView {
    pub snapshot: SdkConfigSnapshotView,
    pub validation: SdkConfigValidationView,
}

impl From<ComputerConfigSnapshot> for SdkConfigSnapshotView {
    fn from(snapshot: ComputerConfigSnapshot) -> Self {
        Self {
            version: snapshot.version,
            revision: snapshot.revision.0,
            mcp: SdkMcpConfigView {
                servers: snapshot
                    .mcp
                    .servers
                    .into_iter()
                    .filter(|server| {
                        resolve_bundle_id(&server.config).as_str() != CLIENT_CONTROL_BUNDLE_ID
                    })
                    .map(|server| {
                        let origin = server.origin;
                        let bundle_id = resolve_bundle_id(&server.config).into_string();
                        SdkMcpServerView {
                            bundle_id,
                            name: server.name,
                            origin: provenance_scope_name(origin).to_string(),
                            writable: is_writable_provenance(origin),
                            trusted_origin: server.trusted_origin,
                            bundled: server.bundled,
                            config: server.config,
                        }
                    })
                    .collect(),
            },
            skills: SdkSkillConfigView {
                skill_home: snapshot.skills.skill_home.to_string_lossy().into_owned(),
            },
            marketplace: SdkMarketplaceConfigView {
                known: snapshot
                    .marketplace
                    .known
                    .into_iter()
                    .map(|marketplace| SdkMarketplaceView {
                        name: marketplace.name,
                        source: marketplace.source,
                        origin: provenance_scope_name(marketplace.origin).to_string(),
                    })
                    .collect(),
                strict: snapshot.marketplace.strict,
                trusted: snapshot.marketplace.trusted,
                blocked: snapshot.marketplace.blocked,
                extra_known: snapshot.marketplace.extra_known,
            },
            plugins: SdkPluginConfigView {
                installed: snapshot
                    .plugins
                    .installed
                    .into_iter()
                    .map(|plugin| SdkPluginRecordView {
                        id: plugin.id,
                        origin: provenance_scope_name(plugin.origin).to_string(),
                    })
                    .collect(),
                enabled: snapshot
                    .plugins
                    .enabled
                    .into_iter()
                    .map(|plugin| SdkPluginEnablementView {
                        id: plugin.id,
                        enabled: plugin.enabled,
                        origin: provenance_scope_name(plugin.origin).to_string(),
                    })
                    .collect(),
            },
            runtime: SdkRuntimeDefaultsView {
                extra: snapshot.runtime.extra,
            },
            provenance: snapshot
                .provenance
                .into_iter()
                .map(|(entity, scope)| {
                    (entity.to_string(), provenance_scope_name(scope).to_string())
                })
                .collect(),
        }
    }
}

fn provenance_scope_name(scope: ProvenanceScope) -> &'static str {
    match scope {
        ProvenanceScope::Plugin => "plugin",
        ProvenanceScope::User => "user",
        ProvenanceScope::Project => "project",
        ProvenanceScope::Local => "local",
        ProvenanceScope::Embed => "embed",
        ProvenanceScope::Flag => "flag",
        ProvenanceScope::Policy => "policy",
        ProvenanceScope::Intent => "intent",
    }
}

/// Returns snapshot and schema validation from one serialized configuration read transaction.
#[tauri::command]
pub async fn get_computer_config_state(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<SdkConfigStateView, String> {
    get_computer_config_state_core(&state, &instance_id).await
}

pub async fn get_computer_config_state_core(
    state: &AppState,
    instance_id: &str,
) -> Result<SdkConfigStateView, String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance(state, instance_id)?;
    let (snapshot, report) = state
        .sdk_config
        .load_with_validation(instance_id)
        .map_err(|error| error.to_string())?;
    let snapshot = state
        .sdk_config
        .sanitize_snapshot_for_view(snapshot)
        .map_err(|error| error.to_string())?;
    Ok(SdkConfigStateView {
        snapshot: snapshot.into(),
        validation: validation_view(report),
    })
}

/// Creates or updates one SDK-owned MCP declaration, then applies it to the instance runtime
/// without rebuilding the Computer handle. Runtime activation failures remain observable on the
/// MCP runtime row and do not discard a valid durable declaration.
#[tauri::command]
pub async fn upsert_computer_mcp_config(
    state: State<'_, AppState>,
    instance_id: String,
    config: MCPServerConfig,
) -> Result<(), RuntimeActionError> {
    upsert_computer_mcp_config_core(&state, &instance_id, config).await
}

pub async fn upsert_computer_mcp_config_core(
    state: &AppState,
    instance_id: &str,
    config: MCPServerConfig,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance(state, instance_id).map_err(RuntimeActionError::runtime)?;
    let config = normalize_mcp_input_references(config).map_err(RuntimeActionError::runtime)?;
    if resolve_bundle_id(&config).as_str() == CLIENT_CONTROL_BUNDLE_ID {
        return Err(RuntimeActionError::runtime(
            "bundleId 'client_control' is reserved for the built-in Client Control provider",
        ));
    }
    let defined_inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let missing_input_id = referenced_input_ids(&config)?
        .into_iter()
        .find(|id| !defined_inputs.iter().any(|input| input.id() == id));
    let previous_bundle_id = state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.origin != ProvenanceScope::Plugin && server.name == config.name())
        .map(|server| resolve_bundle_id(&server.config));
    let next_bundle_id = resolve_bundle_id(&config);
    state
        .sdk_config
        .upsert_mcp_configs(instance_id, std::slice::from_ref(&config))
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    if let Some(runtime) = state.computer_registry.runtime(instance_id).await {
        if let Some(previous_bundle_id) = previous_bundle_id {
            if previous_bundle_id != next_bundle_id {
                if let Err(error) = runtime
                    .remove_user_mcp_server_config(&previous_bundle_id)
                    .await
                {
                    runtime
                        .record_mcp_config_apply_diagnostic(
                            previous_bundle_id.clone(),
                            format!(
                                "Configuration saved, but the previous MCP runtime identity could not be removed: {error}. Restart Runtime or inspect the logs before retrying."
                            ),
                        )
                        .await;
                    return Err(RuntimeActionError::runtime(format!(
                        "MCP config was saved, but the previous runtime identity could not be removed: {error}"
                    )));
                }
            }
        }
        if let Some(input_id) = missing_input_id {
            return Err(missing_input_definition_error(input_id));
        }
        runtime
            .apply_user_mcp_server_config(config)
            .await
            .map_err(RuntimeActionError::from)?;
    } else if let Some(input_id) = missing_input_id {
        return Err(missing_input_definition_error(input_id));
    }
    Ok(())
}

fn missing_input_definition_error(input_id: String) -> RuntimeActionError {
    RuntimeActionError::MissingInput {
        env_hint: env_var_name(&input_id),
        message: format!("Required input '{input_id}' is not defined for this Computer"),
        input_id,
    }
}

fn referenced_input_ids(config: &MCPServerConfig) -> Result<BTreeSet<String>, RuntimeActionError> {
    let value = serde_json::to_value(config)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let mut references = BTreeSet::new();
    if let Some(parameters) = value.get("server_parameters") {
        collect_input_references_in_value(parameters, &mut references);
    }
    if let Some(env_file) = value.get("envFile") {
        collect_input_references_in_value(env_file, &mut references);
    }
    Ok(references)
}

fn collect_input_references_in_value(value: &Value, references: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => collect_input_references_in_string(text, references),
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_input_references_in_value(value, references)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_input_references_in_value(value, references)),
        _ => {}
    }
}

fn collect_input_references_in_string(value: &str, references: &mut BTreeSet<String>) {
    let mut remaining = value;
    while let Some(start) = remaining.find("${input:") {
        let candidate = &remaining[start + "${input:".len()..];
        let Some(end) = candidate.find('}') else {
            return;
        };
        let id = &candidate[..end];
        if !id.is_empty() {
            references.insert(id.to_string());
        }
        remaining = &candidate[end + 1..];
    }
}

/// Removes one SDK-owned MCP declaration without stopping or reloading runtime state.
#[tauri::command]
pub async fn remove_computer_mcp_config(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    remove_computer_mcp_config_core(&state, &instance_id, &name).await
}

pub async fn remove_computer_mcp_config_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<(), String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance(state, instance_id)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("name is required".to_string());
    }
    let bundle_id = state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.origin != ProvenanceScope::Plugin && server.name == name)
        .map(|server| resolve_bundle_id(&server.config));
    state
        .sdk_config
        .remove_mcp_config(instance_id, name)
        .map_err(|error| error.to_string())?;
    if let Some(runtime) = state.computer_registry.runtime(instance_id).await {
        if let Some(bundle_id) = bundle_id {
            if let Err(error) = runtime.remove_user_mcp_server_config(&bundle_id).await {
                runtime
                    .record_mcp_config_apply_diagnostic(
                        bundle_id.clone(),
                        format!(
                            "Configuration removed, but the active MCP runtime could not be cleaned up: {error}. Restart Runtime or inspect the logs before retrying."
                        ),
                    )
                    .await;
                return Err(format!(
                    "MCP config was removed, but runtime cleanup failed: {error}"
                ));
            }
        }
    }
    Ok(())
}

fn validation_view(
    report: a2c_smcp::smcp_computer::settings::config::ValidationReport,
) -> SdkConfigValidationView {
    SdkConfigValidationView {
        valid: report.is_valid(),
        errors: report.errors,
    }
}

fn require_instance<'a>(state: &AppState, instance_id: &'a str) -> Result<&'a str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    Ok(instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2c_smcp::smcp_computer::settings::config::{
        load_config, ComputerConfigSnapshot, ConfigContext, ConfigRevision, EntityKey,
        InputDefsView, MarketplaceGovView, MarketplaceView, McpConfigView, McpServerView,
        PluginConfigView, PluginEnablementView, PluginRecordView, RuntimeDefaults, SkillConfigView,
    };
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn client_snapshot_excludes_sdk_input_definitions() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = load_config(&ConfigContext::new(directory.path()));
        let value = serde_json::to_value(SdkConfigSnapshotView::from(snapshot)).unwrap();
        let object = value.as_object().unwrap();

        assert!(!object.contains_key("inputs"));
        assert!(object.contains_key("revision"));
        assert!(object.contains_key("provenance"));
        assert!(object.contains_key("mcp"));
    }

    #[test]
    fn client_snapshot_owns_nested_camel_case_contract() {
        let config: MCPServerConfig = serde_json::from_value(json!({
            "type": "stdio",
            "name": "audit",
            "server_parameters": { "command": "node" }
        }))
        .unwrap();
        let snapshot = ComputerConfigSnapshot {
            version: 1,
            revision: ConfigRevision("sha256:stable".to_string()),
            mcp: McpConfigView {
                servers: vec![McpServerView {
                    name: "audit".to_string(),
                    origin: ProvenanceScope::Local,
                    trusted_origin: false,
                    bundled: true,
                    config,
                }],
            },
            inputs: InputDefsView::default(),
            skills: SkillConfigView {
                skill_home: PathBuf::from("/tmp/skills"),
            },
            marketplace: MarketplaceGovView {
                known: vec![MarketplaceView {
                    name: "acme".to_string(),
                    source: json!({ "type": "git", "url": "https://example.com/acme.git" }),
                    origin: ProvenanceScope::Intent,
                }],
                strict: Some(true),
                trusted: vec!["acme".to_string()],
                blocked: Vec::new(),
                extra_known: BTreeMap::from([("inline".to_string(), json!({"url": "x"}))]),
            },
            plugins: PluginConfigView {
                installed: vec![PluginRecordView {
                    id: "audit@acme".to_string(),
                    origin: ProvenanceScope::Intent,
                }],
                enabled: vec![PluginEnablementView {
                    id: "audit@acme".to_string(),
                    enabled: true,
                    origin: ProvenanceScope::Project,
                }],
            },
            runtime: RuntimeDefaults {
                extra: Map::from_iter([("timeout".to_string(), json!(30))]),
            },
            provenance: BTreeMap::from([
                (EntityKey::Mcp("audit".to_string()), ProvenanceScope::Local),
                (
                    EntityKey::PluginEnablement("audit@acme".to_string()),
                    ProvenanceScope::Project,
                ),
            ]),
            diagnostics: Vec::new(),
        };

        let value = serde_json::to_value(SdkConfigSnapshotView::from(snapshot)).unwrap();

        assert_eq!(value["revision"], "sha256:stable");
        assert_eq!(value["mcp"]["servers"][0]["trustedOrigin"], false);
        assert_eq!(value["mcp"]["servers"][0]["writable"], true);
        assert!(value["mcp"]["servers"][0].get("trusted_origin").is_none());
        assert_eq!(value["skills"]["skillHome"], "/tmp/skills");
        assert!(value["skills"].get("skill_home").is_none());
        assert_eq!(value["marketplace"]["extraKnown"]["inline"]["url"], "x");
        assert!(value["marketplace"].get("extra_known").is_none());
        assert_eq!(value["provenance"]["mcp:audit"], "local");
        assert_eq!(
            value["provenance"]["pluginEnablement:audit@acme"],
            "project"
        );
    }

    #[test]
    fn client_snapshot_marks_only_sdk_writable_origins_as_writable() {
        for origin in [
            ProvenanceScope::User,
            ProvenanceScope::Project,
            ProvenanceScope::Local,
        ] {
            assert!(is_writable_provenance(origin));
        }
        for origin in [
            ProvenanceScope::Plugin,
            ProvenanceScope::Embed,
            ProvenanceScope::Flag,
            ProvenanceScope::Policy,
            ProvenanceScope::Intent,
        ] {
            assert!(!is_writable_provenance(origin));
        }
    }

    #[test]
    fn normalizes_mustache_input_references_to_sdk_canonical_syntax() {
        let config: MCPServerConfig = serde_json::from_value(json!({
            "type": "stdio",
            "name": "openai-{{STAGE}}",
            "vrl": "{{VRL_TEMPLATE}}",
            "server_parameters": {
                "command": "node",
                "args": ["--token={{ OPENAI_KEY }}", "{{not {an id}}}"],
                "env": {
                    "OPENAI_API_KEY": "{{OPENAI_KEY}}",
                    "UNICODE": "{{地区 key}}",
                    "EXISTING": "${input:EXISTING}"
                }
            }
        }))
        .unwrap();

        let normalized = normalize_mcp_input_references(config).unwrap();
        let value = serde_json::to_value(normalized).unwrap();

        assert_eq!(
            value["server_parameters"]["env"]["OPENAI_API_KEY"],
            "${input:OPENAI_KEY}"
        );
        assert_eq!(
            value["server_parameters"]["args"][0],
            "--token=${input:OPENAI_KEY}"
        );
        assert_eq!(value["server_parameters"]["args"][1], "{{not {an id}}}");
        assert_eq!(
            value["server_parameters"]["env"]["UNICODE"],
            "${input:地区 key}"
        );
        assert_eq!(
            value["server_parameters"]["env"]["EXISTING"],
            "${input:EXISTING}"
        );
        assert_eq!(value["name"], "openai-{{STAGE}}");
        assert_eq!(value["vrl"], "{{VRL_TEMPLATE}}");
    }

    #[test]
    fn collects_unique_canonical_input_references_in_stable_order() {
        let config: MCPServerConfig = serde_json::from_value(json!({
            "type": "stdio",
            "name": "openai",
            "server_parameters": {
                "command": "${input:COMMAND}",
                "args": ["--token=${input:OPENAI_KEY}", "${input:OPENAI_KEY}"],
                "env": {
                    "IGNORED": "${input:not valid}",
                    "UNFINISHED": "${input:unfinished"
                }
            }
        }))
        .unwrap();

        assert_eq!(
            referenced_input_ids(&config)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![
                "COMMAND".to_string(),
                "OPENAI_KEY".to_string(),
                "not valid".to_string()
            ]
        );
    }
}
