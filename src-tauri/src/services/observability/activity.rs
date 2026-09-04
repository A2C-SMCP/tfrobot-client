use serde::{Deserialize, Serialize};
use std::future::Future;

/// Stable Computer-level activity domains. Client-level domains remain open strings because they
/// describe application concerns outside the Computer contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerActivityCategory {
    Computer,
    Runtime,
    Connection,
    Mcp,
    Input,
    Tool,
    Resource,
    Skill,
    Marketplace,
}

impl ComputerActivityCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Computer => "computer",
            Self::Runtime => "runtime",
            Self::Connection => "connection",
            Self::Mcp => "mcp",
            Self::Input => "input",
            Self::Tool => "tool",
            Self::Resource => "resource",
            Self::Skill => "skill",
            Self::Marketplace => "marketplace",
        }
    }
}

impl From<ComputerActivityCategory> for String {
    fn from(value: ComputerActivityCategory) -> Self {
        value.as_str().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTrigger {
    User,
    Command,
    ClientControl,
    Policy,
    Runtime,
    Server,
    System,
}

impl ActivityTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Command => "command",
            Self::ClientControl => "client_control",
            Self::Policy => "policy",
            Self::Runtime => "runtime",
            Self::Server => "server",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityManagedBy {
    User,
    Plugin,
    BuiltIn,
    System,
}

impl ActivityManagedBy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Plugin => "plugin",
            Self::BuiltIn => "built_in",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityProvider {
    UserMcp,
    PluginMcp,
    BuiltInMcp,
    Smcp,
    Client,
}

impl ActivityProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserMcp => "user_mcp",
            Self::PluginMcp => "plugin_mcp",
            Self::BuiltInMcp => "built_in_mcp",
            Self::Smcp => "smcp",
            Self::Client => "client",
        }
    }
}

/// Invocation metadata inherited by domain activity emitted while one Client Control tool is
/// executing. This lets the source tool call and target-side domain mutation share one causal ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityInvocationContext {
    pub correlation_id: String,
    pub trigger: ActivityTrigger,
    pub provider: ActivityProvider,
    pub managed_by: ActivityManagedBy,
    pub source_computer_id: String,
    pub target_computer_id: Option<String>,
}

tokio::task_local! {
    static ACTIVITY_INVOCATION_CONTEXT: ActivityInvocationContext;
}

pub async fn with_activity_invocation_context<T>(
    context: ActivityInvocationContext,
    future: impl Future<Output = T>,
) -> T {
    ACTIVITY_INVOCATION_CONTEXT.scope(context, future).await
}

/// Captures the current invocation metadata before work crosses a spawned-task boundary.
/// Tokio task-local values are intentionally not inherited by `tokio::spawn`.
pub fn current_activity_invocation_context() -> Option<ActivityInvocationContext> {
    ACTIVITY_INVOCATION_CONTEXT.try_with(Clone::clone).ok()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl ActivityLevel {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub(crate) fn from_db(value: String) -> Self {
        match value.as_str() {
            "debug" => Self::Debug,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityOutcome {
    Succeeded,
    Failed,
    Unknown,
}

impl ActivityOutcome {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn from_db(value: String) -> Self {
        match value.as_str() {
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityScope {
    Client,
    Computer { computer_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityScopeFilter {
    #[default]
    All,
    ClientOnly,
    Computer {
        computer_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: i64,
    pub timestamp: String,
    pub scope: ActivityScope,
    pub level: ActivityLevel,
    pub category: String,
    pub event_type: String,
    pub operation: String,
    pub outcome: ActivityOutcome,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActivityEventDraft {
    pub scope: ActivityScope,
    pub level: ActivityLevel,
    pub category: String,
    pub event_type: String,
    pub operation: String,
    pub outcome: ActivityOutcome,
    pub message: String,
    pub fields: Option<serde_json::Value>,
    pub correlation_id: Option<String>,
}

impl ActivityEventDraft {
    pub fn client(
        level: ActivityLevel,
        category: impl Into<String>,
        event_type: impl Into<String>,
        operation: impl Into<String>,
        outcome: ActivityOutcome,
        message: impl Into<String>,
    ) -> Self {
        Self {
            scope: ActivityScope::Client,
            level,
            category: category.into(),
            event_type: event_type.into(),
            operation: operation.into(),
            outcome,
            message: message.into(),
            fields: None,
            correlation_id: None,
        }
    }

    pub fn computer(
        computer_id: impl Into<String>,
        level: ActivityLevel,
        category: impl Into<String>,
        event_type: impl Into<String>,
        operation: impl Into<String>,
        outcome: ActivityOutcome,
        message: impl Into<String>,
    ) -> Self {
        let mut draft = Self::client(level, category, event_type, operation, outcome, message);
        draft.scope = ActivityScope::Computer {
            computer_id: computer_id.into(),
        };
        draft
    }

    pub fn with_standard_fields(
        mut self,
        trigger: ActivityTrigger,
        managed_by: Option<ActivityManagedBy>,
        provider: Option<ActivityProvider>,
    ) -> Self {
        self.merge_fields(serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "trigger": trigger.as_str(),
            "managed_by": managed_by.map(ActivityManagedBy::as_str),
            "provider": provider.map(ActivityProvider::as_str),
        }));
        self
    }

    pub fn merge_fields(&mut self, extra: serde_json::Value) {
        let fields = self
            .fields
            .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(fields) = fields.as_object_mut() else {
            return;
        };
        let Some(extra) = extra.as_object() else {
            return;
        };
        for (key, value) in extra {
            fields.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }

    pub(crate) fn inherit_invocation_context(mut self) -> Self {
        let _ = ACTIVITY_INVOCATION_CONTEXT.try_with(|context| {
            self.correlation_id = Some(context.correlation_id.clone());
            let fields = self
                .fields
                .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if !fields.is_object() {
                *fields = serde_json::Value::Object(serde_json::Map::new());
            }
            let fields = fields.as_object_mut().expect("fields normalized to object");
            fields.insert("trigger".to_string(), context.trigger.as_str().into());
            fields.insert(
                "source_computer_id".to_string(),
                context.source_computer_id.clone().into(),
            );
            fields.insert(
                "target_computer_id".to_string(),
                context
                    .target_computer_id
                    .clone()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invocation_context_enriches_causality_without_overwriting_domain_ownership() {
        let context = ActivityInvocationContext {
            correlation_id: "request-1".to_string(),
            trigger: ActivityTrigger::ClientControl,
            provider: ActivityProvider::BuiltInMcp,
            managed_by: ActivityManagedBy::BuiltIn,
            source_computer_id: "source".to_string(),
            target_computer_id: Some("target".to_string()),
        };
        let draft = with_activity_invocation_context(context, async {
            let mut draft = ActivityEventDraft::computer(
                "target",
                ActivityLevel::Info,
                ComputerActivityCategory::Input,
                "input_value",
                "set",
                ActivityOutcome::Succeeded,
                "Input value set",
            );
            draft.fields = Some(serde_json::json!({
                "trigger": "explicit",
                "provider": ActivityProvider::UserMcp.as_str(),
                "managed_by": ActivityManagedBy::User.as_str(),
            }));
            draft.inherit_invocation_context()
        })
        .await;

        assert_eq!(draft.correlation_id.as_deref(), Some("request-1"));
        let fields = draft.fields.unwrap();
        assert_eq!(fields["trigger"], "client_control");
        assert_eq!(fields["provider"], "user_mcp");
        assert_eq!(fields["managed_by"], "user");
        assert_eq!(fields["source_computer_id"], "source");
        assert_eq!(fields["target_computer_id"], "target");
    }

    #[tokio::test]
    async fn captured_invocation_context_can_cross_a_spawn_boundary() {
        let context = ActivityInvocationContext {
            correlation_id: "request-spawned".to_string(),
            trigger: ActivityTrigger::ClientControl,
            provider: ActivityProvider::BuiltInMcp,
            managed_by: ActivityManagedBy::BuiltIn,
            source_computer_id: "source".to_string(),
            target_computer_id: Some("target".to_string()),
        };
        let captured = with_activity_invocation_context(context, async {
            current_activity_invocation_context().unwrap()
        })
        .await;
        let draft = tokio::spawn(async move {
            with_activity_invocation_context(captured, async {
                ActivityEventDraft::computer(
                    "target",
                    ActivityLevel::Info,
                    ComputerActivityCategory::Computer,
                    "profile",
                    "duplicate",
                    ActivityOutcome::Succeeded,
                    "Computer duplicated",
                )
                .inherit_invocation_context()
            })
            .await
        })
        .await
        .unwrap();

        assert_eq!(draft.correlation_id.as_deref(), Some("request-spawned"));
        assert_eq!(draft.fields.unwrap()["trigger"], "client_control");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityQuery {
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub levels: Option<Vec<ActivityLevel>>,
    pub categories: Option<Vec<String>>,
    pub keyword: Option<String>,
    #[serde(default)]
    pub scope: ActivityScopeFilter,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for ActivityQuery {
    fn default() -> Self {
        Self {
            start_time: None,
            end_time: None,
            levels: None,
            categories: None,
            keyword: None,
            scope: ActivityScopeFilter::All,
            limit: Some(50),
            offset: Some(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPage {
    pub items: Vec<ActivityEvent>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}
