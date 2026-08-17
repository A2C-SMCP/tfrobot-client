use crate::commands::inputs::{prepare_portable_input_definitions, InputDefinition};
use crate::commands::runtime_error::RuntimeActionError;
use crate::services::client_control::CLIENT_CONTROL_BUNDLE_ID;
use crate::services::input_references;
use crate::services::oauth_credential_store::clear_oauth_credentials_for_config;
use crate::services::sdk_config::is_writable_provenance;
use crate::AppState;
use a2c_smcp::smcp_computer::inputs::env_var_name;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::{ComputerConfigSnapshot, ProvenanceScope};
use a2c_smcp::smcp_computer::settings::SettingsValidationError;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use tauri::State;

/// Client-facing projection of SDK-owned configuration.
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
    get_computer_config_state_for_projection(state, instance_id, ConfigStateProjection::LocalRaw)
        .await
}

/// Returns the API-safe configuration projection used by Client Control. Unlike the trusted
/// same-machine editor projection, this boundary must not expose plaintext configuration values.
pub async fn get_computer_config_state_for_client_control_core(
    state: &AppState,
    instance_id: &str,
) -> Result<SdkConfigStateView, String> {
    get_computer_config_state_for_projection(
        state,
        instance_id,
        ConfigStateProjection::ClientControlRedacted,
    )
    .await
}

#[derive(Debug, Clone, Copy)]
enum ConfigStateProjection {
    LocalRaw,
    ClientControlRedacted,
}

async fn get_computer_config_state_for_projection(
    state: &AppState,
    instance_id: &str,
    projection: ConfigStateProjection,
) -> Result<SdkConfigStateView, String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance(state, instance_id)?;
    let (mut snapshot, report) = state
        .sdk_config
        .load_with_validation(instance_id)
        .map_err(|error| error.to_string())?;
    if matches!(projection, ConfigStateProjection::ClientControlRedacted) {
        snapshot = state
            .sdk_config
            .sanitize_snapshot_for_client_control(snapshot)
            .map_err(|error| error.to_string())?;
    }
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
    if resolve_bundle_id(&config).as_str() == CLIENT_CONTROL_BUNDLE_ID {
        return Err(RuntimeActionError::runtime(
            "bundleId 'client_control' is reserved for the built-in Client Control provider",
        ));
    }
    let defined_inputs = state.sdk_config.load_input_definitions(instance_id);
    let referenced_input_ids = referenced_input_ids(&config)?;
    let missing_input_id = referenced_input_ids
        .into_iter()
        .find(|id| !defined_inputs.iter().any(|input| input.id() == id));
    let previous_config = state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.origin != ProvenanceScope::Plugin && server.name == config.name())
        .map(|server| server.config);
    let runtime = state.computer_registry.runtime(instance_id).await;
    let oauth_identity_change = previous_config
        .as_ref()
        .filter(|previous| oauth_credential_identity_changed(previous, &config));
    let _oauth_admission_guard = if oauth_identity_change.is_some() {
        match runtime.as_ref() {
            Some(runtime) => Some(runtime.block_oauth_admission_for_server_change().await),
            None => None,
        }
    } else {
        None
    };
    if let Some(previous) = oauth_identity_change {
        clear_oauth_before_config_change(state, runtime.as_ref(), instance_id, previous.clone())
            .await
            .map_err(RuntimeActionError::runtime)?;
    }
    state
        .sdk_config
        .upsert_mcp_configs(instance_id, std::slice::from_ref(&config))
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    if let Some(input_id) = missing_input_id {
        return Err(missing_input_definition_error(input_id));
    }
    Ok(())
}

/// Commits one Client editor draft as the SDK's top-level Input definitions plus canonical
/// server references. The SDK remains the persistence/parser boundary; this command only makes
/// the Client's two projections one atomic user operation.
#[tauri::command]
pub async fn upsert_computer_mcp_config_with_inputs(
    state: State<'_, AppState>,
    instance_id: String,
    config: MCPServerConfig,
    input_definitions: Vec<InputDefinition>,
    remove_input_ids_if_unused: Vec<String>,
) -> Result<(), RuntimeActionError> {
    upsert_computer_mcp_config_with_inputs_core(
        &state,
        &instance_id,
        config,
        input_definitions,
        remove_input_ids_if_unused,
    )
    .await
}

pub async fn upsert_computer_mcp_config_with_inputs_core(
    state: &AppState,
    instance_id: &str,
    config: MCPServerConfig,
    input_definitions: Vec<InputDefinition>,
    remove_input_ids_if_unused: Vec<String>,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _input_guard = state.input_mutation_lock.lock().await;
    let instance_id = require_instance(state, instance_id).map_err(RuntimeActionError::runtime)?;
    if resolve_bundle_id(&config).as_str() == CLIENT_CONTROL_BUNDLE_ID {
        return Err(RuntimeActionError::runtime(
            "bundleId 'client_control' is reserved for the built-in Client Control provider",
        ));
    }

    let input_definitions = prepare_portable_input_definitions(&input_definitions)
        .map_err(RuntimeActionError::runtime)?;
    let edited_input_ids = input_definitions
        .iter()
        .map(|definition| definition.id().to_string())
        .collect::<std::collections::HashSet<_>>();
    let mut project_inputs = state
        .sdk_config
        .load_project_input_definitions(instance_id)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    for definition in &input_definitions {
        project_inputs.retain(|item| item.id() != definition.id());
        project_inputs.push(definition.clone());
    }

    let mut available_inputs = state.sdk_config.load_input_definitions(instance_id);
    // Only the submitted definitions are candidates for this operation. Replacing the merged
    // view with every Project definition would incorrectly override Local/Policy precedence.
    for definition in &input_definitions {
        available_inputs.retain(|item| item.id() != definition.id());
        available_inputs.push(definition.clone());
    }
    if let Some(input_id) = referenced_input_ids(&config)?
        .into_iter()
        .find(|id| !available_inputs.iter().any(|input| input.id() == id))
    {
        return Err(missing_input_definition_error(input_id));
    }

    let previous_config = state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.origin != ProvenanceScope::Plugin && server.name == config.name())
        .map(|server| server.config);
    let runtime = state.computer_registry.runtime(instance_id).await;
    let oauth_identity_change = previous_config
        .as_ref()
        .filter(|previous| oauth_credential_identity_changed(previous, &config));
    let _oauth_admission_guard = if oauth_identity_change.is_some() {
        match runtime.as_ref() {
            Some(runtime) => Some(runtime.block_oauth_admission_for_server_change().await),
            None => None,
        }
    } else {
        None
    };
    if let Some(previous) = oauth_identity_change {
        clear_oauth_before_config_change(state, runtime.as_ref(), instance_id, previous.clone())
            .await
            .map_err(RuntimeActionError::runtime)?;
    }

    state
        .sdk_config
        .upsert_mcp_config_with_inputs_atomically(
            instance_id,
            &config,
            &project_inputs,
            &edited_input_ids,
            &remove_input_ids_if_unused.into_iter().collect(),
        )
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    Ok(())
}

fn oauth_credential_identity_changed(previous: &MCPServerConfig, next: &MCPServerConfig) -> bool {
    let Some(previous) =
        crate::services::oauth_credential_store::oauth_cleanup_config(previous.clone())
    else {
        return false;
    };
    let Some(next) = crate::services::oauth_credential_store::oauth_cleanup_config(next.clone())
    else {
        return true;
    };
    let MCPServerConfig::Http(previous) = previous else {
        unreachable!("OAuth cleanup config is always HTTP");
    };
    let MCPServerConfig::Http(next) = next else {
        unreachable!("OAuth cleanup config is always HTTP");
    };
    resolve_bundle_id(&MCPServerConfig::Http(previous.clone()))
        != resolve_bundle_id(&MCPServerConfig::Http(next.clone()))
        || previous.server_parameters.url != next.server_parameters.url
}

async fn clear_oauth_before_config_change(
    state: &AppState,
    runtime: Option<&crate::services::computer::ComputerInstanceRuntime>,
    instance_id: &str,
    config: MCPServerConfig,
) -> Result<(), String> {
    if let Some(runtime) = runtime {
        runtime.clear_oauth_for_server_config(config).await
    } else {
        clear_oauth_credentials_for_config(instance_id, state.secret_store.clone(), config).await
    }
}

fn missing_input_definition_error(input_id: String) -> RuntimeActionError {
    RuntimeActionError::MissingInput {
        env_hint: env_var_name(&input_id),
        message: format!("Required input '{input_id}' is not defined for this Computer"),
        input_id,
    }
}

fn referenced_input_ids(
    config: &MCPServerConfig,
) -> Result<std::collections::BTreeSet<String>, RuntimeActionError> {
    let value = serde_json::to_value(config)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let mut references = std::collections::BTreeSet::new();
    for field in [value.get("server_parameters"), value.get("envFile")]
        .into_iter()
        .flatten()
    {
        references.extend(input_references::referenced_input_ids(field));
    }
    Ok(references)
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
    let _input_guard = state.input_mutation_lock.lock().await;
    let instance_id = require_instance(state, instance_id)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("name is required".to_string());
    }
    let previous_config = state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.origin != ProvenanceScope::Plugin && server.name == name)
        .map(|server| server.config);
    let runtime = state.computer_registry.runtime(instance_id).await;
    let _oauth_admission_guard = match (previous_config.as_ref(), runtime.as_ref()) {
        (Some(_), Some(runtime)) => Some(runtime.block_oauth_admission_for_server_change().await),
        _ => None,
    };
    if let Some(config) = previous_config.clone() {
        clear_oauth_before_config_change(state, runtime.as_ref(), instance_id, config).await?;
    }
    let input_candidates = previous_config
        .as_ref()
        .map(referenced_input_ids)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default()
        .into_iter()
        .collect();
    state
        .sdk_config
        .remove_mcp_config_with_input_gc_atomically(instance_id, name, &input_candidates)
        .map_err(|error| error.to_string())?;
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

    fn oauth_http_config(endpoint: Option<&str>, disabled: bool) -> MCPServerConfig {
        serde_json::from_value(json!({
            "type": "streamable",
            "name": "protected",
            "bundle_id": "protected",
            "disabled": disabled,
            "server_parameters": {
                "url": endpoint.unwrap_or("https://mcp.example.com/mcp")
            }
        }))
        .unwrap()
    }

    #[test]
    fn oauth_identity_change_ignores_disable_but_detects_endpoint_or_static_auth() {
        let enabled = oauth_http_config(None, false);
        let disabled = oauth_http_config(None, true);
        let different_resource = oauth_http_config(Some("https://resource.example.com"), false);
        let mut different_implicit_resource = enabled.clone();
        if let MCPServerConfig::Http(http) = &mut different_implicit_resource {
            http.server_parameters.url = "https://new-mcp.example.com/mcp".to_string();
        }
        assert!(!oauth_credential_identity_changed(&enabled, &disabled));
        assert!(oauth_credential_identity_changed(
            &enabled,
            &different_resource
        ));
        assert!(oauth_credential_identity_changed(
            &enabled,
            &different_implicit_resource
        ));
        let legacy_auto: MCPServerConfig = serde_json::from_value(json!({
            "type": "streamable",
            "name": "legacy-auto",
            "bundle_id": "legacy-auto",
            "server_parameters": { "url": "https://legacy.example.com/mcp" }
        }))
        .unwrap();
        let mut moved_legacy_auto = legacy_auto.clone();
        if let MCPServerConfig::Http(http) = &mut moved_legacy_auto {
            http.server_parameters.url = "https://new-legacy.example.com/mcp".to_string();
        }
        assert!(oauth_credential_identity_changed(
            &legacy_auto,
            &moved_legacy_auto
        ));

        let mut static_authorization = legacy_auto.clone();
        if let MCPServerConfig::Http(http) = &mut static_authorization {
            http.server_parameters.headers.insert(
                "authorization".to_string(),
                "Bearer static-token".to_string(),
            );
        }
        assert!(oauth_credential_identity_changed(
            &legacy_auto,
            &static_authorization
        ));
    }

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
