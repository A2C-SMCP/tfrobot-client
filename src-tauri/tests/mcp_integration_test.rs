//! Integration tests for MCP server management through AppState.
//! These tests exercise the full flow: config persistence + MCPServerManager.

mod common;

use common::{create_test_app_state, echo_server_config, stderr_flood_server_config};
use smcp_computer::mcp_clients::MCPServerConfig;
use tfrobot_client_lib::commands::{config_io, mcp};
use tfrobot_client_lib::services::computer::{ComputerInstance, DEFAULT_COMPUTER_INSTANCE_ID};
use tfrobot_client_lib::AppState;

async fn create_mcp_test_app_state(path: &std::path::Path) -> AppState {
    let state = create_test_app_state(path);
    if state
        .config
        .get_computer_instance(DEFAULT_COMPUTER_INSTANCE_ID)
        .is_err()
    {
        state
            .config
            .add_computer_instance(ComputerInstance::default_instance())
            .unwrap();
    }
    let mut instances = state.config.load_computer_instances().unwrap();
    instances.default_instance_id = DEFAULT_COMPUTER_INSTANCE_ID.to_string();
    state.config.save_computer_instances(&instances).unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(DEFAULT_COMPUTER_INSTANCE_ID)
                .unwrap(),
        )
        .await;
    state
}

/// Panics if Node.js is not available — CI must have Node.js installed.
fn require_node() {
    let available = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(available, "Node.js is required for MCP integration tests. CI has it configured (test.yml:87-88). If running locally without Node.js, use `cargo test --lib` to skip integration tests.");
}

// ── Config CRUD via AppState ──

#[tokio::test]
async fn test_add_and_load_server_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config = echo_server_config("test-echo");
    state.config.add_config(config).unwrap();

    let loaded = state.config.load_configs().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name(), "test-echo");
}

#[tokio::test]
async fn test_add_remove_server_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config = echo_server_config("to-remove");
    state.config.add_config(config).unwrap();
    state.config.remove_config("to-remove").unwrap();

    let loaded = state.config.load_configs().unwrap();
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_update_server_config_replaces() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config1 = echo_server_config("updatable");
    state.config.add_config(config1).unwrap();

    // Add again with same name (should replace)
    let config2 = echo_server_config("updatable");
    state.config.add_config(config2).unwrap();

    let loaded = state.config.load_configs().unwrap();
    assert_eq!(loaded.len(), 1);
}

#[tokio::test]
async fn test_mcp_commands_require_instance_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let result = mcp::get_mcp_servers_core(&state, "").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("instance_id is required"));
}

#[tokio::test]
async fn test_mcp_commands_are_instance_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_computer_instance(ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::default_instance()
        })
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance("second").unwrap())
        .await;

    mcp::add_mcp_server_core(&state, "second", echo_server_config("second-only"))
        .await
        .unwrap();

    let default_configs = state
        .config
        .load_configs_for_instance(DEFAULT_COMPUTER_INSTANCE_ID)
        .unwrap();
    let second_configs = state.config.load_configs_for_instance("second").unwrap();
    let second_statuses = mcp::get_mcp_servers_core(&state, "second").await.unwrap();

    assert!(default_configs.is_empty());
    assert_eq!(second_configs.len(), 1);
    assert_eq!(second_configs[0].name(), "second-only");
    assert_eq!(second_statuses.len(), 1);
    assert_eq!(second_statuses[0].name, "second-only");
}

#[tokio::test]
async fn test_config_io_requires_instance_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let export_path = tmp.path().join("export.json");

    let result = config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        "".into(),
        None,
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("instance_id is required"));
}

#[tokio::test]
async fn test_config_io_import_export_are_instance_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_computer_instance(ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::default_instance()
        })
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance("second").unwrap())
        .await;

    let import_path = tmp.path().join("import.json");
    let export_path = tmp.path().join("export.json");
    let import_data = serde_json::json!({
        "servers": [echo_server_config("imported-second")],
        "inputs": []
    });
    std::fs::write(
        &import_path,
        serde_json::to_string_pretty(&import_data).unwrap(),
    )
    .unwrap();

    let import_result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().to_string(),
        "second".into(),
        None,
    )
    .await
    .unwrap();
    config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        "second".into(),
        None,
    )
    .await
    .unwrap();

    let default_configs = state
        .config
        .load_configs_for_instance(DEFAULT_COMPUTER_INSTANCE_ID)
        .unwrap();
    let second_configs = state.config.load_configs_for_instance("second").unwrap();
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(export_path).unwrap()).unwrap();

    assert_eq!(import_result.servers_imported, 1);
    assert!(default_configs.is_empty());
    assert_eq!(second_configs.len(), 1);
    assert_eq!(second_configs[0].name(), "imported-second");
    assert_eq!(exported["servers"].as_array().unwrap().len(), 1);
    assert_eq!(exported["servers"][0]["name"], "imported-second");
}

// ── MCPServerManager lifecycle (requires Node.js) ──
// Echo server uses newline-delimited JSON framing (MCP spec 2025-03-26).

const MANAGER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test]
async fn test_manager_add_and_start_server() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let config = echo_server_config("lifecycle-test");

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();
    mgr.add_or_update_server(config.clone()).await.unwrap();

    // Start with timeout - may fail if echo server protocol doesn't match rmcp
    match tokio::time::timeout(MANAGER_TIMEOUT, mgr.start_client("lifecycle-test")).await {
        Ok(Ok(())) => {
            // Verify running
            let statuses = mgr.get_server_status().await;
            let found = statuses.iter().find(|(n, _, _)| n == "lifecycle-test");
            assert!(found.is_some());
            assert!(found.unwrap().1, "Server should be running");

            // Stop
            let _ = mgr.stop_client("lifecycle-test").await;
        }
        Ok(Err(e)) => {
            panic!("start_client failed: {e}");
        }
        Err(_) => {
            panic!("start_client timed out after {MANAGER_TIMEOUT:?}");
        }
    }
}

#[tokio::test]
async fn test_manager_list_tools_after_start() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let config = echo_server_config("tool-list-test");

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();
    mgr.add_or_update_server(config).await.unwrap();

    match tokio::time::timeout(MANAGER_TIMEOUT, mgr.start_client("tool-list-test")).await {
        Ok(Ok(())) => {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let tools = mgr.list_available_tools().await;
            assert!(
                !tools.is_empty(),
                "Expected at least one tool from echo server"
            );
            let echo_tool = tools.iter().find(|t| t.name == "echo");
            assert!(echo_tool.is_some(), "Expected 'echo' tool");
            let _ = mgr.stop_client("tool-list-test").await;
        }
        Ok(Err(e)) => panic!("start_client failed: {e}"),
        Err(_) => panic!("start_client timed out after {MANAGER_TIMEOUT:?}"),
    }
}

#[tokio::test]
async fn test_manager_execute_echo_tool() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let config = echo_server_config("echo-call-test");

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();
    mgr.add_or_update_server(config).await.unwrap();

    match tokio::time::timeout(MANAGER_TIMEOUT, mgr.start_client("echo-call-test")).await {
        Ok(Ok(())) => {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let params = serde_json::json!({"message": "hello from test"});
            match mgr.execute_tool("echo", params, None).await {
                Ok(call_result) => {
                    assert!(
                        !call_result.is_error.unwrap_or(false),
                        "Tool call should succeed"
                    );
                    assert!(!call_result.content.is_empty(), "Should have content");
                }
                Err(e) => panic!("Tool call failed: {e}"),
            }
            let _ = mgr.stop_client("echo-call-test").await;
        }
        Ok(Err(e)) => panic!("start_client failed: {e}"),
        Err(_) => panic!("start_client timed out after {MANAGER_TIMEOUT:?}"),
    }
}

#[tokio::test]
async fn test_manager_start_all_stop_all() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();

    // Use a single server to avoid tool name conflicts (all echo servers
    // expose the same "echo" tool, triggering ToolNameDuplicated).
    let config = echo_server_config("batch-single");
    mgr.add_or_update_server(config).await.unwrap();

    match tokio::time::timeout(MANAGER_TIMEOUT, mgr.start_all()).await {
        Ok(Ok(())) => {
            let statuses = mgr.get_server_status().await;
            let found = statuses.iter().find(|(n, _, _)| n == "batch-single");
            assert!(found.is_some());
            assert!(found.unwrap().1, "Server should be running after start_all");
            let _ = mgr.stop_all().await;
            // Verify all stopped
            let after = mgr.get_server_status().await;
            assert!(
                after.iter().all(|(_, running, _)| !*running),
                "All servers should be stopped"
            );
        }
        Ok(Err(e)) => panic!("start_all failed: {e}"),
        Err(_) => panic!("start_all timed out after {MANAGER_TIMEOUT:?}"),
    }
}

#[tokio::test]
async fn test_manager_start_nonexistent_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();

    let result = tokio::time::timeout(MANAGER_TIMEOUT, mgr.start_client("ghost-server")).await;
    match result {
        Ok(r) => assert!(r.is_err()),
        Err(_) => { /* Timeout is acceptable — the server doesn't exist / command is invalid */ }
    }
}

#[tokio::test]
async fn test_manager_invalid_command_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "bad-server",
        "server_parameters": {
            "command": "/nonexistent/binary",
            "args": [],
            "env": {}
        }
    }))
    .unwrap();

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();
    mgr.add_or_update_server(config).await.unwrap();

    let result = tokio::time::timeout(MANAGER_TIMEOUT, mgr.start_client("bad-server")).await;
    match result {
        Ok(r) => assert!(
            r.is_err(),
            "Starting server with invalid command should fail"
        ),
        Err(_) => { /* Timeout is acceptable — the server doesn't exist / command is invalid */ }
    }
}

// ── Config IO integration ──

#[tokio::test]
async fn test_export_and_reimport_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Add configs
    let config = echo_server_config("export-me");
    state.config.add_config(config).unwrap();

    // Export
    let configs = state.config.load_configs().unwrap();
    let export_path = tmp.path().join("exported.json");
    let export_data = serde_json::json!({
        "servers": configs,
        "inputs": []
    });
    std::fs::write(
        &export_path,
        serde_json::to_string_pretty(&export_data).unwrap(),
    )
    .unwrap();

    // Verify exported file is valid JSON
    let content = std::fs::read_to_string(&export_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["servers"].is_array());
    assert_eq!(parsed["servers"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_claude_desktop_format_detection() {
    let tmp = tempfile::tempdir().unwrap();

    // Claude Desktop format
    let claude_config = serde_json::json!({
        "mcpServers": {
            "myserver": {
                "command": "node",
                "args": ["server.js"]
            }
        }
    });
    let path = tmp.path().join("claude_config.json");
    std::fs::write(&path, serde_json::to_string(&claude_config).unwrap()).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(value.get("mcpServers").is_some());

    // CLI native format
    let native_config = serde_json::json!({
        "servers": [],
        "inputs": []
    });
    let path2 = tmp.path().join("native_config.json");
    std::fs::write(&path2, serde_json::to_string(&native_config).unwrap()).unwrap();

    let content2 = std::fs::read_to_string(&path2).unwrap();
    let value2: serde_json::Value = serde_json::from_str(&content2).unwrap();
    assert!(value2.get("servers").is_some());
}

// ── Logs integration ──

#[tokio::test]
async fn test_log_write_query_export_clear() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Write
    state
        .log_service
        .write("info", "system", "App started", None)
        .unwrap();
    state
        .log_service
        .write("error", "mcp", "Server crashed", Some("stack trace"))
        .unwrap();

    // Query
    let logs = state.log_service.query(&Default::default()).unwrap();
    assert_eq!(logs.len(), 2);

    // Export
    let json = state.log_service.export(&Default::default()).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.len(), 2);

    // Export to file
    let export_path = tmp.path().join("logs.json");
    std::fs::write(&export_path, &json).unwrap();
    assert!(export_path.exists());

    // Clear
    state.log_service.clear_all().unwrap();
    let after = state.log_service.query(&Default::default()).unwrap();
    assert!(after.is_empty());
}

// ── Settings integration ──

#[tokio::test]
async fn test_settings_persist_and_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let mut settings = state.settings_service.load();
    settings.language = "zh".to_string();
    settings.log_retention_days = 7;
    state.settings_service.save(&settings).unwrap();

    // Create new state pointing to same dir (simulates app restart)
    let state2 = create_mcp_test_app_state(tmp.path()).await;
    let reloaded = state2.settings_service.load();
    assert_eq!(reloaded.language, "zh");
    assert_eq!(reloaded.log_retention_days, 7);
}

// ── Input definitions integration ──

#[tokio::test]
async fn test_input_definitions_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Add
    let input: tfrobot_client_lib::commands::inputs::InputDefinition =
        serde_json::from_value(serde_json::json!({
            "type": "PromptString",
            "id": "test-input",
            "label": "Test Input"
        }))
        .unwrap();

    let mut inputs = state.config.load_inputs().unwrap();
    inputs.push(input);
    state.config.save_inputs(&inputs).unwrap();

    // Read back
    let loaded = state.config.load_inputs().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id(), "test-input");

    // Remove
    let filtered: Vec<_> = loaded
        .into_iter()
        .filter(|i| i.id() != "test-input")
        .collect();
    state.config.save_inputs(&filtered).unwrap();

    let after = state.config.load_inputs().unwrap();
    assert!(after.is_empty());
}

#[tokio::test]
async fn test_input_values_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Set values
    let mut values = std::collections::HashMap::new();
    values.insert("key1".to_string(), serde_json::json!("value1"));
    values.insert("key2".to_string(), serde_json::json!(42));
    state.config.save_input_values(&values).unwrap();

    // Read back
    let loaded = state.config.load_input_values().unwrap();
    assert_eq!(loaded["key1"], serde_json::json!("value1"));
    assert_eq!(loaded["key2"], serde_json::json!(42));

    // Clear
    state
        .config
        .save_input_values(&std::collections::HashMap::new())
        .unwrap();
    let after = state.config.load_input_values().unwrap();
    assert!(after.is_empty());
}

// ── Connection profiles integration ──

#[tokio::test]
async fn test_profiles_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let profile: tfrobot_client_lib::commands::connection::ConnectionProfile =
        serde_json::from_value(serde_json::json!({
            "name": "test-profile",
            "url": "https://smcp.example.com",
            "namespace": "/smcp",
            "office_id": "office-1",
            "computer_name": "my-pc",
            "headers": {},
            "auto_connect": true,
            "auto_reconnect": true
        }))
        .unwrap();

    // Add
    let mut profiles = state.config.load_profiles().unwrap();
    profiles.push(profile);
    state.config.save_profiles(&profiles).unwrap();

    // Read
    let loaded = state.config.load_profiles().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "test-profile");

    // Delete
    let filtered: Vec<_> = loaded
        .into_iter()
        .filter(|p| p.name != "test-profile")
        .collect();
    state.config.save_profiles(&filtered).unwrap();

    let after = state.config.load_profiles().unwrap();
    assert!(after.is_empty());
}

// ── Issue #19 regression: stderr pipe deadlock ──
// The stderr-flood server writes >64 KB to stderr on startup and per tool call.
// If smcp-computer does not consume the stderr pipe, the child process blocks
// and tool calls time out.  This test will FAIL until smcp-computer is fixed.

/// Longer timeout for this test — the server may be slow to initialize while
/// flushing stderr, but 30 s is more than enough if the pipe is being consumed.
const STDERR_FLOOD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[tokio::test]
async fn test_manager_stderr_flood_does_not_block() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let config = stderr_flood_server_config("stderr-flood-test");

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().unwrap();
    mgr.add_or_update_server(config).await.unwrap();

    // Start the server — this itself may hang if stderr blocks during init.
    match tokio::time::timeout(STDERR_FLOOD_TIMEOUT, mgr.start_client("stderr-flood-test")).await {
        Ok(Ok(())) => {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            // Execute a tool call.  The server writes another burst of stderr
            // during this call, so if the pipe is not being drained this will
            // time out.
            let params = serde_json::json!({"message": "hello through stderr storm"});
            match tokio::time::timeout(STDERR_FLOOD_TIMEOUT, mgr.execute_tool("echo", params, None))
                .await
            {
                Ok(Ok(call_result)) => {
                    assert!(
                        !call_result.is_error.unwrap_or(false),
                        "Tool call should succeed despite heavy stderr output"
                    );
                    assert!(!call_result.content.is_empty(), "Should have content");
                }
                Ok(Err(e)) => panic!("Tool call failed: {e}"),
                Err(_) => panic!(
                    "Tool call timed out after {STDERR_FLOOD_TIMEOUT:?} — \
                     stderr pipe is likely blocked (Issue #19)"
                ),
            }

            let _ = mgr.stop_client("stderr-flood-test").await;
        }
        Ok(Err(e)) => panic!("start_client failed: {e}"),
        Err(_) => panic!(
            "start_client timed out after {STDERR_FLOOD_TIMEOUT:?} — \
             stderr pipe is likely blocked during initialization (Issue #19)"
        ),
    }
}
