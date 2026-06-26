//! Contract tests for smcp-computer API.
//! These verify that tfrobot-client's assumptions about smcp-computer's public API hold.
//! When smcp-computer upgrades, these tests should fail first, providing clear guidance.

use smcp_computer::mcp_clients::model::*;
use smcp_computer::mcp_clients::MCPServerConfig;
use smcp_computer::{
    computer::{Computer, Session},
    errors::ComputerResult,
};
use std::collections::HashMap;

struct ContractSession;

#[async_trait::async_trait]
impl Session for ContractSession {
    async fn resolve_input(&self, _input: &MCPServerInput) -> ComputerResult<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }

    fn session_id(&self) -> &str {
        "contract-session"
    }
}

// ── API Existence Contracts ──
// If these fail to compile, the smcp-computer API has changed.

#[test]
fn contract_computer_constructible() {
    let _computer = Computer::new(
        "contract-computer",
        ContractSession,
        Some(HashMap::new()),
        Some(HashMap::new()),
        false,
        true,
    );
}

#[tokio::test]
async fn contract_computer_mcp_api_surface() {
    let computer = Computer::new(
        "contract-computer",
        ContractSession,
        Some(HashMap::new()),
        Some(HashMap::new()),
        false,
        true,
    );

    // These calls verify the API exists with expected signatures.
    // We don't assert behavior, just compilation.
    let _statuses = computer.get_server_status().await;
    let _tools = computer.get_available_tools().await;
    let _ = computer.start_mcp_client("all").await;
    let _ = computer.stop_mcp_client("all").await;
    let _ = computer.shutdown().await;
}

#[test]
fn contract_version_exists() {
    let version = smcp_computer::VERSION;
    assert!(!version.is_empty());
    assert!(
        version.contains('.'),
        "VERSION should be semver format: {version}"
    );
}

// ── Serialization Format Contracts ──

#[test]
fn contract_stdio_config_format() {
    let json = serde_json::json!({
        "type": "Stdio",
        "name": "test",
        "server_parameters": {
            "command": "node",
            "args": ["server.js"],
            "env": {}
        }
    });
    let config: MCPServerConfig =
        serde_json::from_value(json).expect("Stdio config should deserialize");
    assert_eq!(config.name(), "test");
    assert!(matches!(config, MCPServerConfig::Stdio(_)));
}

#[test]
fn contract_http_config_format() {
    let json = serde_json::json!({
        "type": "Http",
        "name": "http-test",
        "server_parameters": {
            "url": "http://localhost:8080",
            "headers": {}
        }
    });
    let config: MCPServerConfig =
        serde_json::from_value(json).expect("Http config should deserialize");
    assert_eq!(config.name(), "http-test");
    assert!(matches!(config, MCPServerConfig::Http(_)));
}

#[test]
fn contract_sse_config_format() {
    let json = serde_json::json!({
        "type": "Sse",
        "name": "sse-test",
        "server_parameters": {
            "url": "http://localhost:8081/sse",
            "headers": {}
        }
    });
    let config: MCPServerConfig =
        serde_json::from_value(json).expect("Sse config should deserialize");
    assert_eq!(config.name(), "sse-test");
    assert!(matches!(config, MCPServerConfig::Sse(_)));
}

#[test]
fn contract_lowercase_type_alias() {
    // smcp-computer should accept lowercase type aliases
    let json = serde_json::json!({
        "type": "stdio",
        "name": "lowercase-test",
        "server_parameters": {
            "command": "node",
            "args": [],
            "env": {}
        }
    });
    let result: Result<MCPServerConfig, _> = serde_json::from_value(json);
    assert!(result.is_ok(), "Should accept lowercase 'stdio' type alias");
}

#[test]
fn contract_config_has_name_method() {
    let json = serde_json::json!({
        "type": "Stdio",
        "name": "name-test",
        "server_parameters": {
            "command": "node",
            "args": [],
            "env": {}
        }
    });
    let config: MCPServerConfig = serde_json::from_value(json).unwrap();
    // .name() method should exist and return the server name
    assert_eq!(config.name(), "name-test");
}

// ── Config Field Contracts ──

#[test]
fn contract_stdio_config_fields() {
    let json = serde_json::json!({
        "type": "Stdio",
        "name": "full-fields",
        "disabled": true,
        "forbidden_tools": ["dangerous"],
        "tool_meta": {},
        "default_tool_meta": null,
        "vrl": null,
        "server_parameters": {
            "command": "python",
            "args": ["-m", "server"],
            "env": {"KEY": "value"},
            "cwd": "/app"
        }
    });
    let config: MCPServerConfig = serde_json::from_value(json).unwrap();
    match config {
        MCPServerConfig::Stdio(c) => {
            assert!(c.disabled);
            assert_eq!(c.forbidden_tools, vec!["dangerous"]);
            assert_eq!(c.server_parameters.command, "python");
            assert_eq!(c.server_parameters.args, vec!["-m", "server"]);
            assert_eq!(c.server_parameters.cwd.as_deref(), Some("/app"));
        }
        _ => panic!("Expected Stdio variant"),
    }
}

#[test]
fn contract_tool_meta_structure() {
    // ToolMeta should have auto_apply, alias, tags, ret_object_mapper fields
    let meta = ToolMeta {
        auto_apply: Some(true),
        alias: Some("my-alias".to_string()),
        tags: Some(vec!["tag1".to_string()]),
        ret_object_mapper: Some(std::collections::HashMap::new()),
    };
    assert_eq!(meta.auto_apply, Some(true));
    assert_eq!(meta.alias.as_deref(), Some("my-alias"));
}

// ── Serde Roundtrip Contracts ──

#[test]
fn contract_config_serde_roundtrip() {
    let json = serde_json::json!({
        "type": "Stdio",
        "name": "roundtrip",
        "server_parameters": {
            "command": "node",
            "args": ["server.js"],
            "env": {}
        }
    });
    let config: MCPServerConfig = serde_json::from_value(json).unwrap();
    let serialized = serde_json::to_value(&config).unwrap();
    let deserialized: MCPServerConfig = serde_json::from_value(serialized).unwrap();
    assert_eq!(config, deserialized);
}

// ── MCPServerInput Contract ──

#[test]
fn contract_mcp_server_input_variants() {
    // Verify MCPServerInput enum has expected variants
    let prompt = MCPServerInput::PromptString(PromptStringInput {
        id: "test".to_string(),
        description: "Test".to_string(),
        default: None,
        password: None,
    });
    assert_eq!(prompt.id(), "test");

    let pick = MCPServerInput::PickString(PickStringInput {
        id: "pick".to_string(),
        description: "Pick".to_string(),
        options: vec![],
        default: None,
    });
    assert_eq!(pick.id(), "pick");

    let cmd = MCPServerInput::Command(CommandInput {
        id: "cmd".to_string(),
        description: "Cmd".to_string(),
        command: "echo".to_string(),
        args: None,
    });
    assert_eq!(cmd.id(), "cmd");
}

// ── Error Type Contract ──

#[test]
fn contract_computer_error_variants_exist() {
    use smcp_computer::errors::ComputerError;

    // Verify key variants that tfrobot-client depends on can be constructed.
    // If smcp-computer removes/renames these, this test will fail at compile time.
    let e1 = ComputerError::InvalidConfiguration("test".into());
    assert!(e1.to_string().contains("test"));

    let e2 = ComputerError::RuntimeError("runtime".into());
    assert!(e2.to_string().contains("runtime"));

    let e3 = ComputerError::ServerNotActive {
        server_name: "srv".into(),
    };
    assert!(e3.to_string().contains("srv"));

    let e4 = ComputerError::TransportError("transport".into());
    assert!(e4.to_string().contains("transport"));

    // Verify error_code() method exists and returns expected codes
    assert_eq!(e1.error_code(), 400);
    assert_eq!(e2.error_code(), 500);
}

#[test]
fn contract_computer_error_implements_std_error() {
    use smcp_computer::errors::ComputerError;
    let e = ComputerError::InvalidConfiguration("test".into());
    // Must implement std::error::Error (Display + Debug)
    let _display = format!("{e}");
    let _debug = format!("{e:?}");
}
