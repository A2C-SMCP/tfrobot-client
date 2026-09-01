use super::{ClientControlError, ClientControlPlane, InvocationContext, ToolId};
use a2c_smcp::smcp_computer::mcp_clients::model::{
    BundleId, CallToolResult, ClientNotifyCtx, ClientState, Content, MCPClientError,
    MCPClientProtocol, ReadResourceResult, Resource, StdioServerConfig, StdioServerParameters,
    Tool,
};
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, Weak};
use uuid::Uuid;

pub const CLIENT_CONTROL_BUNDLE_ID: &str = "client_control";

/// Shared late binding that breaks the registry → runtime → provider → control-plane cycle.
/// Runtimes may be constructed before the application finishes constructing the plane.
#[derive(Clone, Default)]
pub struct ClientControlBinding {
    plane: Arc<RwLock<Weak<ClientControlPlane>>>,
}

impl ClientControlBinding {
    pub fn bind(&self, plane: &Arc<ClientControlPlane>) {
        *self
            .plane
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(plane);
    }

    fn plane(&self) -> Result<Arc<ClientControlPlane>, MCPClientError> {
        self.plane
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .upgrade()
            .ok_or_else(|| {
                MCPClientError::ConnectionError("Client Control plane is not available".to_string())
            })
    }
}

#[must_use]
pub fn client_control_server_config() -> MCPServerConfig {
    let mut config = StdioServerConfig::new(
        "Client Control",
        StdioServerParameters {
            // This config is an in-memory identity carrier. The factory intercepts its reserved
            // BundleID before any process spawn can occur.
            command: "__tfrobot_client_control_internal__".to_string(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
        },
    );
    config.bundle_id = Some(
        BundleId::try_from(CLIENT_CONTROL_BUNDLE_ID.to_string())
            .expect("reserved Client Control BundleID must remain valid"),
    );
    MCPServerConfig::Stdio(config)
}

pub struct ClientControlMcpClient {
    source_computer_id: String,
    binding: ClientControlBinding,
    /// Immutable policy captured when this provider incarnation is mounted. Call-time
    /// authorization still consults the registry-published runtime through the control plane.
    tool_policy: super::RemoteControlPolicy,
    state: RwLock<ClientState>,
}

impl ClientControlMcpClient {
    #[must_use]
    pub fn new(
        source_computer_id: impl Into<String>,
        binding: ClientControlBinding,
        tool_policy: super::RemoteControlPolicy,
        _notify: Option<ClientNotifyCtx>,
    ) -> Self {
        Self {
            source_computer_id: source_computer_id.into(),
            binding,
            tool_policy,
            state: RwLock::new(ClientState::Initialized),
        }
    }

    fn set_state(&self, state: ClientState) {
        *self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner()) = state;
    }

    fn mcp_tool(definition: super::ToolDefinition) -> Tool {
        let input_schema = serde_json::from_value(definition.input_schema())
            .expect("Client Control schemas must be JSON objects");
        Tool::new(
            definition.id.as_str().to_string(),
            format!(
                "TFRobot Client Control: {} ({:?})",
                definition.id, definition.risk
            ),
            Arc::new(input_schema),
        )
    }

    fn result(value: serde_json::Value) -> CallToolResult {
        CallToolResult::success(vec![Content::text(value.to_string())])
    }

    fn error_result(error: ClientControlError) -> CallToolResult {
        let body = serde_json::to_string(&error).unwrap_or_else(|_| {
            r#"{"code":"operation_failed","message":"Client Control failed"}"#.to_string()
        });
        CallToolResult::error(vec![Content::text(body)])
    }
}

#[async_trait::async_trait]
impl MCPClientProtocol for ClientControlMcpClient {
    fn state(&self) -> ClientState {
        *self.state.read().unwrap_or_else(|error| error.into_inner())
    }

    async fn connect(&self) -> Result<(), MCPClientError> {
        self.binding.plane()?;
        self.set_state(ClientState::Connected);
        Ok(())
    }

    async fn disconnect(&self) -> Result<(), MCPClientError> {
        self.set_state(ClientState::Disconnected);
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<Tool>, MCPClientError> {
        let plane = self.binding.plane()?;
        Ok(plane
            .catalog()
            .into_iter()
            .filter(|definition| self.tool_policy.allows_tool(definition.id))
            .map(Self::mcp_tool)
            .collect())
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<CallToolResult, MCPClientError> {
        let plane = self.binding.plane()?;
        let tool = match tool_name.parse::<ToolId>() {
            Ok(tool) => tool,
            Err(error) => {
                return Ok(CallToolResult::error(vec![Content::text(
                    serde_json::json!({"code": "unknown_tool", "message": error.to_string()})
                        .to_string(),
                )]));
            }
        };
        let context = InvocationContext {
            request_id: Uuid::new_v4().to_string(),
            source_computer_id: self.source_computer_id.clone(),
        };
        Ok(match plane.dispatch(context, tool, params).await {
            Ok(value) => Self::result(value),
            Err(error) => Self::error_result(error),
        })
    }

    async fn list_windows(&self) -> Result<Vec<Resource>, MCPClientError> {
        Ok(Vec::new())
    }

    async fn list_resources_page(
        &self,
        _cursor: Option<String>,
    ) -> Result<(Vec<Resource>, Option<String>), MCPClientError> {
        Err(MCPClientError::CapabilityNotSupported(
            "Client Control does not expose MCP resources".to_string(),
        ))
    }

    async fn get_window_detail(
        &self,
        _resource: Resource,
    ) -> Result<ReadResourceResult, MCPClientError> {
        Err(MCPClientError::CapabilityNotSupported(
            "Client Control does not expose window resources".to_string(),
        ))
    }

    async fn subscribe_window(&self, _resource: Resource) -> Result<(), MCPClientError> {
        Err(MCPClientError::CapabilityNotSupported(
            "Client Control does not expose window subscriptions".to_string(),
        ))
    }

    async fn unsubscribe_window(&self, _resource: Resource) -> Result<(), MCPClientError> {
        Err(MCPClientError::CapabilityNotSupported(
            "Client Control does not expose window subscriptions".to_string(),
        ))
    }
}

impl ClientControlMcpClient {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_config_has_stable_identity_and_never_names_a_real_executable() {
        let config = client_control_server_config();
        assert_eq!(
            config.bundle_id().unwrap().as_str(),
            CLIENT_CONTROL_BUNDLE_ID
        );
        match config {
            MCPServerConfig::Stdio(config) => assert_eq!(
                config.server_parameters.command,
                "__tfrobot_client_control_internal__"
            ),
            _ => panic!("reserved provider identity carrier must remain stdio"),
        }
    }
}
