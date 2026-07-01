//! Integration tests for the Skills Marketplace governance boundary.
//!
//! The current smcp-computer version does not expose Computer-level
//! marketplace/plugin lifecycle APIs. tfrobot-client must fail clearly instead
//! of creating its own SDK governance ledger.

mod common;

use common::create_test_app_state;
use tfrobot_client_lib::commands::marketplace::{
    add_marketplace_core, disable_plugin_core, enable_plugin_core,
    get_marketplace_capabilities_core, get_marketplace_governance_core, install_plugin_core,
    reconcile_governance_core, refresh_marketplace_core, remove_marketplace_core,
    uninstall_plugin_core,
    AddMarketplaceRequest, PluginLifecycleRequest,
};
use tfrobot_client_lib::services::computer::ComputerInstance;
use tfrobot_client_lib::AppState;

const TEST_INSTANCE_ID: &str = "computer-a";

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
        .await;
    state
}

#[tokio::test]
async fn capabilities_report_sdk_governance_lifecycle_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let capabilities = get_marketplace_capabilities_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(!capabilities.computer_lifecycle_api_available);
    assert!(capabilities.supported_operations.is_empty());
    assert!(capabilities
        .required_sdk_apis
        .contains(&"Computer::install_plugin".to_string()));
    assert!(capabilities.reason.contains("will not emulate"));
}

#[tokio::test]
async fn governance_report_has_stable_empty_sdk_owned_ledger_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(!governance.capabilities.computer_lifecycle_api_available);
    assert!(governance.marketplaces.is_empty());
    assert!(governance.plugins.is_empty());
    assert!(governance.capabilities.reason.contains("will not emulate"));
}

#[tokio::test]
async fn marketplace_lifecycle_commands_fail_without_creating_client_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let error = add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "tf-market".to_string(),
            git_url: "https://example.invalid/market.git".to_string(),
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("does not expose Computer-level"));

    let error = refresh_marketplace_core(&state, TEST_INSTANCE_ID, "tf-market")
        .await
        .unwrap_err();
    assert!(error.contains("does not expose Computer-level"));

    let error = remove_marketplace_core(&state, TEST_INSTANCE_ID, "tf-market")
        .await
        .unwrap_err();
    assert!(error.contains("does not expose Computer-level"));

    assert_no_client_governance_ledgers(&state);
}

#[tokio::test]
async fn plugin_lifecycle_commands_fail_without_creating_client_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let request = PluginLifecycleRequest {
        marketplace: "tf-market".to_string(),
        plugin: "desktop-tools".to_string(),
    };

    for error in [
        install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
            .await
            .unwrap_err(),
        enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
            .await
            .unwrap_err(),
        disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
            .await
            .unwrap_err(),
        uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
            .await
            .unwrap_err(),
    ] {
        assert!(error.contains("will not emulate"));
    }

    assert_no_client_governance_ledgers(&state);
}

#[tokio::test]
async fn reconcile_governance_fails_without_creating_client_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let error = reconcile_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap_err();
    assert!(error.contains("does not expose Computer-level"));

    assert_no_client_governance_ledgers(&state);
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
