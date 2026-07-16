//! Real marketplace end-to-end coverage for TFRC-43.
//!
//! This test intentionally depends on the public CNB marketplace repository and
//! is opt-in for regular test runs. Set `TFROBOT_RUN_REAL_MARKETPLACE_E2E=1` to
//! exercise the real marketplace integration path.

mod common;

use common::{create_test_app_state, echo_server_config};
use tfrobot_client_lib::commands::{
    computer::start_computer_instance_core,
    marketplace::{
        add_marketplace_core, disable_plugin_core, enable_plugin_core,
        get_marketplace_governance_core, install_plugin_core, refresh_marketplace_core,
        remove_marketplace_core, uninstall_plugin_core, AddMarketplaceRequest,
        PluginLifecycleRequest,
    },
    mcp, skills,
};
use tfrobot_client_lib::services::computer::{ComputerInstance, McpServerManagedBy};
use tfrobot_client_lib::AppState;

const INSTANCE_ID: &str = "marketplace-real-e2e";
const MARKETPLACE_NAME: &str = "turingfocus-skills";
const MARKETPLACE_URL: &str = "https://cnb.cool/turingfocus/turingfocus-skills-a2c.git";
const PLUGIN_NAME: &str = "turingfocus-toolkit";
const PLUGIN_ID: &str = "turingfocus-toolkit@turingfocus-skills";
const MCP_SERVER_NAME: &str = "everything";
const SAMPLE_SKILLS: &[&str] = &[
    "turingfocus-toolkit:add-feature",
    "turingfocus-toolkit:code-review",
    "turingfocus-toolkit:troubleshoot",
];

#[tokio::test]
async fn real_cnb_marketplace_plugin_skills_and_mcp_lifecycle_work() {
    if std::env::var("TFROBOT_RUN_REAL_MARKETPLACE_E2E").as_deref() != Ok("1") {
        eprintln!(
            "skipping real CNB marketplace E2E; set TFROBOT_RUN_REAL_MARKETPLACE_E2E=1 to run"
        );
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let xdg_config_home = tmp.path().join("xdg-config-home");
    std::fs::create_dir_all(&xdg_config_home).unwrap();
    std::env::set_var("XDG_CONFIG_HOME", &xdg_config_home);

    let state = create_marketplace_real_test_app_state(tmp.path()).await;
    let request = PluginLifecycleRequest {
        marketplace: MARKETPLACE_NAME.to_string(),
        plugin: PLUGIN_NAME.to_string(),
    };

    add_marketplace_core(
        &state,
        INSTANCE_ID,
        AddMarketplaceRequest {
            name: MARKETPLACE_NAME.to_string(),
            git_url: MARKETPLACE_URL.to_string(),
        },
    )
    .await
    .unwrap();
    refresh_marketplace_core(&state, INSTANCE_ID, MARKETPLACE_NAME)
        .await
        .unwrap();

    let governance = get_marketplace_governance_core(&state, INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance
        .marketplaces
        .iter()
        .any(|marketplace| marketplace.name == MARKETPLACE_NAME));

    install_plugin_core(&state, INSTANCE_ID, request.clone())
        .await
        .unwrap();
    assert_plugin_enabled_with_expected_assets(&state).await;
    assert_sample_skill_content_readable(&state).await;
    assert_plugin_owned_mcp_server_visible_and_user_lifecycle_blocked(&state).await;
    assert_bootup_does_not_start_plugin_mcp_server(&state).await;

    disable_plugin_core(&state, INSTANCE_ID, request.clone())
        .await
        .unwrap();
    assert_plugin_disabled_without_active_assets(&state).await;

    enable_plugin_core(&state, INSTANCE_ID, request.clone())
        .await
        .unwrap();
    assert_plugin_enabled_with_expected_assets(&state).await;
    assert_sample_skill_content_readable(&state).await;
    assert_plugin_owned_mcp_server_visible_and_user_lifecycle_blocked(&state).await;

    uninstall_plugin_core(&state, INSTANCE_ID, request)
        .await
        .unwrap();
    assert_plugin_uninstalled(&state).await;

    remove_marketplace_core(&state, INSTANCE_ID, MARKETPLACE_NAME)
        .await
        .unwrap();
    let governance = get_marketplace_governance_core(&state, INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance.marketplaces.is_empty());
    assert!(governance.plugins.is_empty());
}

async fn create_marketplace_real_test_app_state(path: &std::path::Path) -> AppState {
    let state = create_test_app_state(path);
    state
        .config
        .add_computer_instance(ComputerInstance::new(INSTANCE_ID, "Marketplace Real E2E"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance(INSTANCE_ID).unwrap())
        .await;
    state
}

async fn assert_plugin_enabled_with_expected_assets(state: &AppState) {
    let governance = get_marketplace_governance_core(state, INSTANCE_ID)
        .await
        .unwrap();
    let plugin = governance
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id.as_deref() == Some(PLUGIN_ID))
        .expect("installed plugin should be visible in governance");
    assert!(plugin.enabled);
    assert_eq!(plugin.status, "enabled");
    assert!(plugin
        .bundled_mcp_servers
        .contains(&MCP_SERVER_NAME.to_string()));
    for skill in SAMPLE_SKILLS {
        assert!(
            plugin.bundled_skills.contains(&(*skill).to_string()),
            "expected bundled skill {skill:?}, got {:?}",
            plugin.bundled_skills
        );
    }

    let active_skills = skills::list_skills_core(state, INSTANCE_ID).await.unwrap();
    for skill in SAMPLE_SKILLS {
        assert!(
            active_skills.iter().any(|active| active.name == *skill),
            "expected active skill {skill:?}"
        );
    }
}

async fn assert_sample_skill_content_readable(state: &AppState) {
    let skill = skills::get_skill_core(state, INSTANCE_ID, SAMPLE_SKILLS[0], None)
        .await
        .unwrap();
    let body = skill.body.expect("main SKILL.md should be inline text");
    assert!(body.contains("# Add Feature"));
    assert!(body.contains("Feature Request"));

    let resource = skills::get_skill_core(
        state,
        INSTANCE_ID,
        SAMPLE_SKILLS[0],
        Some("resources/tfrobotv2.md"),
    )
    .await
    .unwrap();
    let body = resource.body.expect("resource should be inline text");
    assert!(body.contains("TFRobotV2"));
}

async fn assert_plugin_owned_mcp_server_visible_and_user_lifecycle_blocked(state: &AppState) {
    let servers = mcp::get_mcp_servers_core(state, INSTANCE_ID).await.unwrap();
    let server = servers
        .iter()
        .find(|server| server.name == MCP_SERVER_NAME)
        .expect("bundled MCP server should be visible");
    match &server.managed_by {
        McpServerManagedBy::Plugin {
            marketplace,
            plugin,
            plugin_id,
        } => {
            assert_eq!(marketplace, MARKETPLACE_NAME);
            assert_eq!(plugin, PLUGIN_NAME);
            assert_eq!(plugin_id.as_deref(), Some(PLUGIN_ID));
        }
        other => panic!("expected plugin-owned server, got {other:?}"),
    }

    let update_err =
        mcp::update_mcp_server_core(state, INSTANCE_ID, echo_server_config(MCP_SERVER_NAME))
            .await
            .unwrap_err();
    let remove_err = mcp::remove_mcp_server_core(state, INSTANCE_ID, MCP_SERVER_NAME)
        .await
        .unwrap_err();
    let start_err = mcp::start_mcp_server_core(state, INSTANCE_ID, MCP_SERVER_NAME)
        .await
        .unwrap_err();
    let stop_err = mcp::stop_mcp_server_core(state, INSTANCE_ID, MCP_SERVER_NAME)
        .await
        .unwrap_err();

    for error in [
        update_err.to_string(),
        remove_err,
        start_err.to_string(),
        stop_err,
    ] {
        assert!(
            error.contains("Marketplace plugin"),
            "expected plugin lifecycle guard, got: {error}"
        );
    }
}

async fn assert_bootup_does_not_start_plugin_mcp_server(state: &AppState) {
    start_computer_instance_core(None, state, INSTANCE_ID.to_string())
        .await
        .unwrap();

    let server = wait_for_mcp_server_running(state, MCP_SERVER_NAME)
        .await
        .expect("bundled MCP server should be visible after bootup");
    assert!(
        !server.running,
        "Computer bootup should not auto-start plugin MCP server; status: {}",
        server.status_message
    );
}

async fn wait_for_mcp_server_running(state: &AppState, name: &str) -> Option<mcp::McpServerStatus> {
    for _ in 0..120 {
        let servers = mcp::get_mcp_servers_core(state, INSTANCE_ID).await.ok()?;
        let server = servers.into_iter().find(|server| server.name == name)?;
        if server.running {
            return Some(server);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    mcp::get_mcp_servers_core(state, INSTANCE_ID)
        .await
        .ok()?
        .into_iter()
        .find(|server| server.name == name)
}

async fn assert_plugin_disabled_without_active_assets(state: &AppState) {
    let governance = get_marketplace_governance_core(state, INSTANCE_ID)
        .await
        .unwrap();
    let plugin = governance
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id.as_deref() == Some(PLUGIN_ID))
        .expect("disabled plugin should remain visible in governance");
    assert!(!plugin.enabled);
    assert_eq!(plugin.status, "disabled");

    let servers = mcp::get_mcp_servers_core(state, INSTANCE_ID).await.unwrap();
    assert!(servers.iter().all(|server| server.name != MCP_SERVER_NAME));

    let active_skills = skills::list_skills_core(state, INSTANCE_ID).await.unwrap();
    for skill in SAMPLE_SKILLS {
        assert!(
            active_skills.iter().all(|active| active.name != *skill),
            "disabled plugin skill should not be active: {skill}"
        );
    }
    let error = skills::get_skill_core(state, INSTANCE_ID, SAMPLE_SKILLS[0], None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        skills::SkillCommandError::SkillNotFound { .. }
    ));
}

async fn assert_plugin_uninstalled(state: &AppState) {
    let governance = get_marketplace_governance_core(state, INSTANCE_ID)
        .await
        .unwrap();
    assert!(governance
        .plugins
        .iter()
        .all(|plugin| plugin.plugin_id.as_deref() != Some(PLUGIN_ID)));

    let servers = mcp::get_mcp_servers_core(state, INSTANCE_ID).await.unwrap();
    assert!(servers.iter().all(|server| server.name != MCP_SERVER_NAME));

    let active_skills = skills::list_skills_core(state, INSTANCE_ID).await.unwrap();
    for skill in SAMPLE_SKILLS {
        assert!(active_skills.iter().all(|active| active.name != *skill));
    }
}
