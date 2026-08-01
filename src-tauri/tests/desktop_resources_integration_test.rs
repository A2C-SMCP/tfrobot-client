mod common;

use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use common::{create_test_app_state, mcp};
use tfrobot_client_lib::commands::desktop::{
    get_desktop_core, get_window_detail_core, DesktopEnumerationStatus,
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
        get_window_detail_core(&state, INSTANCE_ID, &windows[0].bundle_id, &windows[0].uri)
            .await
            .unwrap();

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
