use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityManagedBy, ActivityOutcome,
    ActivityProvider, ActivityTrigger, ComputerActivityCategory,
};
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::settings::config::ProvenanceScope;
use std::time::Instant;

pub(crate) struct ComputerActivitySpec<'a> {
    pub computer_id: &'a str,
    pub category: ComputerActivityCategory,
    pub event_type: &'a str,
    pub operation: &'a str,
    pub trigger: ActivityTrigger,
    pub managed_by: Option<ActivityManagedBy>,
    pub provider: Option<ActivityProvider>,
    pub message_subject: &'a str,
    pub fields: serde_json::Value,
}

pub(crate) fn mcp_activity_ownership(
    state: &AppState,
    computer_id: &str,
    bundle_id: &str,
) -> (ActivityManagedBy, ActivityProvider) {
    if crate::services::computer::is_reserved_built_in_bundle_id(bundle_id) {
        return (ActivityManagedBy::BuiltIn, ActivityProvider::BuiltInMcp);
    }
    let plugin_owned = state
        .sdk_config
        .load(computer_id)
        .mcp
        .servers
        .into_iter()
        .any(|server| {
            server.origin == ProvenanceScope::Plugin
                && resolve_bundle_id(&server.config).as_str() == bundle_id
        });
    if plugin_owned {
        (ActivityManagedBy::Plugin, ActivityProvider::PluginMcp)
    } else {
        (ActivityManagedBy::User, ActivityProvider::UserMcp)
    }
}

/// Persist one completed Computer operation using the shared taxonomy and redaction contract.
/// Persistence is deliberately best-effort: observability must not change the domain outcome.
pub(crate) async fn record_computer_activity<T, E: std::fmt::Display>(
    state: &AppState,
    spec: ComputerActivitySpec<'_>,
    started: Instant,
    result: &Result<T, E>,
) {
    let (level, outcome, suffix, error) = match result {
        Ok(_) => (
            ActivityLevel::Info,
            ActivityOutcome::Succeeded,
            "succeeded",
            None,
        ),
        Err(error) => (
            ActivityLevel::Warn,
            ActivityOutcome::Failed,
            "failed",
            Some(redact_text(&error.to_string())),
        ),
    };
    let mut activity = ActivityEventDraft::computer(
        spec.computer_id,
        level,
        spec.category,
        spec.event_type,
        spec.operation,
        outcome,
        format!("{} {suffix}", spec.message_subject),
    )
    .with_standard_fields(spec.trigger, spec.managed_by, spec.provider);
    activity.merge_fields(spec.fields);
    activity.merge_fields(serde_json::json!({
        "duration_ms": started.elapsed().as_millis(),
        "error": error,
    }));
    if let Err(error) = state.observability.record_activity_async(activity).await {
        log::error!(
            "failed to persist {} {} activity: {error}",
            spec.event_type,
            spec.operation
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::keychain::InMemorySecretStore;
    use crate::services::observability::{
        ActivityQuery, ActivityScopeFilter, ObservabilityService,
    };
    use crate::services::settings::SettingsService;
    use tempfile::tempdir;

    #[tokio::test]
    async fn standard_writer_redacts_errors_and_emits_contract_fields() {
        let dir = tempdir().unwrap();
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );
        let result: Result<(), String> = Err("token=top-secret".to_string());
        record_computer_activity(
            &state,
            ComputerActivitySpec {
                computer_id: "computer-1",
                category: ComputerActivityCategory::Input,
                event_type: "input_value",
                operation: "set",
                trigger: ActivityTrigger::User,
                managed_by: Some(ActivityManagedBy::User),
                provider: Some(ActivityProvider::Client),
                message_subject: "Input value set",
                fields: serde_json::json!({"input_id": "secret"}),
            },
            Instant::now(),
            &result,
        )
        .await;

        let page = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "computer-1".to_string(),
                },
                ..Default::default()
            })
            .unwrap();
        let event = &page.items[0];
        assert_eq!(event.category, "input");
        let fields = event.fields.as_ref().unwrap();
        assert_eq!(fields["trigger"], "user");
        assert_eq!(fields["managed_by"], "user");
        assert_eq!(fields["provider"], "client");
        assert!(!fields.to_string().contains("top-secret"));
    }
}
