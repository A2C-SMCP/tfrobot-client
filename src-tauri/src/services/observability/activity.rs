use serde::{Deserialize, Serialize};

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
