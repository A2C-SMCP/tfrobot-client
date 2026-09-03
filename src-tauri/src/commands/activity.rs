use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityOutcome, ActivityPage, ActivityQuery,
    ActivityScopeFilter,
};
use crate::AppState;
use serde::Deserialize;
use tauri::State;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationUpdateActivity {
    CheckFailed,
    InstallStarted,
    InstallSucceeded,
    InstallFailed,
}

#[tauri::command]
pub async fn get_activity(
    state: State<'_, AppState>,
    query: Option<ActivityQuery>,
) -> Result<ActivityPage, String> {
    let query = query.unwrap_or_default();
    state.observability.query_activity_async(query).await
}

#[tauri::command]
pub async fn export_activity(
    state: State<'_, AppState>,
    path: String,
    query: Option<ActivityQuery>,
) -> Result<(), String> {
    let service = state.observability.as_ref().clone();
    let query = query.unwrap_or_default();
    let scope = query.scope.clone();
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (json, count) = service.export_activity_with_count(&query)?;
        std::fs::write(path, json)
            .map(|()| count)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?;

    let (level, outcome, message, count, error) = match &result {
        Ok(count) => (
            ActivityLevel::Info,
            ActivityOutcome::Succeeded,
            "Activity exported",
            Some(*count),
            None,
        ),
        Err(error) => (
            ActivityLevel::Warn,
            ActivityOutcome::Failed,
            "Activity export failed",
            None,
            Some(redact_text(error)),
        ),
    };
    let mut activity = ActivityEventDraft::client(
        level,
        "observability",
        "activity_journal",
        "export",
        outcome,
        message,
    );
    activity.fields = Some(serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "trigger": "user",
        "scope": scope,
        "exported_count": count,
        "error": error,
    }));
    activity.correlation_id = Some(correlation_id);
    if let Err(error) = state.observability.record_activity_async(activity).await {
        log::error!("failed to persist activity export audit: {error}");
    }
    result.map(|_| ())
}

#[tauri::command]
pub async fn clear_activity(
    state: State<'_, AppState>,
    scope: Option<ActivityScopeFilter>,
) -> Result<u64, String> {
    let service = state.observability.as_ref().clone();
    let scope = scope.unwrap_or_default();
    let audit_scope = scope.clone();
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let mut activity = ActivityEventDraft::client(
        ActivityLevel::Info,
        "observability",
        "activity_journal",
        "clear",
        ActivityOutcome::Succeeded,
        "Activity cleared",
    );
    activity.fields = Some(serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "trigger": "user",
        "scope": audit_scope.clone(),
    }));
    activity.correlation_id = Some(correlation_id.clone());
    let result = tauri::async_runtime::spawn_blocking(move || {
        service.clear_activity_with_audit(&scope, activity)
    })
    .await
    .map_err(|error| error.to_string())?;
    if let Err(error) = &result {
        let mut failed = ActivityEventDraft::client(
            ActivityLevel::Warn,
            "observability",
            "activity_journal",
            "clear",
            ActivityOutcome::Failed,
            "Activity clear failed",
        );
        failed.fields = Some(serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "trigger": "user",
            "scope": audit_scope,
            "error": redact_text(error),
        }));
        failed.correlation_id = Some(correlation_id);
        if let Err(persist_error) = state.observability.record_activity_async(failed).await {
            log::error!("failed to persist activity clear failure: {persist_error}");
        }
    }
    result
}

#[tauri::command]
pub async fn record_application_update_activity(
    state: State<'_, AppState>,
    activity: ApplicationUpdateActivity,
    target_version: Option<String>,
    correlation_id: String,
    error: Option<String>,
) -> Result<(), String> {
    record_application_update_activity_core(&state, activity, target_version, correlation_id, error)
        .await
}

pub async fn record_application_update_activity_core(
    state: &AppState,
    activity: ApplicationUpdateActivity,
    target_version: Option<String>,
    correlation_id: String,
    error: Option<String>,
) -> Result<(), String> {
    if correlation_id.trim().is_empty() || correlation_id.len() > 128 {
        return Err("invalid update correlation id".to_string());
    }
    if target_version
        .as_ref()
        .is_some_and(|version| version.len() > 128 || version.contains(['\r', '\n']))
    {
        return Err("invalid update target version".to_string());
    }
    let (operation, level, outcome, message) = match activity {
        ApplicationUpdateActivity::CheckFailed => (
            "check",
            ActivityLevel::Warn,
            ActivityOutcome::Failed,
            "Application update check failed",
        ),
        ApplicationUpdateActivity::InstallStarted => (
            "install",
            ActivityLevel::Info,
            ActivityOutcome::Unknown,
            "Application update installation started",
        ),
        ApplicationUpdateActivity::InstallSucceeded => (
            "install",
            ActivityLevel::Info,
            ActivityOutcome::Succeeded,
            "Application update installed",
        ),
        ApplicationUpdateActivity::InstallFailed => (
            "install",
            ActivityLevel::Warn,
            ActivityOutcome::Failed,
            "Application update installation failed",
        ),
    };
    let mut draft = ActivityEventDraft::client(
        level,
        "update",
        "application_update",
        operation,
        outcome,
        message,
    );
    draft.fields = Some(serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "trigger": "user",
        "target_version": target_version,
        "error": error.as_deref().map(redact_text),
    }));
    draft.correlation_id = Some(correlation_id);
    state.observability.record_activity_async(draft).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::keychain::InMemorySecretStore;
    use crate::services::observability::{ActivityQuery, ObservabilityService};
    use crate::services::settings::SettingsService;
    use tempfile::tempdir;

    #[tokio::test]
    async fn application_update_activity_is_correlated_and_redacted() {
        let dir = tempdir().unwrap();
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );

        record_application_update_activity_core(
            &state,
            ApplicationUpdateActivity::InstallFailed,
            Some("0.3.0".to_string()),
            "update-1".to_string(),
            Some("token=super-secret".to_string()),
        )
        .await
        .unwrap();

        let page = state
            .observability
            .query_activity(&ActivityQuery::default())
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].category, "update");
        assert_eq!(page.items[0].outcome, ActivityOutcome::Failed);
        assert_eq!(page.items[0].correlation_id.as_deref(), Some("update-1"));
        assert!(!serde_json::to_string(&page.items[0].fields)
            .unwrap()
            .contains("super-secret"));
    }

    #[tokio::test]
    async fn application_update_activity_rejects_multiline_versions() {
        let dir = tempdir().unwrap();
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );
        assert!(record_application_update_activity_core(
            &state,
            ApplicationUpdateActivity::InstallStarted,
            Some("0.3.0\nforged".to_string()),
            "update-1".to_string(),
            None,
        )
        .await
        .is_err());
        assert_eq!(
            state
                .observability
                .query_activity(&ActivityQuery::default())
                .unwrap()
                .total,
            0
        );
    }
}
