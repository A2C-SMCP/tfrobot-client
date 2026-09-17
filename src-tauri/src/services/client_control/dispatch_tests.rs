use super::*;
use crate::services::computer::ComputerInstance;
use crate::services::config::ConfigService;
use crate::services::keychain::InMemorySecretStore;
use crate::services::observability::{ActivityQuery, ObservabilityService};
use crate::services::settings::SettingsService;
use serde_json::{json, Value};
use tempfile::TempDir;

async fn fixture() -> (TempDir, crate::AppState) {
    let temp = TempDir::new().unwrap();
    let config = ConfigService::new(temp.path().into()).unwrap();
    for id in ["source", "target"] {
        config
            .add_computer_instance(ComputerInstance::new(id, id))
            .unwrap();
    }
    let state = crate::AppState::new_with_secret_store(
        config,
        ObservabilityService::new(temp.path()).unwrap(),
        SettingsService::new(temp.path().into()),
        InMemorySecretStore::shared(),
    );
    state
        .client_control
        .update_policy_local(
            "source",
            RemoteControlPolicy {
                enabled: true,
                tool_scope: ToolScope::All,
                target_scope: TargetScope::All,
            },
        )
        .await
        .unwrap();
    (temp, state)
}

fn context(id: &str) -> InvocationContext {
    InvocationContext {
        request_id: id.into(),
        source_computer_id: "source".into(),
    }
}

async fn history_row(state: &crate::AppState, id: &str) -> Value {
    let history = state
        .client_control
        .dispatch(
            context(&format!("history-{id}")),
            ToolId::McpToolHistory,
            json!({"computer_id":"source"}),
        )
        .await
        .unwrap();
    let rows: Vec<_> = history
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["req_id"] == id)
        .collect();
    assert_eq!(rows.len(), 1, "one terminal record for {id}: {history}");
    let activities = state
        .observability
        .query_activity(&ActivityQuery::default())
        .unwrap();
    assert_eq!(
        activities
            .items
            .iter()
            .filter(
                |item| item.correlation_id.as_deref() == Some(id) && item.event_type == "tool_call"
            )
            .count(),
        1
    );
    rows[0].clone()
}

#[tokio::test]
async fn invalid_arguments_survive_sqlite_and_history_without_input_values() {
    let (_temp, state) = fixture().await;
    let cases = [
        (
            "variant",
            ToolId::McpServerUpsert,
            json!({"computer_id":"target", "server": {
                "type":"StreamableHttp", "name":"private-name", "server_parameters": {
                    "url":"https://user:private-password@example.test/?key=private-query",
                    "headers":{"X-Credential":"private-header"}
                }
            }}),
            "server",
            "unknown variant",
        ),
        (
            "missing",
            ToolId::McpServerUpsert,
            json!({"computer_id":"target"}),
            "arguments",
            "missing field `server`",
        ),
        (
            "nested-missing",
            ToolId::McpServerUpsert,
            json!({"computer_id":"target", "server": {"type":"stdio", "name":"demo"}}),
            "server",
            "missing field `server_parameters`",
        ),
        (
            "sensitive-type",
            ToolId::ComputerRename,
            json!({"computer_id":"target", "name":{"unassuming":"private-value"}}),
            "name",
            "invalid type",
        ),
        (
            "sensitive-scalar",
            ToolId::ComputerDuplicate,
            json!({"computer_id":"target", "name":"demo", "copy_robot_binding":"private-boolean"}),
            "copy_robot_binding",
            "invalid type",
        ),
        (
            "skill-update-decode",
            ToolId::SkillUpdate,
            json!({"computer_id":"target", "name":"demo", "changes":[{"action":"private-action", "content":"private-content"}]}),
            "changes",
            "unknown variant",
        ),
        (
            "skill-delete-decode",
            ToolId::SkillDelete,
            json!({"computer_id":"target"}),
            "arguments",
            "missing field `name`",
        ),
        (
            "sensitive-variant",
            ToolId::McpServerUpsert,
            json!({"computer_id":"target", "server":{"type":"private-variant"}}),
            "server",
            "unknown variant",
        ),
        (
            "skill-decode",
            ToolId::SkillCreate,
            json!({"computer_id":"target", "name":"demo", "files":[{"path":"SKILL.md", "encoding":"private-encoding", "content":"private-content"}]}),
            "files",
            "unknown variant",
        ),
        (
            "unknown-field",
            ToolId::ComputerGetStatus,
            json!({"computer_id":"target", "private-field":"private-value"}),
            "arguments",
            "unknown field",
        ),
        (
            "missing-target",
            ToolId::McpServerUpsert,
            json!({"server":{"type":"private-variant"}}),
            "computer_id",
            "required",
        ),
    ];
    for (id, tool, parameters, field, reason) in cases {
        let error = state
            .client_control
            .dispatch(context(id), tool, parameters)
            .await
            .unwrap_err();
        assert_eq!(error.code, ClientControlErrorCode::InvalidArguments);
        let row = history_row(&state, id).await;
        assert_eq!(row["success"], false);
        let message = row["error"].as_str().unwrap();
        assert!(message.contains("InvalidArguments"), "{id}: {message}");
        assert!(message.contains(field), "{id}: {message}");
        assert!(message.contains(reason), "{id}: {message}");
        assert!(!message.contains("before execution completed"));
        assert!(!row.to_string().contains("private-"), "{row}");
        assert!(!error.to_string().contains("private-"));
    }
    let activity = state
        .observability
        .query_activity(&ActivityQuery::default())
        .unwrap();
    assert!(!serde_json::to_string(&activity)
        .unwrap()
        .contains("private-"));
}

#[tokio::test]
async fn success_and_business_failure_each_have_one_terminal_record() {
    let (_temp, state) = fixture().await;
    state
        .client_control
        .dispatch(
            context("success"),
            ToolId::ComputerGetStatus,
            json!({"computer_id":"target"}),
        )
        .await
        .unwrap();
    assert_eq!(history_row(&state, "success").await["success"], true);
    let error = state
        .client_control
        .dispatch(
            context("failure"),
            ToolId::InputValueSet,
            json!({"computer_id":"target", "input_id":"not-defined", "value":"private-value"}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ClientControlErrorCode::OperationFailed);
    let row = history_row(&state, "failure").await;
    assert_eq!(row["success"], false);
    assert!(row["error"].as_str().unwrap().contains("OperationFailed"));
    assert!(!row.to_string().contains("private-value"));
}

#[tokio::test]
async fn skill_owned_audits_keep_summaries_and_do_not_duplicate() {
    let (_temp, state) = fixture().await;
    let parameters = json!({"computer_id":"target", "name":"demo", "files":[{
        "path":"SKILL.md", "encoding":"utf8", "content":"---\nname: demo\ndescription: Example\n---\nprivate-content\n"
    }]});
    state
        .client_control
        .dispatch(
            context("skill-success"),
            ToolId::SkillCreate,
            parameters.clone(),
        )
        .await
        .unwrap();
    let success = history_row(&state, "skill-success").await;
    assert_eq!(success["success"], true);
    assert!(success["parameters"].to_string().contains("SKILL.md"));
    assert!(!success.to_string().contains("private-content"));
    state
        .client_control
        .dispatch(context("skill-failure"), ToolId::SkillCreate, parameters)
        .await
        .unwrap_err();
    let failure = history_row(&state, "skill-failure").await;
    assert_eq!(failure["success"], false);
    assert!(!failure.to_string().contains("private-content"));
}

#[tokio::test]
async fn dropping_pending_dispatch_records_only_the_unfinished_terminal() {
    use std::future::Future;
    let (_temp, state) = fixture().await;
    let runtime = state.computer_registry.runtime("target").await.unwrap();
    let lease = runtime.acquire_skill_mutation_lease().await.unwrap();
    let mut call = Box::pin(state.client_control.dispatch(
        context("cancelled"),
        ToolId::SkillDelete,
        json!({"computer_id":"target", "name":"demo"}),
    ));
    // The held lifecycle lease makes the first poll stop inside execution, deterministically.
    std::future::poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(state
        .observability
        .tool_history("source", 100)
        .unwrap()
        .is_empty());
    drop(call);
    drop(lease);
    let row = history_row(&state, "cancelled").await;
    assert_eq!(row["success"], false);
    assert!(row["error"]
        .as_str()
        .unwrap()
        .contains("before execution completed"));
    assert!(!row["error"].as_str().unwrap().contains("InvalidArguments"));
}

#[tokio::test]
async fn missing_command_host_preserves_its_concrete_failure() {
    let (_temp, state) = fixture().await;
    let plane = Arc::new(ClientControlPlane::new(
        state.config.clone(),
        state.computer_registry.clone(),
        Arc::new(ObservabilityControlAuditSink::new(
            state.observability.clone(),
        )),
    ));
    let error = plane
        .dispatch(context("missing-host"), ToolId::ComputerList, json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ClientControlErrorCode::OperationFailed);
    let row = history_row(&state, "missing-host").await;
    assert_eq!(
        row["error"],
        "OperationFailed: Client Control command host is not available"
    );
}

#[tokio::test]
async fn sensitive_schema_field_names_keep_structured_validation_details() {
    let (_temp, state) = fixture().await;
    let error = state
        .client_control
        .dispatch(
            context("sensitive-field"),
            ToolId::ConnectionTargetUpsert,
            json!({"target":{"id":"demo", "name":"Demo", "namespace":"demo", "office_id":"demo"},
            "api_key_action":{"kind":"private-variant", "value":"private-value"}}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ClientControlErrorCode::InvalidArguments);
    let row = history_row(&state, "sensitive-field").await;
    let validation = &row["parameters"]["validation"];
    assert_eq!(validation["category"], "invalid_arguments");
    assert!(validation["message"]
        .as_str()
        .unwrap()
        .contains("api_key_action"));
    assert!(validation["message"]
        .as_str()
        .unwrap()
        .contains("unknown variant"));
    assert!(!row.to_string().contains("private-"));
}
