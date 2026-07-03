//! Integration tests for the Skills Marketplace governance boundary.
//!
//! tfrobot-client drives marketplace/plugin lifecycle through SDK Computer-level
//! APIs and does not create its own governance ledger.

mod common;

use common::{create_test_app_state, echo_server_config, echo_server_path};
use std::fs;
use std::path::Path;
use std::process::Command;
use tfrobot_client_lib::commands::{
    computer::start_computer_instance_core,
    marketplace::{
        add_marketplace_core, disable_plugin_core, enable_plugin_core,
        get_marketplace_capabilities_core, get_marketplace_governance_core, install_plugin_core,
        refresh_marketplace_core, remove_marketplace_core, uninstall_plugin_core,
        update_marketplace_core, AddMarketplaceRequest, PluginLifecycleRequest,
        UpdateMarketplaceRequest,
    },
    mcp, skills,
};
use tfrobot_client_lib::services::computer::{ComputerInstance, McpServerManagedBy};
use tfrobot_client_lib::AppState;

const TEST_INSTANCE_ID: &str = "computer-a";
const TEST_SECOND_INSTANCE_ID: &str = "computer-b";

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
    // Cold-start governance recovery/remount is intentionally not advertised as
    // a client capability until SDK exposes the complete instance-scoped
    // contract. The client must not fill that gap by reconstructing SDK
    // lifecycle state from ledgers.
    assert!(!capabilities
        .supported_operations
        .contains(&"reconcile_governance".to_string()));
    assert!(!capabilities
        .required_sdk_apis
        .contains(&"Computer::reconcile_governance".to_string()));
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
async fn marketplace_lifecycle_commands_use_sdk_errors_without_client_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;

    let error = add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "tf-market".to_string(),
            git_url: "not a git url".to_string(),
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
            git_url: format!("file://{}", repo.display()),
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
    assert_eq!(
        governance.plugins[0].bundled_mcp_servers,
        vec!["audit-mcp".to_string()]
    );
    assert!(governance.plugins[0]
        .bundled_skills
        .contains(&"audit:code-review".to_string()));

    install_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
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

    let managed = state
        .config
        .load_managed_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(managed.iter().all(|server| server.name() != "audit-mcp"));
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

    let injected = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(injected
        .iter()
        .any(|input| input.id() == "audit@acme/api_token"));

    let remove_error = remove_marketplace_core(&state, TEST_INSTANCE_ID, "acme")
        .await
        .unwrap_err();
    assert!(remove_error.contains("uninstall plugins before removing"));

    uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    let managed = state
        .config
        .load_managed_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(managed.iter().all(|server| server.name() != "audit-mcp"));
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
async fn plugin_mcp_servers_are_dynamic_and_user_servers_win_after_disable() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            git_url: format!("file://{}", repo.display()),
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

    let add_error =
        mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("audit-mcp"))
            .await
            .unwrap_err();
    assert!(add_error.contains("Marketplace plugin"));

    disable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("audit-mcp"))
        .await
        .unwrap();

    enable_plugin_core(&state, TEST_INSTANCE_ID, request.clone())
        .await
        .unwrap();
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let audit_rows: Vec<_> = servers
        .iter()
        .filter(|server| server.name == "audit-mcp")
        .collect();
    assert_eq!(audit_rows.len(), 1);
    assert!(matches!(audit_rows[0].managed_by, McpServerManagedBy::User));

    uninstall_plugin_core(&state, TEST_INSTANCE_ID, request)
        .await
        .unwrap();
    let managed = state
        .config
        .load_managed_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(managed.iter().any(|server| server.name() == "audit-mcp"));
}

#[tokio::test]
async fn computer_bootup_does_not_start_enabled_plugin_mcp_servers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_marketplace_test_app_state(tmp.path()).await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: "acme".to_string(),
            git_url: format!("file://{}", repo.display()),
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

    // SDK currently owns boot/recovery semantics. tfrobot-client verifies that
    // plugin servers stay visible in the current runtime, but it does not
    // synthesize cold-start remount behavior or start bundled MCP servers during
    // Computer bootup.
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
        !audit_after_boot.running,
        "enabled plugin MCP server should remain stopped after Computer bootup; status: {}",
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
            git_url: format!("file://{}", repo.display()),
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
    let installed_server = wait_for_mcp_server_running(TEST_INSTANCE_ID, &state, "audit-mcp")
        .await
        .expect("plugin install should start bundled MCP server when Computer is running");
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
            git_url: format!("file://{}", repo.display()),
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
            git_url: format!("file://{}", repo.display()),
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
async fn marketplace_update_replaces_url_when_no_plugins_are_installed() {
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
            git_url: format!("file://{}", first_repo.display()),
        },
    )
    .await
    .unwrap();

    update_marketplace_core(
        &state,
        TEST_INSTANCE_ID,
        UpdateMarketplaceRequest {
            name: "acme".to_string(),
            git_url: format!("file://{}", second_repo.display()),
        },
    )
    .await
    .unwrap();

    let governance = get_marketplace_governance_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(governance.marketplaces.len(), 1);
    assert_eq!(
        governance.marketplaces[0].git_url.as_deref(),
        Some(format!("file://{}", second_repo.display()).as_str())
    );
    assert_eq!(governance.plugins.len(), 1);
    assert_eq!(governance.plugins[0].plugin, "tf45-audit");
    assert_eq!(governance.plugins[0].status, "available");
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
            git_url: format!("file://{}", first_repo.display()),
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
            git_url: format!("file://{}", second_repo.display()),
        },
    )
    .await
    .unwrap_err();

    assert!(error.contains("uninstall plugins before updating"));
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
        .await;
    let repo = tmp.path().join("marketplace-repo");
    build_marketplace_repo(&repo);
    let git_url = format!("file://{}", repo.display());

    for instance_id in [TEST_INSTANCE_ID, TEST_SECOND_INSTANCE_ID] {
        add_marketplace_core(
            &state,
            instance_id,
            AddMarketplaceRequest {
                name: "acme".to_string(),
                git_url: git_url.clone(),
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
        .await;
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
        .await;
    let repo = tmp.path().join("marketplace-repo");
    build_tf45_isolation_marketplace_repo(&repo);

    add_marketplace_core(
        &state,
        FIRST_INSTANCE_ID,
        AddMarketplaceRequest {
            name: MARKETPLACE.to_string(),
            git_url: format!("file://{}", repo.display()),
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
    let server_path = echo_server_path();
    fs::write(
        servers.join("audit-mcp.json"),
        format!(
            r#"{{"type":"stdio","name":"audit-mcp","server_parameters":{{"command":"node","args":["{}"],"env":{{}}}}}}"#,
            server_path.display()
        ),
    )
    .unwrap();
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
    let server_path = echo_server_path();
    fs::write(
        servers.join("tf45-audit-mcp.json"),
        format!(
            r#"{{"type":"stdio","name":"tf45-audit-mcp","server_parameters":{{"command":"node","args":["{}"],"env":{{}}}}}}"#,
            server_path.display()
        ),
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
