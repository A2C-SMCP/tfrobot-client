mod common;

use a2c_smcp::smcp_computer::mcp_clients::{model::BundleId, MCPServerConfig};
use common::{create_test_app_state, mcp};
use tfrobot_client_lib::commands::desktop::{
    get_desktop_core, get_window_detail_core, DesktopEnumerationStatus,
};
use tfrobot_client_lib::services::built_in_tools::{
    CommandLineRuntimeAssets, COMMAND_LINE_BUNDLE_ID,
};
use tfrobot_client_lib::services::computer::ComputerInstance;

const INSTANCE_ID: &str = "desktop-computer";

fn require_node() {
    let available = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        available,
        "Node.js is required for Desktop Resources integration tests"
    );
}

fn desktop_server_config(marker_path: &std::path::Path) -> MCPServerConfig {
    let server_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/echo-mcp-server/index-desktop.js");
    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "desktop-fixture",
        "server_parameters": {
            "command": "node",
            "args": [server_path.to_str().unwrap(), marker_path.to_str().unwrap()],
            "env": {}
        }
    }))
    .unwrap()
}

#[tokio::test]
async fn list_is_metadata_only_and_detail_performs_the_first_resource_read() {
    require_node();
    let temp = tempfile::tempdir().unwrap();
    let marker_path = temp.path().join("resource-read.marker");
    let state = create_test_app_state(temp.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(INSTANCE_ID, "Desktop Computer"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance(INSTANCE_ID).unwrap())
        .await
        .unwrap();
    mcp::add_mcp_server_core(&state, INSTANCE_ID, desktop_server_config(&marker_path))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(INSTANCE_ID)
        .await
        .unwrap();

    let enumeration = get_desktop_core(&state, INSTANCE_ID, None).await.unwrap();
    assert_eq!(
        enumeration.status,
        DesktopEnumerationStatus::Unverified,
        "the pinned SDK cannot prove enumeration completeness"
    );
    let windows = enumeration.windows;

    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].uri, "window://fixture/main");
    assert_eq!(windows[0].title, "Fixture Window");
    assert_eq!(windows[0].server, "desktop-fixture");
    assert_eq!(windows[0].mime_type.as_deref(), Some("text/plain"));
    assert!(
        !marker_path.exists(),
        "resources/list must not trigger resources/read"
    );

    let detail =
        get_window_detail_core(&state, INSTANCE_ID, &windows[0].bundle_id, &windows[0].uri).await;
    let missing = get_window_detail_core(
        &state,
        INSTANCE_ID,
        &BundleId::try_from("missing-server").unwrap(),
        &windows[0].uri,
    )
    .await;
    state
        .computer_registry
        .stop_runtime(INSTANCE_ID)
        .await
        .unwrap();
    let detail = detail.unwrap();

    assert_eq!(detail.server, windows[0].server);
    assert_eq!(missing.unwrap_err(), "MCP server not found: missing-server");
    assert_eq!(detail.contents.len(), 1);
    assert_eq!(
        detail.contents[0].text.as_deref(),
        Some("Fixture window content")
    );
    assert_eq!(
        std::fs::read_to_string(marker_path).unwrap(),
        "window://fixture/main\n"
    );
}

/// Requires the packaged tfbash runtime, selected with TFROBOT_BUILT_IN_RESOURCE_ROOT.
/// Run explicitly with `cargo test --test desktop_resources_integration_test -- --include-ignored`.
#[tokio::test]
#[ignore = "requires prepared bundled tfbash runtime and TFROBOT_BUILT_IN_RESOURCE_ROOT"]
async fn built_in_shell_overview_can_be_read_while_hidden_from_the_management_inventory() {
    CommandLineRuntimeAssets::discover().expect("prepare the bundled tfbash runtime first");
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_app_state(temp.path());
    let mut instance = ComputerInstance::new(INSTANCE_ID, "Desktop Computer");
    instance.command_line.enabled = true;
    instance.command_line.workspace_root = Some(temp.path().to_path_buf());
    state
        .config
        .add_computer_instance(instance.clone())
        .unwrap();
    let runtime = state
        .computer_registry
        .upsert_runtime(instance)
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(INSTANCE_ID)
        .await
        .unwrap();

    // Use the production command core and SDK transport for both discovery and content reads.
    let result = async {
        let windows = get_desktop_core(&state, INSTANCE_ID, None).await?.windows;
        let window = windows
            .iter()
            .find(|window| {
                window.bundle_id.as_str() == COMMAND_LINE_BUNDLE_ID
                    && window.uri == "window://io.github.a2c-smcp.tfbash/shell-overview"
            })
            .ok_or_else(|| "Shell Overview was not enumerated".to_string())?;
        let detail =
            get_window_detail_core(&state, INSTANCE_ID, &window.bundle_id, &window.uri).await?;
        Ok::<_, String>((window.clone(), detail))
    }
    .await;
    let inventory = runtime.sdk_mcp_server_ownership().await;
    state
        .computer_registry
        .stop_runtime(INSTANCE_ID)
        .await
        .unwrap();

    let (window, detail) = result.unwrap();
    assert_eq!(window.server, "TFRobot command line");
    assert_eq!(detail.server, window.server);
    assert_eq!(detail.bundle_id, window.bundle_id);
    assert_eq!(detail.uri, window.uri);
    assert!(
        detail.contents.iter().any(|content| {
            content
                .text
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
        }),
        "Shell Overview must contain readable text"
    );
    assert!(inventory
        .iter()
        .all(|entry| entry.bundle_id != COMMAND_LINE_BUNDLE_ID));
}
