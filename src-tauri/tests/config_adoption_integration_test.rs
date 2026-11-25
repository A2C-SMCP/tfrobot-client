//! Real filesystem + SDK + Node MCP acceptance for TFRC-139. No mocked runtime contract.
mod common;
use a2c_smcp::smcp_computer::mcp_clients::{model::BundleId, MCPServerConfig};
use common::{create_test_app_state, echo_server_config, env_server_path};
use tfrobot_client_lib::commands::{
    computer::{self, GeneralFieldPatch},
    computer_runtime::get_computer_config_adoption_core as adoption,
    debug, inputs, mcp, sdk_config,
};
use tfrobot_client_lib::services::computer::ComputerInstance;
use tfrobot_client_lib::AppState;

#[derive(Default)]
struct RollbackFailureSecrets {
    inner: tfrobot_client_lib::services::keychain::InMemorySecretStore,
    block_metadata: std::sync::Mutex<Option<std::path::PathBuf>>,
    fail_rollback: std::sync::atomic::AtomicBool,
}

impl RollbackFailureSecrets {
    fn after_mutation(&self) -> Result<(), tfrobot_client_lib::services::keychain::KeychainError> {
        if let Some(path) = self.block_metadata.lock().unwrap().take() {
            // Fail the real atomic metadata rename after the backing value changed.
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            self.fail_rollback
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
}

impl tfrobot_client_lib::services::keychain::SecretStore for RollbackFailureSecrets {
    fn set_secret(
        &self,
        key: &str,
        secret: &str,
    ) -> Result<(), tfrobot_client_lib::services::keychain::KeychainError> {
        use std::sync::atomic::Ordering;
        use tfrobot_client_lib::services::keychain::KeychainError;
        if self.fail_rollback.load(Ordering::SeqCst) {
            return Err(KeychainError::Store("injected rollback failure".into()));
        }
        self.inner.set_secret(key, secret)?;
        self.after_mutation()
    }

    fn get_secret(
        &self,
        key: &str,
    ) -> Result<Option<String>, tfrobot_client_lib::services::keychain::KeychainError> {
        self.inner.get_secret(key)
    }

    fn delete_secret(
        &self,
        key: &str,
    ) -> Result<(), tfrobot_client_lib::services::keychain::KeychainError> {
        self.inner.delete_secret(key)?;
        self.after_mutation()
    }
}

#[tokio::test]
async fn adoption_partial_input_commit_with_failed_rollback_is_unknown_and_recovers() {
    use std::sync::{atomic::Ordering, Arc};
    use tfrobot_client_lib::services::{
        config::ConfigService, keychain, observability::ObservabilityService,
        settings::SettingsService,
    };
    for delete in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(RollbackFailureSecrets::default());
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            secrets.clone(),
        );
        let instance = ComputerInstance::new("adoption-a", "Adoption A");
        state
            .config
            .add_computer_instance(instance.clone())
            .unwrap();
        state
            .computer_registry
            .upsert_runtime(instance)
            .await
            .unwrap();
        inputs::add_or_update_input_core(
            &state,
            "adoption-a",
            inputs::InputDefinition::PromptString {
                id: "region".into(),
                label: None,
                description: None,
                default: None,
                password: Some(true),
            },
        )
        .await
        .unwrap();
        inputs::set_input_value_core(
            &state,
            "adoption-a",
            "region".into(),
            "original-private-value".into(),
        )
        .await
        .unwrap();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type":"stdio", "name":"env-a", "bundle_id":"env-a",
            "server_parameters": {"command":"node", "args":[env_server_path()], "env":{"REGION":"${input:region}"}}
        })).unwrap();
        sdk_config::upsert_computer_mcp_config_core(&state, "adoption-a", config)
            .await
            .unwrap();
        computer::start_computer_instance_core(None, &state, "adoption-a".into())
            .await
            .unwrap();
        assert_eq!(
            adoption(&state, "adoption-a").await.unwrap().state,
            "current"
        );
        let metadata = state
            .config
            .computer_instance_storage_root("adoption-a")
            .join("input_entries.json");
        let original_metadata = std::fs::read(&metadata).unwrap();
        *secrets.block_metadata.lock().unwrap() = Some(metadata.clone());
        let error = if delete {
            inputs::delete_input_entry_core(&state, "adoption-a", "region")
                .await
                .unwrap_err()
        } else {
            inputs::set_input_value_core(
                &state,
                "adoption-a",
                "region".into(),
                "replacement-private-value".into(),
            )
            .await
            .unwrap_err()
        };
        assert!(error.contains("rollback also failed"), "{error}");
        assert_eq!(
            keychain::get_input_secret(&secrets.inner, "adoption-a", "region").unwrap(),
            (!delete).then(|| "replacement-private-value".to_string())
        );
        let observed = adoption(&state, "adoption-a").await.unwrap();
        assert_eq!(observed.state, "unknown", "{observed:?}");
        assert_eq!(observed.unknown_resources, ["env-a:region"]);
        assert!(!serde_json::to_string(&observed)
            .unwrap()
            .contains("private-value"));
        assert_eq!(read_region(&state, "env-a").await, "original-private-value");
        std::fs::remove_dir(&metadata).unwrap();
        std::fs::write(&metadata, original_metadata).unwrap();
        secrets.fail_rollback.store(false, Ordering::SeqCst);
        inputs::set_input_value_core(
            &state,
            "adoption-a",
            "region".into(),
            "replacement-private-value".into(),
        )
        .await
        .unwrap();
        assert_eq!(
            adoption(&state, "adoption-a").await.unwrap().state,
            "pending"
        );
        mcp::restart_mcp_server_with_latest_config_core(&state, "adoption-a", &id("env-a"))
            .await
            .unwrap();
        assert_eq!(
            adoption(&state, "adoption-a").await.unwrap().state,
            "current"
        );
        state.computer_registry.shutdown_all().await;
    }
}

#[tokio::test]
async fn adoption_http_requires_a_proven_restart_instead_of_inferring_an_idempotent_start() {
    use http_body_util::{BodyExt, Full};
    use hyper::{body::Bytes, service::service_fn};
    use std::{
        convert::Infallible,
        sync::{Arc, Mutex},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = seen.clone();
    let server = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let seen = captured.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                    let seen = seen.clone();
                    async move {
                        if request.method() != hyper::Method::POST {
                            return Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(405)
                                    .body(Full::<Bytes>::from(""))
                                    .unwrap(),
                            );
                        }
                        let intent = request
                            .headers()
                            .get("x-intent")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        let body = request.into_body().collect().await.unwrap().to_bytes();
                        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
                        let method = request["method"].as_str().unwrap_or_default();
                        if method.starts_with("notifications/") {
                            return Ok(hyper::Response::builder()
                                .status(202)
                                .body(Full::<Bytes>::from(""))
                                .unwrap());
                        }
                        let result = match method {
                            "initialize" => {
                                seen.lock().unwrap().push(intent);
                                serde_json::json!({"protocolVersion":"2024-11-05","serverInfo":{"name":"adoption-http","version":"1"},"capabilities":{"tools":{}}})
                            }
                            "tools/list" => serde_json::json!({"tools":[]}),
                            _ => serde_json::json!({}),
                        };
                        let payload =
                            serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":result});
                        Ok(hyper::Response::builder()
                            .header("content-type", "application/json")
                            .body(Full::<Bytes>::from(serde_json::to_vec(&payload).unwrap()))
                            .unwrap())
                    }
                });
                let stream = hyper_util::rt::TokioIo::new(stream);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .await;
            });
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path()).await;
    let mut config = serde_json::json!({"type":"Http","name":"http-proof","bundle_id":"http-proof","server_parameters":{"url":url,"headers":{"X-Intent":"original"}}});
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(config.clone()).unwrap(),
    )
    .await
    .unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    assert_eq!(seen.lock().unwrap().as_slice(), ["original"]);
    config["server_parameters"]["headers"]["X-Intent"] = "replacement".into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(config).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "pending"
    );
    mcp::start_mcp_server_core(&state, "adoption-a", &id("http-proof"))
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "unknown"
    );
    assert_eq!(seen.lock().unwrap().as_slice(), ["original"]);
    mcp::restart_mcp_server_with_latest_config_core(&state, "adoption-a", &id("http-proof"))
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    assert_eq!(seen.lock().unwrap().last().unwrap(), "replacement");
    inputs::add_or_update_input_core(
        &state,
        "adoption-a",
        inputs::InputDefinition::PromptString {
            id: "http-intent".into(),
            label: None,
            description: None,
            default: None,
            password: None,
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(&state, "adoption-a", "http-intent".into(), "fresh".into())
        .await
        .unwrap();
    let new_config = serde_json::json!({"type":"Http","name":"http-new","bundle_id":"http-new","server_parameters":{"url":url,"headers":{"X-Intent":"${input:http-intent}"}}});
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(new_config).unwrap(),
    )
    .await
    .unwrap();
    mcp::start_mcp_server_core(&state, "adoption-a", &id("http-new"))
        .await
        .unwrap();
    let unconfirmed = adoption(&state, "adoption-a").await.unwrap();
    assert_eq!(unconfirmed.state, "unknown", "{unconfirmed:?}");
    assert!(
        unconfirmed.changes.is_empty(),
        "missing receipts do not prove a definition change: {unconfirmed:?}"
    );
    assert!(unconfirmed
        .unknown_resources
        .contains(&"http-new".to_string()));
    mcp::restart_mcp_server_with_latest_config_core(&state, "adoption-a", &id("http-new"))
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    assert_eq!(seen.lock().unwrap().last().unwrap(), "fresh");
    state.computer_registry.shutdown_all().await;
    server.abort();
}

async fn state(path: &std::path::Path) -> AppState {
    let state = create_test_app_state(path);
    let instance = ComputerInstance::new("adoption-a", "Adoption A");
    state
        .config
        .add_computer_instance(instance.clone())
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(instance)
        .await
        .unwrap();
    state
}
fn id(value: &str) -> BundleId {
    BundleId::try_from(value).unwrap()
}
async fn patch(state: &AppState, value: GeneralFieldPatch) {
    computer::patch_computer_general_core(state, "adoption-a".into(), value)
        .await
        .unwrap();
}

#[tokio::test]
async fn adoption_profile_patch_is_field_scoped_and_startup_parameters_are_observed() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path()).await;
    patch(&state, GeneralFieldPatch::McpStartConcurrency(7)).await;
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "waiting_start"
    );
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    let generation = state
        .computer_registry
        .runtime("adoption-a")
        .await
        .unwrap()
        .runtime_generation();
    tokio::join!(
        patch(&state, GeneralFieldPatch::Name("Renamed".into())),
        patch(&state, GeneralFieldPatch::Description("Kept".into()))
    );
    let profile = state.config.get_computer_instance("adoption-a").unwrap();
    assert_eq!(profile.name, "Renamed");
    assert_eq!(profile.description.as_deref(), Some("Kept"));
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    patch(&state, GeneralFieldPatch::McpStartConcurrency(9)).await;
    let observed = adoption(&state, "adoption-a").await.unwrap();
    assert_eq!(observed.changes.len(), 1);
    assert_eq!(observed.changes[0].current_value.as_deref(), Some("7"));
    assert_eq!(observed.generation, generation);
    patch(&state, GeneralFieldPatch::McpStartConcurrency(7)).await;
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    assert!(computer::patch_computer_general_core(
        &state,
        "adoption-a".into(),
        GeneralFieldPatch::McpStartConcurrency(0)
    )
    .await
    .is_err());
    state
        .computer_registry
        .runtime("adoption-a")
        .await
        .unwrap()
        .try_shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn adoption_disabled_running_and_new_declarations_are_distinct_from_manual_stop() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path()).await;
    let mut config = serde_json::to_value(echo_server_config("adoption-echo")).unwrap();
    config["bundle_id"] = "adoption-echo".into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(config.clone()).unwrap(),
    )
    .await
    .unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    mcp::stop_mcp_server_core(&state, "adoption-a", &id("adoption-echo"))
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    mcp::start_mcp_server_core(&state, "adoption-a", &id("adoption-echo"))
        .await
        .unwrap();
    config["disabled"] = true.into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(config).unwrap(),
    )
    .await
    .unwrap();
    let observed = adoption(&state, "adoption-a").await.unwrap();
    assert_eq!(observed.changes[0].reason, "disabled_running");
    mcp::stop_mcp_server_core(&state, "adoption-a", &id("adoption-echo"))
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        echo_server_config("added-after-start"),
    )
    .await
    .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().changes.len(),
        1
    );
    state
        .computer_registry
        .runtime("adoption-a")
        .await
        .unwrap()
        .try_shutdown()
        .await
        .unwrap();
}

async fn read_region(state: &AppState, name: &str) -> String {
    let response = debug::execute_tool_core(
        state,
        "adoption-a",
        &format!("{name}__read_env"),
        serde_json::json!({}),
        Some(10.0),
    )
    .await
    .unwrap();
    assert!(response.success);
    let result = serde_json::to_value(response.result.unwrap()).unwrap();
    let env: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    env["REGION"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn adoption_real_input_values_and_secrets_follow_each_process_and_partial_restart() {
    for secret in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path()).await;
        inputs::add_or_update_input_core(
            &state,
            "adoption-a",
            inputs::InputDefinition::PromptString {
                id: "region".into(),
                label: None,
                description: None,
                default: None,
                password: Some(secret),
            },
        )
        .await
        .unwrap();
        inputs::set_input_value_core(
            &state,
            "adoption-a",
            "region".into(),
            "original-private-value".into(),
        )
        .await
        .unwrap();
        for name in ["env-a", "env-b"] {
            let config: MCPServerConfig = serde_json::from_value(serde_json::json!({ "type":"stdio", "name":name, "bundle_id":name, "server_parameters": { "command":"node", "args":[env_server_path()], "env": { "REGION":"${input:region}" } } })).unwrap();
            sdk_config::upsert_computer_mcp_config_core(&state, "adoption-a", config)
                .await
                .unwrap();
        }
        computer::start_computer_instance_core(None, &state, "adoption-a".into())
            .await
            .unwrap();
        assert_eq!(read_region(&state, "env-a").await, "original-private-value");
        inputs::set_input_value_core(
            &state,
            "adoption-a",
            "region".into(),
            "replacement-private-value".into(),
        )
        .await
        .unwrap();
        let observed = adoption(&state, "adoption-a").await.unwrap();
        assert_eq!(
            observed
                .changes
                .iter()
                .filter(|c| c.reason == "input_value")
                .count(),
            2,
            "{observed:?}"
        );
        let serialized = serde_json::to_string(&observed).unwrap();
        assert!(!serialized.contains("private-value"));
        assert_eq!(read_region(&state, "env-a").await, "original-private-value");
        // A healthy Start is a no-op: it must not acknowledge saved values as consumed.
        mcp::start_mcp_server_core(&state, "adoption-a", &id("env-a"))
            .await
            .unwrap();
        assert_eq!(
            adoption(&state, "adoption-a").await.unwrap().changes.len(),
            2
        );
        // Materialization fails before stopping the healthy old process. Preserve its receipt.
        inputs::delete_input_entry_core(&state, "adoption-a", "region")
            .await
            .unwrap();
        assert!(mcp::restart_mcp_server_with_latest_config_core(
            &state,
            "adoption-a",
            &id("env-a")
        )
        .await
        .is_err());
        assert_eq!(read_region(&state, "env-a").await, "original-private-value");
        assert_eq!(
            adoption(&state, "adoption-a").await.unwrap().changes.len(),
            2
        );
        inputs::set_input_value_core(
            &state,
            "adoption-a",
            "region".into(),
            "replacement-private-value".into(),
        )
        .await
        .unwrap();
        mcp::restart_mcp_server_with_latest_config_core(&state, "adoption-a", &id("env-a"))
            .await
            .unwrap();
        assert_eq!(
            read_region(&state, "env-a").await,
            "replacement-private-value"
        );
        assert_eq!(read_region(&state, "env-b").await, "original-private-value");
        let observed = adoption(&state, "adoption-a").await.unwrap();
        assert_eq!(observed.changes.len(), 1, "{observed:?}");
        assert_eq!(observed.changes[0].resource_id, "env-b");
        // Restoring the value is semantic equality, not a permanent dirty flag.
        inputs::set_input_value_core(
            &state,
            "adoption-a",
            "region".into(),
            "original-private-value".into(),
        )
        .await
        .unwrap();
        let observed = adoption(&state, "adoption-a").await.unwrap();
        assert_eq!(observed.changes.len(), 1);
        assert_eq!(observed.changes[0].resource_id, "env-a");
        state
            .computer_registry
            .runtime("adoption-a")
            .await
            .unwrap()
            .try_shutdown()
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn adoption_failed_restart_keeps_saved_declaration_and_exposes_runtime_failure() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path()).await;
    let mut config = serde_json::to_value(echo_server_config("failure-case")).unwrap();
    config["bundle_id"] = "failure-case".into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(config.clone()).unwrap(),
    )
    .await
    .unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    let runtime = state.computer_registry.runtime("adoption-a").await.unwrap();
    let generation = runtime.runtime_generation();
    config["server_parameters"]["command"] = "/definitely-missing-tfrc139-command".into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(config).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "pending"
    );
    assert!(mcp::restart_mcp_server_with_latest_config_core(
        &state,
        "adoption-a",
        &id("failure-case")
    )
    .await
    .is_err());
    assert_eq!(runtime.runtime_generation(), generation);
    assert!(runtime
        .runtime_snapshot()
        .await
        .problems
        .iter()
        .any(|p| p.operation == "start" || p.operation == "restart"));
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    let stored = state.sdk_config.load("adoption-a");
    assert!(serde_json::to_string(&stored)
        .unwrap()
        .contains("definitely-missing-tfrc139-command"));
    runtime.try_shutdown().await.unwrap();
}

#[tokio::test]
async fn adoption_skill_directory_and_multi_computer_boundaries_survive_partial_changes() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path()).await;
    let other = ComputerInstance::new("adoption-b", "B");
    state.config.add_computer_instance(other.clone()).unwrap();
    state.computer_registry.upsert_runtime(other).await.unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-b".into())
        .await
        .unwrap();
    let runtime = state.computer_registry.runtime("adoption-a").await.unwrap();
    let previous = runtime.sdk_skill_home().await;
    let desired = dir.path().join("different-skills");
    computer::update_computer_skill_home_core(
        &state,
        computer::UpdateComputerSkillHomeRequest {
            id: "adoption-a".into(),
            local_skills_root: Some(desired.display().to_string()),
        },
    )
    .await
    .unwrap();
    assert_eq!(runtime.sdk_skill_home().await, previous);
    let observed = adoption(&state, "adoption-a").await.unwrap();
    assert_eq!(observed.changes[0].reason, "skill_home");
    assert_eq!(
        adoption(&state, "adoption-b").await.unwrap().state,
        "current"
    );
    computer::restart_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    assert_eq!(
        state
            .computer_registry
            .runtime("adoption-a")
            .await
            .unwrap()
            .sdk_skill_home()
            .await,
        desired
    );
    state.computer_registry.shutdown_all().await;
}

#[tokio::test]
async fn adoption_plugin_input_consumers_are_observed_without_independent_mcp_actions() {
    use tfrobot_client_lib::commands::marketplace::{
        self, AddMarketplaceRequest, MarketplaceSource, PluginLifecycleRequest,
    };
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path()).await;
    let repo = dir.path().join("plugin-fixture");
    std::fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    std::fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"}]}"#,
    )
    .unwrap();
    let servers = repo.join("plugins/audit/mcp-servers");
    std::fs::create_dir_all(&servers).unwrap();
    std::fs::write(servers.join("plugin-env.json"), serde_json::to_vec(&serde_json::json!({
        "type":"stdio", "name":"plugin-env", "bundle_id":"plugin-env", "server_parameters": { "command":"node", "args":[env_server_path()], "env": { "REGION":"${input:audit@acme/region}" } }
    })).unwrap()).unwrap();
    std::fs::write(servers.join("inputs.json"), r#"{"inputs":[{"type":"PromptString","id":"region","description":"Region","password":true}]}"#).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(std::process::Command::new("git")
            .current_dir(&repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    inputs::upsert_input_entry_core(
        &state,
        "adoption-a",
        "audit@acme/region",
        Some("plugin-original-private-value".into()),
        true,
    )
    .await
    .unwrap();
    marketplace::add_marketplace_core(
        &state,
        "adoption-a",
        AddMarketplaceRequest {
            name: "acme".into(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".into(),
        plugin: "audit".into(),
    };
    marketplace::install_plugin_core(&state, "adoption-a", request.clone())
        .await
        .unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    marketplace::enable_plugin_core(&state, "adoption-a", request)
        .await
        .unwrap();
    assert_eq!(
        read_region(&state, "plugin-env").await,
        "plugin-original-private-value"
    );
    inputs::upsert_input_entry_core(
        &state,
        "adoption-a",
        "audit@acme/region",
        Some("plugin-replacement-private-value".into()),
        true,
    )
    .await
    .unwrap();
    let observed = adoption(&state, "adoption-a").await.unwrap();
    assert_ne!(
        observed.state, "current",
        "Plugin consumption must not be silently excluded"
    );
    assert!(observed.plugin_owners.contains_key("plugin-env"));
    assert!(observed
        .changes
        .iter()
        .all(|c| c.resource_id != "plugin-env"
            || serde_json::to_value(&c.action).unwrap() == "manage_plugin"));
    assert!(!serde_json::to_string(&observed)
        .unwrap()
        .contains("private-value"));
    assert_eq!(
        read_region(&state, "plugin-env").await,
        "plugin-original-private-value"
    );
    assert!(mcp::restart_mcp_server_with_latest_config_core(
        &state,
        "adoption-a",
        &id("plugin-env")
    )
    .await
    .is_err());
    state.computer_registry.shutdown_all().await;
}

#[tokio::test]
async fn adoption_plugin_global_fallback_tracks_consumed_definition_even_when_value_is_unchanged() {
    use std::sync::Arc;
    use tfrobot_client_lib::commands::marketplace::{
        self, AddMarketplaceRequest, MarketplaceSource, PluginLifecycleRequest,
    };
    use tfrobot_client_lib::services::runtime_input_bridge::{
        RuntimeInputCompletion, RuntimeInputRequest, RuntimeInputRequestSink,
    };
    struct Sink(tokio::sync::mpsc::UnboundedSender<RuntimeInputRequest>);
    impl RuntimeInputRequestSink for Sink {
        fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String> {
            self.0
                .send(request.clone())
                .map_err(|error| error.to_string())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(state(dir.path()).await);
    let original = inputs::InputDefinition::PromptString {
        id: "region".into(),
        label: None,
        description: Some("Original region".into()),
        default: None,
        password: None,
    };
    inputs::add_or_update_input_core(&state, "adoption-a", original.clone())
        .await
        .unwrap();
    let repo = dir.path().join("global-plugin-fixture");
    std::fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    std::fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"}]}"#,
    )
    .unwrap();
    let servers = repo.join("plugins/audit/mcp-servers");
    std::fs::create_dir_all(&servers).unwrap();
    std::fs::write(servers.join("global-env.json"), serde_json::to_vec(&serde_json::json!({
        "type":"stdio", "name":"global-env", "bundle_id":"global-env", "server_parameters":{"command":"node","args":[env_server_path()],"env":{"REGION":"${input:region}"}}
    })).unwrap()).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(std::process::Command::new("git")
            .current_dir(&repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    marketplace::add_marketplace_core(
        &state,
        "adoption-a",
        AddMarketplaceRequest {
            name: "acme".into(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let plugin = PluginLifecycleRequest {
        marketplace: "acme".into(),
        plugin: "audit".into(),
    };
    marketplace::install_plugin_core(&state, "adoption-a", plugin.clone())
        .await
        .unwrap();
    marketplace::enable_plugin_core(&state, "adoption-a", plugin)
        .await
        .unwrap();
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(Sink(sender)));
    bridge.set_ready("adoption-plugin-test", true);
    let starting = state.clone();
    let start = tokio::spawn(async move {
        computer::start_computer_instance_interactive_core(None, &starting, "adoption-a".into())
            .await
    });
    let request = tokio::time::timeout(std::time::Duration::from_secs(10), requests.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request.definition.id(), "region");
    let completion = bridge.complete(
        &request.request_id,
        RuntimeInputCompletion::Confirmed {
            value: "observed-private-region".into(),
        },
    );
    let (completion, start) = tokio::join!(completion, start);
    completion.unwrap();
    start.unwrap().unwrap();
    assert_eq!(
        read_region(&state, "global-env").await,
        "observed-private-region"
    );
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    inputs::add_or_update_input_core(
        &state,
        "adoption-a",
        inputs::InputDefinition::PromptString {
            id: "region".into(),
            label: None,
            description: Some("Updated region".into()),
            default: None,
            password: None,
        },
    )
    .await
    .unwrap();
    let observed = adoption(&state, "adoption-a").await.unwrap();
    assert_eq!(observed.state, "pending", "{observed:?}");
    assert!(observed
        .changes
        .iter()
        .any(|change| change.resource_id == "global-env"
            && change.reason == "input_definition"
            && serde_json::to_value(&change.action).unwrap() == "manage_plugin"));
    assert!(observed.plugin_owners.contains_key("global-env"));
    assert!(!serde_json::to_string(&observed)
        .unwrap()
        .contains("observed-private-region"));
    assert_eq!(
        read_region(&state, "global-env").await,
        "observed-private-region"
    );
    inputs::add_or_update_input_core(&state, "adoption-a", original)
        .await
        .unwrap();
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    assert!(mcp::restart_mcp_server_with_latest_config_core(
        &state,
        "adoption-a",
        &id("global-env")
    )
    .await
    .is_err());
    state.computer_registry.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adoption_handoff_notifies_existing_single_and_batch_entry_points_after_sdk_events() {
    use std::sync::Arc;
    use tfrobot_client_lib::services::computer_runtime_events::{
        ComputerRuntimeEventCause, ComputerRuntimeEventSink, ComputerRuntimeStatusEvent,
    };
    struct Sink(tokio::sync::mpsc::UnboundedSender<ComputerRuntimeStatusEvent>);
    impl ComputerRuntimeEventSink for Sink {
        fn emit(&self, event: &ComputerRuntimeStatusEvent) -> Result<(), String> {
            self.0.send(event.clone()).map_err(|e| e.to_string())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(state(dir.path()).await);
    let mut fast = serde_json::to_value(echo_server_config("event-fast")).unwrap();
    fast["bundle_id"] = "event-fast".into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(fast.clone()).unwrap(),
    )
    .await
    .unwrap();
    computer::start_computer_instance_core(None, &state, "adoption-a".into())
        .await
        .unwrap();
    mcp::stop_mcp_server_core(&state, "adoption-a", &id("event-fast"))
        .await
        .unwrap();
    fast["forbidden_tools"] = serde_json::json!(["not_a_tool"]);
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(fast.clone()).unwrap(),
    )
    .await
    .unwrap();
    let mut slow = serde_json::to_value(common::slow_echo_server_config("event-slow")).unwrap();
    slow["server_parameters"]["env"]["START_DELAY_MS"] = "1500".into();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(slow).unwrap(),
    )
    .await
    .unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    state
        .computer_registry
        .set_runtime_ui_event_sink(Arc::new(Sink(sender)))
        .await;
    let batch_state = state.clone();
    let batch =
        tokio::spawn(async move { mcp::start_all_servers_core(&batch_state, "adoption-a").await });
    // The fast SDK process is connected while the batch still awaits the slow process.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = receiver.recv().await {
            if event.snapshot.tools > 0 {
                break;
            }
        }
    })
    .await
    .unwrap();
    let early = adoption(&state, "adoption-a").await.unwrap();
    assert_eq!(
        early.state, "pending",
        "the UI can read before client receipt handoff"
    );
    while receiver.try_recv().is_ok() {}
    assert!(batch.await.unwrap().unwrap().failures.is_empty());
    let mut terminal_notifications = 0;
    while let Ok(event) = receiver.try_recv() {
        if matches!(event.cause, ComputerRuntimeEventCause::ConfigurationChanged) {
            terminal_notifications += 1;
        }
    }
    assert!(
        terminal_notifications >= 2,
        "both completed resource handoffs must trigger a new observation"
    );
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    mcp::stop_mcp_server_core(&state, "adoption-a", &id("event-fast"))
        .await
        .unwrap();
    fast["forbidden_tools"] = serde_json::json!([]);
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "adoption-a",
        serde_json::from_value(fast).unwrap(),
    )
    .await
    .unwrap();
    while receiver.try_recv().is_ok() {}
    mcp::start_mcp_server_core(&state, "adoption-a", &id("event-fast"))
        .await
        .unwrap();
    let mut single_notified = false;
    while let Ok(event) = receiver.try_recv() {
        single_notified |= matches!(event.cause, ComputerRuntimeEventCause::ConfigurationChanged);
    }
    assert!(single_notified);
    assert_eq!(
        adoption(&state, "adoption-a").await.unwrap().state,
        "current"
    );
    state.computer_registry.shutdown_all().await;
}
