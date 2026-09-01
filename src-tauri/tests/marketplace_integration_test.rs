//! Integration tests for the Skills Marketplace governance boundary.
//!
//! tfrobot-client drives marketplace/plugin lifecycle through SDK Computer-level
//! APIs and does not create its own governance ledger.

mod common;

use a2c_smcp::smcp_computer::mcp_clients::model::BundleId;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::oauth::{
    OAuthCredentialKey, OAuthCredentialRecordKind, OAuthCredentialStore,
};
use common::{create_test_app_state, echo_server_config, echo_server_path, mcp};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tfrobot_client_lib::commands::{
    computer::{
        duplicate_computer_instance_core, get_computer_instance_status_core,
        restart_computer_instance_core, restart_computer_instance_interactive_core,
        start_computer_instance_core, start_computer_instance_interactive_core,
        stop_computer_instance_core, DuplicateComputerInstanceRequest, DuplicateSkillHomeMode,
    },
    config_io,
    dashboard::get_dashboard_data_core,
    inputs,
    marketplace::{
        add_marketplace_core, disable_plugin_core, enable_plugin_core,
        enable_plugin_interactive_core, get_marketplace_capabilities_core,
        get_marketplace_governance_core, install_plugin_core, refresh_marketplace_core,
        remove_marketplace_core, uninstall_plugin_core, update_marketplace_core,
        AddMarketplaceRequest, MarketplaceSource, MarketplaceSourceSummary, PluginLifecycleRequest,
        UpdateMarketplaceRequest,
    },
    runtime_error::RuntimeActionError,
    sdk_config, skills,
};
use tfrobot_client_lib::services::computer::{ComputerInstance, McpServerManagedBy};
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::keychain::{KeychainError, SecretStore};
use tfrobot_client_lib::services::oauth_credential_store::KeychainOAuthCredentialStore;
use tfrobot_client_lib::services::observability::ObservabilityService;
use tfrobot_client_lib::services::runtime_input_bridge::{
    RuntimeInputCompletion, RuntimeInputRequest, RuntimeInputRequestSink,
};
use tfrobot_client_lib::services::settings::SettingsService;
use tfrobot_client_lib::AppState;
use tokio::time::{timeout, Duration};

const TEST_INSTANCE_ID: &str = "computer-a";
const TEST_SECOND_INSTANCE_ID: &str = "computer-b";

struct RecordingRuntimeInputSink {
    sender: tokio::sync::mpsc::UnboundedSender<RuntimeInputRequest>,
}

impl RuntimeInputRequestSink for RecordingRuntimeInputSink {
    fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String> {
        self.sender
            .send(request.clone())
            .map_err(|error| error.to_string())
    }
}

fn write_mcp_server_config(path: &Path, name: &str) {
    fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "type": "stdio",
            "name": name,
            "server_parameters": {
                "command": "node",
                "args": [echo_server_path()],
                "env": {}
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

fn echo_server_config_with_disabled(name: &str, disabled: bool) -> MCPServerConfig {
    let mut value = serde_json::to_value(echo_server_config(name)).unwrap();
    value["disabled"] = serde_json::Value::Bool(disabled);
    serde_json::from_value(value).unwrap()
}

async fn create_marketplace_test_app_state(path: &std::path::Path) -> AppState {
    let state = create_test_app_state(path);
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(TEST_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    state
}

#[derive(Default)]
struct RecordingSecretStore {
    values: Mutex<std::collections::HashMap<String, String>>,
    deleted: Mutex<Vec<String>>,
}

impl RecordingSecretStore {
    fn deleted_oauth_keys(&self) -> Vec<String> {
        self.deleted
            .lock()
            .unwrap()
            .iter()
            .filter(|key| key.starts_with("mcp-oauth:"))
            .cloned()
            .collect()
    }
}

impl SecretStore for RecordingSecretStore {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        self.values.lock().unwrap().remove(key);
        self.deleted.lock().unwrap().push(key.to_string());
        Ok(())
    }
}

async fn create_marketplace_test_app_state_with_store(
    path: &Path,
    secret_store: Arc<dyn SecretStore>,
) -> AppState {
    let config = ConfigService::new(path.to_path_buf()).unwrap();
    let observability = ObservabilityService::new(path).unwrap();
    let settings = SettingsService::new(path.to_path_buf());
    let state = AppState::new_with_secret_store(config, observability, settings, secret_store);
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(TEST_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    state
}

#[tokio::test]
async fn disabled_legacy_auto_plugin_oauth_credentials_are_retained_until_uninstall() {
    let tmp = tempfile::tempdir().unwrap();
    let secrets = Arc::new(RecordingSecretStore::default());
    let state = create_marketplace_test_app_state_with_store(tmp.path(), secrets.clone()).await;
    let repo = tmp.path().join("oauth-marketplace-repo");
    build_legacy_auto_oauth_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "oauth-tools".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let credential_store = KeychainOAuthCredentialStore::new(TEST_INSTANCE_ID, secrets.clone());
    let (index_key, credential_key) = plugin_oauth_credential_keys();
    credential_store
        .save(&index_key, r#"{"version":1,"issuers":[null]}"#)
        .await
        .unwrap();
    credential_store
        .save(&credential_key, "opaque-sdk-credential-envelope")
        .await
        .unwrap();
    disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    assert!(
        secrets.deleted_oauth_keys().is_empty(),
        "disable must retain OAuth credentials"
    );
    assert_eq!(
        credential_store
            .load(&credential_key)
            .await
            .unwrap()
            .as_deref(),
        Some("opaque-sdk-credential-envelope")
    );

    uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    assert!(
        !secrets.deleted_oauth_keys().is_empty(),
        "uninstall must clear the disabled Plugin's OAuth credential namespace"
    );
    assert_eq!(credential_store.load(&index_key).await.unwrap(), None);
    assert_eq!(credential_store.load(&credential_key).await.unwrap(), None);
}

fn plugin_oauth_credential_keys() -> (OAuthCredentialKey, OAuthCredentialKey) {
    let mut digest = Sha256::new();
    digest.update(b"A2C Computer\0");
    let grant_fingerprint = format!(
        "v1:authorization_code:dynamic:scopes-{:x}",
        digest.finalize()
    );
    let index = OAuthCredentialKey {
        bundle_id: BundleId::try_from("protected-plugin-mcp").unwrap(),
        resource: "https://mcp.example.test/api".to_string(),
        issuer: None,
        grant_fingerprint,
        record_kind: OAuthCredentialRecordKind::IssuerIndex,
    };
    let credential = OAuthCredentialKey {
        record_kind: OAuthCredentialRecordKind::Credentials,
        ..index.clone()
    };
    (index, credential)
}

#[tokio::test]
async fn capabilities_report_sdk_governance_lifecycle_available() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let capabilities = get_marketplace_capabilities_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(capabilities.computer_lifecycle_api_available);
    assert!(capabilities
        .supported_operations
        .contains(&"install_plugin".to_string()));
    assert!(capabilities
        .required_sdk_apis
        .contains(&"Computer::install_plugin".to_string()));
    assert!(capabilities
        .supported_operations
        .contains(&"reconcile_governance".to_string()));
    assert!(capabilities
        .required_sdk_apis
        .contains(&"Computer::reconcile_governance".to_string()));
    assert!(capabilities
        .required_sdk_apis
        .contains(&"Computer::list_mcp_servers_with_metadata".to_string()));
    assert!(capabilities.reason.contains("available"));
}

#[tokio::test]
async fn governance_report_has_stable_empty_sdk_owned_ledger_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(governance.capabilities.computer_lifecycle_api_available);
    assert!(governance.marketplaces.is_empty());
    assert!(governance.plugins.is_empty());
    assert!(governance.capabilities.reason.contains("available"));
}

#[tokio::test]
async fn marketplace_source_adapter_rejects_remote_file_urls_and_missing_local_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("local marketplace 空格");
    build_marketplace_repo(&repo);

    let error = add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "remote-file".to_string(),
            source: MarketplaceSource::RemoteGit {
                git_url: url::Url::from_file_path(&repo).unwrap().to_string(),
            },
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("must not use file://"));

    let missing = tmp.path().join("missing marketplace");
    let error = add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "missing-local".to_string(),
            source: MarketplaceSource::LocalGit {
                path: missing.display().to_string(),
            },
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("failed to resolve Marketplace local repository path"));

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance.marketplaces.is_empty());
}

#[tokio::test]
async fn local_marketplace_path_is_canonicalized_and_presented_as_a_native_path() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("local marketplace 空格");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "local-market".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.join(".").display().to_string(),
            },
        },
    )
    .await
    .unwrap();

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(
        governance.marketplaces[0].source,
        MarketplaceSourceSummary::LocalGit {
            path: repo.canonicalize().unwrap().display().to_string(),
        }
    );
}

#[tokio::test]
async fn marketplace_lifecycle_commands_use_sdk_errors_without_client_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let error = add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "tf-market".to_string(),
            source: MarketplaceSource::RemoteGit {
                git_url: "not a git url".to_string(),
            },
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("not a well-formed git url"));

    let error = refresh_marketplace_core(&state, TEST_INSTANCE_ID, "tf-market")
        .await
        .unwrap_err();
    assert!(error.contains("unknown marketplace"));

    let error = remove_marketplace_core(&state, TEST_INSTANCE_ID, "tf-market")
        .await
        .unwrap_err();
    assert!(error.contains("unknown marketplace"));

    assert_no_client_governance_ledgers(&state);
}

#[tokio::test]
async fn plugin_lifecycle_commands_use_sdk_errors_without_client_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let request = PluginLifecycleRequest {
        marketplace: "tf-market".to_string(),
        plugin: "desktop-tools".to_string(),
    };

    let install_error = install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap_err();
    assert!(install_error.contains("not added"));

    let enable_error = enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap_err();
    let enable_error = enable_error.to_string();
    assert!(enable_error.contains("not installed") || enable_error.contains("not found"));

    // SDK disable/uninstall are intentionally idempotent around absent runtime materialization.
    let _ = disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone()).await;
    uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();

    assert_no_client_governance_ledgers(&state);
}

#[tokio::test]
async fn marketplace_install_and_uninstall_use_sdk_lifecycle_and_mcp_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();

    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.plugins.len(), 1);
    assert_eq!(governance.plugins[0].plugin, "audit");
    assert_eq!(governance.plugins[0].status, "available");
    assert!(!governance.plugins[0].installed);
    assert_eq!(
        governance.plugins[0].bundled_mcp_servers,
        vec!["audit-mcp".to_string()]
    );
    assert!(governance.plugins[0]
        .bundled_skills
        .contains(&"audit:code-review".to_string()));
    let declared = governance.plugins[0]
        .declared
        .as_ref()
        .expect("local-source available plugin should expose declared capabilities");
    assert_eq!(declared.mcp_servers, vec!["audit-mcp".to_string()]);
    assert!(declared.skills.contains(&"audit:code-review".to_string()));

    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let installed = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(installed.plugins[0].status, "disabled");
    assert!(installed.plugins[0].installed);
    assert!(!installed.plugins[0].enabled);
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.marketplaces.len(), 1);
    assert_eq!(governance.marketplaces[0].name, "acme");
    assert_eq!(governance.plugins.len(), 1);
    assert_eq!(
        governance.plugins[0].plugin_id.as_deref(),
        Some("audit@acme")
    );
    assert!(governance.plugins[0].enabled);
    assert_eq!(governance.plugins[0].status, "enabled");
    assert_eq!(
        governance.plugins[0].bundled_mcp_servers,
        vec!["audit-mcp".to_string()]
    );
    assert!(governance.plugins[0]
        .bundled_skills
        .contains(&"audit:code-review".to_string()));

    let stored = state.sdk_config.load(TEST_INSTANCE_ID);
    let stored_plugin_server = stored
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "audit-mcp")
        .expect("enabled plugin server must be projected by the SDK snapshot");
    assert!(stored_plugin_server.bundled);
    assert_eq!(
        stored_plugin_server.origin,
        a2c_smcp::smcp_computer::settings::config::ProvenanceScope::Plugin
    );
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit_server = servers
        .iter()
        .find(|server| server.name == "audit-mcp")
        .expect("plugin MCP server should be visible to frontend");
    assert!(matches!(
        audit_server.managed_by,
        McpServerManagedBy::Plugin { .. }
    ));

    let injected = inputs::list_inputs_core(&state, TEST_INSTANCE_ID).unwrap();
    assert!(injected
        .iter()
        .all(|input| input.id() != "audit@acme/api_token"));

    let remove_error = remove_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap_err();
    assert!(remove_error.contains("uninstall plugins before removing"));

    uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    let stored = state.sdk_config.load(TEST_INSTANCE_ID);
    assert!(stored
        .mcp
        .servers
        .iter()
        .all(|server| server.name != "audit-mcp"));
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers.iter().all(|server| server.name != "audit-mcp"));

    remove_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap();
    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance.marketplaces.is_empty());
    assert!(governance.plugins.is_empty());
}

#[tokio::test]
async fn plugin_missing_input_does_not_block_computer_across_retry_and_cold_start() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("runtime-input-marketplace");
    build_runtime_input_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let started = start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(started.running);
    assert!(started.runtime.problems.iter().any(|problem| problem
        .technical_detail
        .as_deref()
        .is_some_and(|detail| detail.contains("audit@acme/api_token"))));
    assert!(inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());

    // A normal status refresh rebuilds the persistent input projection. Plugin definitions must
    // remain in the runtime-only pool so the prompt can still store the exact scoped value.
    get_computer_instance_status_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
    assert!(inputs::set_runtime_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "audit@acme/api_token".to_string(),
        serde_json::json!("runtime-secret"),
    )
    .await
    .unwrap());
    restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    runtime
        .remove_plugin_server(&BundleId::try_from("audit-mcp").unwrap())
        .await
        .unwrap();
    tfrobot_client_lib::services::keychain::delete_input_secret(
        state.secret_store.as_ref(),
        TEST_INSTANCE_ID,
        "audit@acme/api_token",
    )
    .unwrap();
    runtime.remount_enabled_plugin_servers().await.unwrap();
    let remounted = restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(remounted.running);
    assert!(remounted.runtime.problems.iter().any(|problem| problem
        .technical_detail
        .as_deref()
        .is_some_and(|detail| detail.contains("audit@acme/api_token"))));
    assert!(inputs::set_runtime_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "audit@acme/api_token".to_string(),
        serde_json::json!("runtime-secret-after-remount"),
    )
    .await
    .unwrap());
    restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    assert!(
        wait_for_mcp_server_running(TEST_INSTANCE_ID, &state, "audit-mcp")
            .await
            .is_some_and(|server| server.running)
    );
    runtime
        .remove_plugin_server(&BundleId::try_from("audit-mcp").unwrap())
        .await
        .unwrap();
    tfrobot_client_lib::services::keychain::delete_input_secret(
        state.secret_store.as_ref(),
        TEST_INSTANCE_ID,
        "audit@acme/api_token",
    )
    .unwrap();
    let listed = tfrobot_client_lib::commands::computer::list_computer_instances_core(&state)
        .await
        .unwrap();
    assert!(listed
        .iter()
        .any(|instance| instance.id == TEST_INSTANCE_ID));
    get_computer_instance_status_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let status_retry = restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(status_retry.running);
    assert!(status_retry.runtime.problems.iter().any(|problem| problem
        .technical_detail
        .as_deref()
        .is_some_and(|detail| detail.contains("audit@acme/api_token"))));
    assert!(inputs::set_runtime_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "audit@acme/api_token".to_string(),
        serde_json::json!("runtime-secret-after-status-refresh"),
    )
    .await
    .unwrap());
    restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let restarted = create_test_app_state(tmp.path());
    let cold_started = start_computer_instance_core(None, &restarted, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(cold_started.running);
    assert!(cold_started.runtime.problems.iter().any(|problem| problem
        .technical_detail
        .as_deref()
        .is_some_and(|detail| detail.contains("audit@acme/api_token"))));
    assert!(inputs::set_runtime_input_value_core(
        &restarted,
        TEST_INSTANCE_ID,
        "audit@acme/api_token".to_string(),
        serde_json::json!("runtime-secret-after-restart"),
    )
    .await
    .unwrap());
    let retried = restart_computer_instance_core(None, &restarted, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(retried.running);
    assert!(
        wait_for_mcp_server_running(TEST_INSTANCE_ID, &restarted, "audit-mcp")
            .await
            .is_some_and(|server| server.running)
    );
    assert!(inputs::list_inputs_core(&restarted, TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn running_plugin_with_multiple_mcps_fails_closed_without_requester_context() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("requesting-mcp-marketplace");
    build_runtime_input_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let error = enable_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap_err();
    let serialized = serde_json::to_value(&error).unwrap();
    assert!(matches!(
        error,
        RuntimeActionError::ResolverFailed { ref input_id, .. }
            if input_id == "audit@acme/api_token"
    ));
    assert_eq!(serialized["code"], "resolver_failed");
    assert_eq!(
        serialized["message"],
        "InputEntry is missing and user confirmation is required"
    );
    assert!(serialized.get("requesting_mcp").is_none());
}

#[tokio::test]
async fn foreground_plugin_enable_confirms_runtime_secret_and_continues_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("interactive-runtime-input-marketplace");
    build_runtime_input_marketplace_repo(&repo);
    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let state = Arc::new(state);
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("plugin-enable-test", true);
    let enable_state = state.clone();
    let enable = tokio::spawn(async move {
        enable_plugin_interactive_core(enable_state.as_ref(), TEST_INSTANCE_ID, request).await
    });
    let prompt = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("foreground plugin enable must request its missing runtime input")
        .unwrap();
    assert_eq!(prompt.definition.id(), "audit@acme/api_token");
    assert!(prompt.secret);

    let completion = bridge.complete(
        &prompt.request_id,
        RuntimeInputCompletion::Confirmed {
            value: "plugin-runtime-secret".to_string(),
        },
    );
    let (completion, enable) = tokio::join!(completion, enable);
    completion.unwrap();
    enable.unwrap().unwrap();

    let entries = inputs::list_input_entries_core(state.as_ref(), TEST_INSTANCE_ID).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "audit@acme/api_token");
    assert!(entries[0].secret);
    assert_eq!(entries[0].value, None);
}

#[tokio::test]
async fn foreground_plugin_enable_stops_after_first_input_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("fail-fast-runtime-input-marketplace");
    build_two_runtime_input_marketplace_repo(&repo);
    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let state = Arc::new(state);
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("plugin-fail-fast-test", true);
    let enable_state = state.clone();
    let enable = tokio::spawn(async move {
        enable_plugin_interactive_core(enable_state.as_ref(), TEST_INSTANCE_ID, request).await
    });
    let first = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("foreground plugin enable must request its first missing input")
        .unwrap();

    let completion = bridge.complete(&first.request_id, RuntimeInputCompletion::Cancelled);
    let (completion, enable) = tokio::join!(completion, enable);
    completion.unwrap();
    assert!(matches!(
        enable.unwrap().unwrap_err(),
        RuntimeActionError::RuntimeInputCancelled { .. }
    ));
    assert!(
        requests.try_recv().is_err(),
        "foreground plugin enable must not request another input after cancellation"
    );
    let servers = mcp::get_mcp_servers_core(state.as_ref(), TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers
        .iter()
        .filter(|server| server.name.ends_with("-fail-fast"))
        .all(|server| !server.running));
}

#[tokio::test]
async fn foreground_computer_start_and_restart_fail_fast_for_enabled_plugin_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp
        .path()
        .join("enabled-plugin-fail-fast-runtime-input-marketplace");
    build_two_runtime_input_marketplace_repo(&repo);
    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    drop(state);

    // Recreate AppState so Computer start must restore the already-enabled plugin through SDK
    // governance before the client-owned desired-server loop runs.
    let state = Arc::new(create_test_app_state(tmp.path()));
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("computer-plugin-fail-fast-test", true);

    let start_state = state.clone();
    let start = tokio::spawn(async move {
        start_computer_instance_interactive_core(
            None,
            start_state.as_ref(),
            TEST_INSTANCE_ID.to_string(),
        )
        .await
    });
    let first_start_prompt = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("foreground Computer start must request the first enabled-plugin input")
        .unwrap();
    let completion = bridge.complete(
        &first_start_prompt.request_id,
        RuntimeInputCompletion::Cancelled,
    );
    let (completion, start) = tokio::join!(completion, start);
    completion.unwrap();
    assert!(matches!(
        start.unwrap().unwrap_err(),
        RuntimeActionError::RuntimeInputCancelled { .. }
    ));
    assert!(
        requests.try_recv().is_err(),
        "foreground Computer start must not request another enabled-plugin input after cancellation"
    );

    // A background start keeps its existing best-effort behavior and leaves both unresolved MCPs
    // stopped. Restart must again prompt exactly once and abort at the first cancellation.
    let started = start_computer_instance_core(None, state.as_ref(), TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(started.running);
    assert!(requests.try_recv().is_err());

    let restart_state = state.clone();
    let restart = tokio::spawn(async move {
        restart_computer_instance_interactive_core(
            None,
            restart_state.as_ref(),
            TEST_INSTANCE_ID.to_string(),
        )
        .await
    });
    let first_restart_prompt = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("foreground Computer restart must request the first enabled-plugin input")
        .unwrap();
    let completion = bridge.complete(
        &first_restart_prompt.request_id,
        RuntimeInputCompletion::Cancelled,
    );
    let (completion, restart) = tokio::join!(completion, restart);
    completion.unwrap();
    assert!(matches!(
        restart.unwrap().unwrap_err(),
        RuntimeActionError::RuntimeInputCancelled { .. }
    ));
    assert!(
        requests.try_recv().is_err(),
        "foreground Computer restart must not request another enabled-plugin input after cancellation"
    );

    let servers = mcp::get_mcp_servers_core(state.as_ref(), TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers
        .iter()
        .filter(|server| server.name.ends_with("-fail-fast"))
        .all(|server| !server.running));
}

#[tokio::test]
async fn plugin_runtime_definition_stays_out_of_sdk_export_while_its_entry_is_managed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("runtime-input-import-export-marketplace");
    build_runtime_input_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    assert!(inputs::set_runtime_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "audit@acme/api_token".to_string(),
        serde_json::json!("runtime-secret"),
    )
    .await
    .unwrap());
    assert!(inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
    let entries = inputs::list_input_entries_core(&state, TEST_INSTANCE_ID).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "audit@acme/api_token");
    assert!(entries[0].secret);
    assert_eq!(entries[0].value, None);

    let import_path = tmp.path().join("client-owned-input.json");
    fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [],
            "inputs": [{
                "type": "PromptString",
                "id": "client-note",
                "label": "Client note",
                "default": "saved by the user"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let imported = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(imported.inputs_imported, 1);

    get_computer_instance_status_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let persisted_input_ids = inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .into_iter()
        .map(|input| input.id().to_string())
        .collect::<Vec<_>>();
    assert_eq!(persisted_input_ids, vec!["client-note".to_string()]);

    let export_path = tmp.path().join("client-owned-export.json");
    config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();
    let exported: serde_json::Value =
        serde_json::from_slice(&fs::read(export_path).unwrap()).unwrap();
    let exported_input_ids = exported["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|input| input["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(exported_input_ids, vec!["client-note"]);
}

#[tokio::test]
async fn duplicate_copies_only_user_skills_and_keeps_plugin_governance_isolated() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let source_skill_home = state.config.default_local_skills_root(TEST_INSTANCE_ID);
    let user_skill = source_skill_home.join("user/local-review/SKILL.md");
    fs::create_dir_all(user_skill.parent().unwrap()).unwrap();
    fs::write(
        &user_skill,
        "---\nname: local-review\ndescription: local review\n---\nbody",
    )
    .unwrap();

    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);
    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let plugin = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, plugin.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, plugin)
        .await
        .unwrap();

    assert!(source_skill_home.join("known_marketplaces.json").is_file());
    assert!(source_skill_home.join("installed_plugins.json").is_file());
    assert!(source_skill_home.join("marketplace").is_dir());

    let source_status = get_computer_instance_status_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let dashboard = get_dashboard_data_core(&state).await.unwrap();
    let dashboard_source = dashboard
        .computers
        .iter()
        .find(|computer| computer.id == TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(source_status.mcp_server_count, 1);
    assert_eq!(dashboard_source.mcp_server_count, 1);

    let duplicate = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: TEST_INSTANCE_ID.to_string(),
            name: "Isolated Duplicate".to_string(),
            description: None,
            copy_robot_binding: false,
            connection_target_id: None,
            skill_home_mode: DuplicateSkillHomeMode::Copy,
        },
    )
    .await
    .unwrap();
    let duplicate_skill_home = state.config.default_local_skills_root(&duplicate.id);

    assert!(duplicate_skill_home
        .join("user/local-review/SKILL.md")
        .is_file());
    for sdk_owned_path in [
        "known_marketplaces.json",
        "installed_plugins.json",
        "installed_plugins_intent.json",
        "marketplace",
        "mcp",
    ] {
        assert!(
            !duplicate_skill_home.join(sdk_owned_path).exists(),
            "duplicate must not inherit SDK-owned path {sdk_owned_path}"
        );
    }
    let duplicate_governance = get_marketplace_governance_core(&state, &duplicate.id)
        .await
        .unwrap();
    assert!(duplicate_governance.marketplaces.is_empty());
    assert!(duplicate_governance.plugins.is_empty());

    let source_governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(source_governance.marketplaces.len(), 1);
    assert_eq!(source_governance.plugins.len(), 1);
    assert!(source_governance.plugins[0].enabled);
}

#[tokio::test]
async fn plugin_dependency_claims_bundle_only_while_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let add_error =
        mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("audit-mcp"))
            .await
            .unwrap_err();
    assert!(add_error.to_string().contains("Marketplace plugin"));

    let import_path = tmp.path().join("plugin-name-collision-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [echo_server_config("audit-mcp")],
            "inputs": []
        }))
        .unwrap(),
    )
    .unwrap();
    let imported = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .expect("config import must not depend on enabled plugin runtime ownership");
    assert_eq!(imported.servers_imported, 1);
    assert!(imported.servers_skipped.is_empty());
    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "audit-mcp")
        .await
        .unwrap();

    disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("audit-mcp"),
    )
    .await
    .unwrap();

    let stored = state.sdk_config.load(TEST_INSTANCE_ID);
    assert!(stored
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "audit-mcp" && !server.bundled));
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("audit-mcp"),
    )
    .await
    .expect("bundled name hints must not block editing a config declaration");
    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "audit-mcp")
        .await
        .expect("bundled name hints must not block removing a config declaration");
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("audit-mcp"),
    )
    .await
    .unwrap();

    let restarted = create_test_app_state(tmp.path());
    let restarted_status =
        get_computer_instance_status_core(&restarted, TEST_INSTANCE_ID.to_string())
            .await
            .unwrap();
    assert_eq!(restarted_status.mcp_server_count, 1);
    let restarted_servers = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(restarted_servers.iter().any(|server| {
        server.name == "audit-mcp" && matches!(server.managed_by, McpServerManagedBy::User)
    }));

    enable_plugin_core(&restarted, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let servers = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit_rows: Vec<_> = servers
        .iter()
        .filter(|server| server.name == "audit-mcp")
        .collect();
    assert_eq!(audit_rows.len(), 1);
    assert!(matches!(
        audit_rows[0].managed_by,
        McpServerManagedBy::Plugin { .. }
    ));

    start_computer_instance_core(None, &restarted, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    mcp::add_mcp_server_core(
        &restarted,
        TEST_INSTANCE_ID,
        echo_server_config("user-batch-mcp"),
    )
    .await
    .unwrap();

    let before_batch = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(before_batch.iter().any(|server| {
        server.name == "audit-mcp"
            && server.running
            && matches!(server.managed_by, McpServerManagedBy::Plugin { .. })
    }));
    assert!(before_batch.iter().any(|server| {
        server.name == "user-batch-mcp"
            && !server.running
            && matches!(server.managed_by, McpServerManagedBy::User)
    }));

    restart_computer_instance_core(None, &restarted, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let after_restart = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(after_restart.iter().any(|server| {
        server.name == "audit-mcp"
            && server.running
            && matches!(server.managed_by, McpServerManagedBy::Plugin { .. })
    }));
    assert!(after_restart.iter().any(|server| {
        server.name == "user-batch-mcp"
            && server.running
            && matches!(server.managed_by, McpServerManagedBy::User)
    }));

    let start_batch = mcp::start_all_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(start_batch.candidate_count, 1);
    assert_eq!(start_batch.actual_operation_count, 0);
    assert_eq!(start_batch.unchanged_count, 1);
    assert_eq!(start_batch.excluded_plugin_owned_count, 1);
    assert!(start_batch.failures.is_empty());

    let stop_batch = mcp::stop_all_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(stop_batch.candidate_count, 1);
    assert_eq!(stop_batch.actual_operation_count, 1);
    assert_eq!(stop_batch.unchanged_count, 0);
    assert_eq!(stop_batch.excluded_plugin_owned_count, 1);
    assert!(stop_batch.failures.is_empty());
    let after_batch = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(after_batch.iter().any(|server| {
        server.name == "audit-mcp"
            && server.running
            && matches!(server.managed_by, McpServerManagedBy::Plugin { .. })
    }));
    assert!(after_batch
        .iter()
        .any(|server| server.name == "user-batch-mcp" && !server.running));

    let audit_bundle_id = BundleId::try_from("audit-mcp").unwrap();
    let start_error = mcp::start_mcp_server_core(&restarted, TEST_INSTANCE_ID, &audit_bundle_id)
        .await
        .unwrap_err();
    assert!(start_error.to_string().contains("not user-manageable"));

    disable_plugin_core(&restarted, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let disabled_rows = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(
        disabled_rows.iter().any(|server| {
            server.name == "audit-mcp" && matches!(server.managed_by, McpServerManagedBy::User)
        }),
        "independent declaration missing after disable: {disabled_rows:?}"
    );

    mcp::start_mcp_server_core(&restarted, TEST_INSTANCE_ID, &audit_bundle_id)
        .await
        .unwrap();
    assert!(mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .any(|server| server.name == "audit-mcp" && server.running));
    mcp::stop_mcp_server_core(&restarted, TEST_INSTANCE_ID, &audit_bundle_id)
        .await
        .unwrap();

    mcp::start_all_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .any(|server| server.name == "audit-mcp" && server.running));
    mcp::stop_all_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();

    mcp::update_mcp_server_core(
        &restarted,
        TEST_INSTANCE_ID,
        echo_server_config("audit-mcp"),
    )
    .await
    .unwrap();

    mcp::remove_mcp_server_core(&restarted, TEST_INSTANCE_ID, "audit-mcp")
        .await
        .unwrap();
    enable_plugin_core(&restarted, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let remounted = wait_for_mcp_server_running(TEST_INSTANCE_ID, &restarted, "audit-mcp")
        .await
        .expect("enabled plugin MCP should take over immediately after user removal");
    assert!(matches!(
        remounted.managed_by,
        McpServerManagedBy::Plugin { .. }
    ));
    let governance = get_marketplace_governance_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance.plugins.iter().any(|plugin| {
        plugin.marketplace == "acme" && plugin.plugin == "audit" && plugin.enabled
    }));
    let audit_rows: Vec<_> = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .into_iter()
        .filter(|server| server.name == "audit-mcp")
        .collect();
    assert_eq!(audit_rows.len(), 1);
    assert!(audit_rows[0].running);
    disable_plugin_core(&restarted, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    mcp::add_mcp_server_core(
        &restarted,
        TEST_INSTANCE_ID,
        echo_server_config("audit-mcp"),
    )
    .await
    .unwrap();

    uninstall_plugin_core(&restarted, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    assert!(restarted
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "audit-mcp" && !server.bundled));
}

#[tokio::test]
async fn plugin_disable_removes_skills_from_active_skill_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let active_before_disable = skills::list_skills_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(active_before_disable
        .iter()
        .any(|skill| skill.name == "audit:code-review"));

    disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let plugin = governance
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id.as_deref() == Some("audit@acme"))
        .expect("disabled plugin should remain visible in governance");
    assert_eq!(plugin.status, "disabled");
    assert!(!plugin.enabled);

    let active_after_disable = skills::list_skills_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(active_after_disable
        .iter()
        .all(|skill| skill.name != "audit:code-review"));
    let error = skills::get_skill_core(&state, TEST_INSTANCE_ID, "audit:code-review", None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        skills::SkillCommandError::SkillNotFound { .. }
    ));

    enable_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    let active_after_enable = skills::list_skills_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(active_after_enable
        .iter()
        .any(|skill| skill.name == "audit:code-review"));
}

#[tokio::test]
async fn shared_plugin_dependency_hands_off_and_is_reclaimed_after_the_last_disable() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_duplicate_mcp_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("audit-mcp", true),
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();
    enable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let duplicate_request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "duplicate".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, duplicate_request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, duplicate_request.clone())
        .await
        .unwrap();

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let duplicate = governance
        .plugins
        .iter()
        .find(|plugin| plugin.plugin == "duplicate")
        .expect("duplicate plugin should remain available in the catalog");
    assert!(duplicate.installed);
    assert!(duplicate.enabled);

    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit_rows: Vec<_> = servers
        .iter()
        .filter(|server| server.name == "audit-mcp")
        .collect();
    assert_eq!(audit_rows.len(), 1);
    assert!(matches!(
        audit_rows[0].managed_by,
        McpServerManagedBy::Plugin { .. }
    ));

    disable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();
    let after_first_disable = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let shared = after_first_disable
        .iter()
        .find(|server| server.bundle_id.as_str() == "audit-mcp")
        .expect("the remaining plugin dependency must keep the shared bundle mounted");
    match &shared.managed_by {
        McpServerManagedBy::Plugin { plugin, .. } => assert_eq!(plugin, "duplicate"),
        other => panic!("expected ownership to hand off to duplicate, got {other:?}"),
    }

    disable_plugin_core(&state, TEST_INSTANCE_ID, duplicate_request)
        .await
        .unwrap();
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .all(|server| server.bundle_id.as_str() != "audit-mcp"));
}

#[tokio::test]
async fn enabling_plugin_starts_a_stopped_independent_bundle_dependency() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("audit-mcp"))
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let bundle_id = BundleId::try_from("audit-mcp").unwrap();
    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id)
        .await
        .unwrap();
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .any(|server| server.bundle_id.as_str() == "audit-mcp" && !server.running));

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let started = wait_for_mcp_server_running(TEST_INSTANCE_ID, &state, "audit-mcp")
        .await
        .expect("independent dependency should remain visible");
    assert!(started.running);
    assert!(matches!(
        started.managed_by,
        McpServerManagedBy::Plugin { .. }
    ));

    disable_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    let restored = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .into_iter()
        .find(|server| server.bundle_id.as_str() == "audit-mcp")
        .expect("disabling the plugin must preserve the independent declaration");
    assert!(restored.running);
    assert!(matches!(restored.managed_by, McpServerManagedBy::User));
}

#[tokio::test]
async fn plugin_enable_claims_an_existing_user_bundle_dependency_until_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("audit-mcp"))
        .await
        .unwrap();

    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit = governance
        .plugins
        .iter()
        .find(|plugin| plugin.plugin == "audit")
        .expect("audit plugin should remain available in the catalog");
    assert!(audit.installed);
    assert!(audit.enabled);
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers.iter().any(|server| {
        server.name == "audit-mcp" && matches!(server.managed_by, McpServerManagedBy::Plugin { .. })
    }));
    disable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .any(|server| {
            server.name == "audit-mcp" && matches!(server.managed_by, McpServerManagedBy::User)
        }));
}

#[tokio::test]
async fn enabled_plugin_overrides_disabled_user_fallback_until_plugin_is_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("audit-mcp", true),
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("unrelated-mcp"),
    )
    .await
    .unwrap();

    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let active = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .into_iter()
        .find(|server| server.bundle_id.as_str() == "audit-mcp")
        .expect("enabled plugin MCP must override its disabled user fallback");
    assert!(active.running);
    assert!(matches!(
        active.managed_by,
        McpServerManagedBy::Plugin { .. }
    ));

    stop_computer_instance_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let restarted = create_test_app_state(tmp.path());
    start_computer_instance_core(None, &restarted, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    disable_plugin_core(&restarted, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    let restored_rows = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(
        restored_rows
            .iter()
            .all(|server| server.bundle_id.as_str() != "audit-mcp"),
        "disabled user fallback must be hidden after plugin release: {restored_rows:#?}"
    );
    assert!(restarted
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "audit-mcp" && server.config.disabled()));
    assert!(mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .any(|server| server.name == "unrelated-mcp" && server.running));
}

#[tokio::test]
async fn direct_plugin_uninstall_restores_a_disabled_user_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("audit-mcp", true),
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };
    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();

    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .all(|server| server.bundle_id.as_str() != "audit-mcp"));
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "audit-mcp" && server.config.disabled()));
}

#[tokio::test]
async fn config_import_is_independent_from_dynamic_plugin_runtime_ownership() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();
    enable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let import_path = tmp.path().join("import.json");
    let user_config = echo_server_config("audit-mcp");
    fs::write(
        &import_path,
        serde_json::json!({ "servers": [user_config], "inputs": [] }).to_string(),
    )
    .unwrap();
    let result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        Some(config_io::ConfigFormat::CliNative),
    )
    .await
    .unwrap();

    assert_eq!(result.servers_imported, 1);
    assert!(result.servers_skipped.is_empty());
    let claude_path = tmp.path().join("claude-import.json");
    fs::write(
        &claude_path,
        serde_json::json!({
            "mcpServers": {
                "audit-mcp": {
                    "command": "node",
                    "args": [echo_server_path().to_string_lossy().to_string()],
                    "env": {}
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let claude_result = config_io::import_config_core(
        &state,
        claude_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        Some(config_io::ConfigFormat::ClaudeDesktop),
    )
    .await
    .unwrap();
    assert_eq!(claude_result.servers_imported, 1);
    assert!(claude_result.servers_skipped.is_empty());
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "audit-mcp"));

    let rows = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit = rows
        .iter()
        .find(|server| server.name == "audit-mcp")
        .expect("plugin runtime server remains visible");
    assert!(matches!(
        audit.managed_by,
        McpServerManagedBy::Plugin { .. }
    ));
}

#[tokio::test]
async fn computer_bootup_starts_enabled_plugin_mcp_servers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    enable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let before_boot = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit_before_boot = before_boot
        .iter()
        .find(|server| server.name == "audit-mcp")
        .expect("plugin MCP server should be visible before boot");
    assert!(!audit_before_boot.running);

    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let audit_after_boot = wait_for_mcp_server_running(TEST_INSTANCE_ID, &state, "audit-mcp")
        .await
        .expect("plugin MCP server should remain visible after boot");
    assert!(
        audit_after_boot.running,
        "enabled plugin MCP server should start with Computer bootup; status: {}",
        audit_after_boot.status_message
    );
}

#[tokio::test]
async fn plugin_install_and_enable_start_mcp_when_computer_is_running() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    let request = PluginLifecycleRequest {
        marketplace: "acme".to_string(),
        plugin: "audit".to_string(),
    };

    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let installed_governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(!installed_governance.plugins[0].enabled);
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .all(|server| server.name != "audit-mcp"));
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let installed_server = wait_for_mcp_server_running(TEST_INSTANCE_ID, &state, "audit-mcp")
        .await
        .expect("plugin enable should start bundled MCP server when Computer is running");
    assert!(installed_server.running);

    disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();

    let enabled_server = wait_for_mcp_server_running(TEST_INSTANCE_ID, &state, "audit-mcp")
        .await
        .expect("plugin enable should start bundled MCP server when Computer is running");
    assert!(enabled_server.running);
}

#[tokio::test]
async fn enabled_plugin_mcp_remounts_from_ledger_after_app_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();
    enable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let restarted = create_test_app_state(tmp.path());
    let servers = mcp::get_mcp_servers_core(&restarted, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit = servers
        .iter()
        .find(|server| server.name == "audit-mcp")
        .expect("enabled plugin MCP should remount from installed plugin ledger");

    assert!(!audit.running);
    assert!(matches!(
        audit.managed_by,
        McpServerManagedBy::Plugin { .. }
    ));
}

async fn wait_for_mcp_server_running(
    instance_id: &str,
    state: &AppState,
    name: &str,
) -> Option<mcp::McpServerStatus> {
    for _ in 0..120 {
        let servers = mcp::get_mcp_servers_core(state, instance_id).await.ok()?;
        let server = servers.into_iter().find(|server| server.name == name)?;
        if server.running {
            return Some(server);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    mcp::get_mcp_servers_core(state, instance_id)
        .await
        .ok()?
        .into_iter()
        .find(|server| server.name == name)
}

#[tokio::test]
async fn marketplace_remove_requires_plugins_to_be_uninstalled_first() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let error = remove_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap_err();

    assert!(error.contains("has installed plugins"));
    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.marketplaces.len(), 1);
    assert_eq!(governance.plugins.len(), 1);
}

#[tokio::test]
async fn marketplace_update_is_rejected_without_losing_the_existing_marketplace() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let first_repo = tmp.path().join("first-marketplace-repo");
    let second_repo = tmp.path().join("second-marketplace-repo");
    build_marketplace_repo(&first_repo);
    build_tf45_isolation_marketplace_repo(&second_repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: first_repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();

    let error = update_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        UpdateMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: second_repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap_err();

    assert!(error.contains("atomic update API"));
    assert!(error.contains("left unchanged"));

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.marketplaces.len(), 1);
    assert_eq!(
        governance.marketplaces[0].source,
        MarketplaceSourceSummary::LocalGit {
            path: first_repo.canonicalize().unwrap().display().to_string(),
        }
    );
    assert_eq!(governance.plugins.len(), 1);
    assert_eq!(governance.plugins[0].plugin, "audit");
    assert_eq!(governance.plugins[0].status, "available");
}

#[tokio::test]
async fn local_marketplace_refresh_only_loads_committed_git_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("refresh-marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();

    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"},{"name":"notes","source":"./plugins/notes"}]}"#,
    )
    .unwrap();
    let notes_skill = repo.join("plugins/notes/skills/notes");
    fs::create_dir_all(&notes_skill).unwrap();
    fs::write(
        notes_skill.join("SKILL.md"),
        "---\nname: notes\ndescription: notes\n---\nbody",
    )
    .unwrap();

    refresh_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap();
    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance
        .plugins
        .iter()
        .all(|plugin| plugin.plugin != "notes"));

    run_git(&repo, &["add", "-A"]);
    run_git(
        &repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "add notes plugin",
        ],
    );
    refresh_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap();
    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance
        .plugins
        .iter()
        .any(|plugin| plugin.plugin == "notes"));
}

#[tokio::test]
async fn invalid_local_marketplace_is_rejected_without_governance_residue() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("invalid-marketplace-repo");
    build_invalid_marketplace_repo(&repo);

    let error = add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "invalid".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap_err();

    assert!(error.contains("add failed"));
    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance.marketplaces.is_empty());
    assert!(governance.plugins.is_empty());
}

#[tokio::test]
async fn refresh_reports_a_committed_invalid_manifest_as_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("refresh-invalid-marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();

    fs::write(repo.join(".tfrobot-plugin/marketplace.json"), "not-json").unwrap();
    run_git(&repo, &["add", "-A"]);
    run_git(
        &repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "break manifest",
        ],
    );

    let error = refresh_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap_err();
    assert!(error.contains("refresh failed"));

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.marketplaces[0].status, "degraded");
    assert!(governance.marketplaces[0].message.is_some());
}

#[tokio::test]
async fn marketplace_update_requires_plugins_to_be_uninstalled_first() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let first_repo = tmp.path().join("first-marketplace-repo");
    let second_repo = tmp.path().join("second-marketplace-repo");
    build_marketplace_repo(&first_repo);
    build_tf45_isolation_marketplace_repo(&second_repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: first_repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let error = update_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        UpdateMarketplaceRequest {
            name: "acme".to_string(),
            source: MarketplaceSource::LocalGit {
                path: second_repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap_err();

    assert!(error.contains("atomic update API"));

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.marketplaces.len(), 1);
    assert!(governance.plugins[0].installed);
}

#[tokio::test]
async fn plugin_enable_state_is_isolated_per_computer_instance() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_SECOND_INSTANCE_ID, "Computer B"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(TEST_SECOND_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);
    for instance_id in [TEST_INSTANCE_ID, TEST_SECOND_INSTANCE_ID] {
        add_marketplace_core(
            &state,
            instance_id,
            AddMarketplaceRequest {
                name: "acme".to_string(),
                source: MarketplaceSource::LocalGit {
                    path: repo.display().to_string(),
                },
            },
        )
        .await
        .unwrap();
        install_plugin_core(
            &state,
            instance_id,
            PluginLifecycleRequest {
                marketplace: "acme".to_string(),
                plugin: "audit".to_string(),
            },
        )
        .await
        .unwrap();
        enable_plugin_core(
            &state,
            instance_id,
            PluginLifecycleRequest {
                marketplace: "acme".to_string(),
                plugin: "audit".to_string(),
            },
        )
        .await
        .unwrap();
    }

    disable_plugin_core(
        &state,
        TEST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: "acme".to_string(),
            plugin: "audit".to_string(),
        },
    )
    .await
    .unwrap();

    let first = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let second = get_marketplace_governance_core(&state, TEST_SECOND_INSTANCE_ID)
        .await
        .unwrap();

    assert!(!first.plugins[0].enabled);
    assert_eq!(first.plugins[0].status, "disabled");
    assert!(second.plugins[0].enabled);
    assert_eq!(second.plugins[0].status, "enabled");
}

#[tokio::test]
async fn plugin_install_materializes_mcp_and_skills_only_for_current_instance() {
    const FIRST_INSTANCE_ID: &str = "tf45-computer-a";
    const SECOND_INSTANCE_ID: &str = "tf45-computer-b";
    const MARKETPLACE: &str = "tf45-acme";
    const PLUGIN: &str = "tf45-audit";
    const SERVER: &str = "tf45-audit-mcp";
    const SKILL: &str = "tf45-audit:scope-review";

    let tmp = tempfile::tempdir().unwrap();
    let state = create_test_app_state(tmp.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(FIRST_INSTANCE_ID, "TF45 Computer A"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(FIRST_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    state
        .config
        .add_computer_instance(ComputerInstance::new(SECOND_INSTANCE_ID, "TF45 Computer B"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(SECOND_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    let repo = tmp.path().join("marketplace-repo");
    build_tf45_isolation_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        FIRST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: MARKETPLACE.to_string(),
            source: MarketplaceSource::LocalGit {
                path: repo.display().to_string(),
            },
        },
    )
    .await
    .unwrap();
    install_plugin_core(
        &state,
        FIRST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: MARKETPLACE.to_string(),
            plugin: PLUGIN.to_string(),
        },
    )
    .await
    .unwrap();
    enable_plugin_core(
        &state,
        FIRST_INSTANCE_ID,
        PluginLifecycleRequest {
            marketplace: MARKETPLACE.to_string(),
            plugin: PLUGIN.to_string(),
        },
    )
    .await
    .unwrap();

    let first_governance = get_marketplace_governance_core(&state, FIRST_INSTANCE_ID)
        .await
        .unwrap();
    let second_governance = get_marketplace_governance_core(&state, SECOND_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(first_governance.plugins.len(), 1);
    assert!(second_governance.marketplaces.is_empty());
    assert!(second_governance.plugins.is_empty());

    let first_servers = mcp::get_mcp_servers_core(&state, FIRST_INSTANCE_ID)
        .await
        .unwrap();
    let second_servers = mcp::get_mcp_servers_core(&state, SECOND_INSTANCE_ID)
        .await
        .unwrap();
    assert!(first_servers.iter().any(|server| server.name == SERVER));
    assert!(second_servers.iter().all(|server| server.name != SERVER));

    let first_skills = skills::list_skills_core(&state, FIRST_INSTANCE_ID)
        .await
        .unwrap();
    let second_skills = skills::list_skills_core(&state, SECOND_INSTANCE_ID)
        .await
        .unwrap();
    assert!(first_skills.iter().any(|skill| {
        skill.name == SKILL && skill.source == format!("marketplace:{MARKETPLACE}")
    }));
    assert!(second_skills.iter().all(|skill| skill.name != SKILL));
}

#[tokio::test]
async fn lifecycle_commands_validate_instance_before_reporting_capability() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let error = install_plugin_core(
        &state,
        "missing-instance",
        PluginLifecycleRequest {
            marketplace: "tf-market".to_string(),
            plugin: "desktop-tools".to_string(),
        },
    )
    .await
    .unwrap_err();

    assert_eq!(error, "Computer instance not found: missing-instance");
}

fn build_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"}]}"#,
    )
    .unwrap();
    let skill = repo.join("plugins/audit/skills/code-review");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: code-review\ndescription: review code\n---\nbody",
    )
    .unwrap();
    let servers = repo.join("plugins/audit/mcp-servers");
    fs::create_dir_all(&servers).unwrap();
    write_mcp_server_config(&servers.join("audit-mcp.json"), "audit-mcp");
    fs::write(
        servers.join("inputs.json"),
        r#"{"inputs":[{"type":"PromptString","id":"api_token","description":"API Token","default":"demo","password":true}]}"#,
    )
    .unwrap();

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "init",
        ],
    );
}

fn build_invalid_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(repo.join(".tfrobot-plugin/marketplace.json"), "not-json").unwrap();
    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "invalid manifest",
        ],
    );
}

fn build_legacy_auto_oauth_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"oauth-tools","source":"./plugins/oauth-tools"}]}"#,
    )
    .unwrap();
    let servers = repo.join("plugins/oauth-tools/mcp-servers");
    fs::create_dir_all(&servers).unwrap();
    fs::write(
        servers.join("protected-plugin-mcp.json"),
        serde_json::to_vec(&serde_json::json!({
            "type": "Http",
            "name": "protected-plugin-mcp",
            "server_parameters": {
                "url": "https://mcp.example.test/api",
                "headers": {}
            }
        }))
        .unwrap(),
    )
    .unwrap();

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "init",
        ],
    );
}

fn build_runtime_input_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"}]}"#,
    )
    .unwrap();
    let servers = repo.join("plugins/audit/mcp-servers");
    fs::create_dir_all(&servers).unwrap();
    let server_path = echo_server_path();
    write_mcp_server_config(&servers.join("00-healthy-mcp.json"), "00-healthy-mcp");
    fs::write(
        servers.join("audit-mcp.json"),
        serde_json::to_vec(&serde_json::json!({
            "type": "stdio",
            "name": "audit-mcp",
            "server_parameters": {
                "command": "node",
                "args": [server_path],
                "env": {
                    "API_TOKEN": "${input:audit@acme/api_token}"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        servers.join("inputs.json"),
        r#"{"inputs":[{"type":"PromptString","id":"api_token","description":"API Token","password":true}]}"#,
    )
    .unwrap();

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "init",
        ],
    );
}

fn build_two_runtime_input_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"}]}"#,
    )
    .unwrap();
    let servers = repo.join("plugins/audit/mcp-servers");
    fs::create_dir_all(&servers).unwrap();
    let server_path = echo_server_path();
    for (ordinal, input_id) in [("00", "first_token"), ("01", "second_token")] {
        fs::write(
            servers.join(format!("{ordinal}-fail-fast.json")),
            serde_json::to_vec(&serde_json::json!({
                "type": "stdio",
                "name": format!("{ordinal}-fail-fast"),
                "server_parameters": {
                    "command": "node",
                    "args": [server_path.clone()],
                    "env": {
                        "TOKEN": format!("${{input:audit@acme/{input_id}}}")
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }
    fs::write(
        servers.join("inputs.json"),
        r#"{"inputs":[{"type":"PromptString","id":"first_token","description":"First token"},{"type":"PromptString","id":"second_token","description":"Second token"}]}"#,
    )
    .unwrap();

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "init",
        ],
    );
}

fn build_duplicate_mcp_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"audit","source":"./plugins/audit"},{"name":"duplicate","source":"./plugins/duplicate"}]}"#,
    )
    .unwrap();
    for plugin in ["audit", "duplicate"] {
        let skill = repo.join(format!("plugins/{plugin}/skills/code-review"));
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: code-review\ndescription: review code\n---\nbody",
        )
        .unwrap();
        let servers = repo.join(format!("plugins/{plugin}/mcp-servers"));
        fs::create_dir_all(&servers).unwrap();
        write_mcp_server_config(&servers.join("audit-mcp.json"), "audit-mcp");
    }

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "init",
        ],
    );
}

fn build_tf45_isolation_marketplace_repo(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"tf45-audit","source":"./plugins/tf45-audit"}]}"#,
    )
    .unwrap();
    let skill = repo.join("plugins/tf45-audit/skills/scope-review");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: scope-review\ndescription: scoped review\n---\nbody",
    )
    .unwrap();
    let servers = repo.join("plugins/tf45-audit/mcp-servers");
    fs::create_dir_all(&servers).unwrap();
    write_mcp_server_config(&servers.join("tf45-audit-mcp.json"), "tf45-audit-mcp");

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-qm",
            "init",
        ],
    );
}

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_no_client_governance_ledgers(state: &AppState) {
    let skill_home = state.config.default_local_skills_root(TEST_INSTANCE_ID);
    assert!(
        !skill_home.join("known_marketplaces.json").exists(),
        "tfrobot-client must not create SDK marketplace ledger"
    );
    assert!(
        !skill_home.join("installed_plugins.json").exists(),
        "tfrobot-client must not create SDK plugin ledger"
    );
    assert!(
        !skill_home.join("marketplace").exists(),
        "tfrobot-client must not stage marketplace content without SDK lifecycle APIs"
    );
}
