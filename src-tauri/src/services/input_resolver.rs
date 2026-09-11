use crate::services::input_entry_store::{InputEntryStorageKind, InputEntryStore};
use crate::services::keychain::SecretStore;
use crate::services::runtime_input_bridge::{
    RuntimeInputBridge, RuntimeInputCompletion, RuntimeInputRequestReason,
};
use a2c_smcp::smcp_computer::inputs::{
    InputResolutionError, InputValueResolver, SecretValueResolver,
};
use a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput;
use async_trait::async_trait;
use serde_json::Value;
use std::future::Future;
use std::sync::{Arc, Mutex};

pub(crate) const REDACTED_SECRET_SELECTION: &str = "<redacted secret selection>";
pub(crate) const RUNTIME_INPUT_CANCELLED: &str = "Runtime Input request was cancelled";
pub(crate) const RUNTIME_INPUT_REQUIRED: &str =
    "InputEntry is missing and user confirmation is required";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInputInteractionMode {
    NonInteractive,
    Interactive,
}

#[derive(Clone)]
struct RuntimeInputInteractionContext {
    instance_id: Arc<str>,
    mode: RuntimeInputInteractionMode,
    failure: Arc<Mutex<Option<InputResolutionError>>>,
}

tokio::task_local! {
    static RUNTIME_INPUT_INTERACTION: RuntimeInputInteractionContext;
}

/// Client-owned bridge between SDK runtime resolution and the OS-backed SecretStore.
///
/// The SDK receives only the resolved value for the current render operation. It never
/// receives a persisted value map and never owns the storage key namespace.
pub struct RuntimeInputResolver {
    instance_id: Arc<str>,
    entries: InputEntryStore,
    bridge: Arc<RuntimeInputBridge>,
}

impl RuntimeInputResolver {
    pub fn new(
        instance_id: impl Into<Arc<str>>,
        storage_root: impl AsRef<std::path::Path>,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self::new_with_bridge(
            instance_id,
            storage_root,
            secret_store,
            Arc::new(RuntimeInputBridge::new()),
        )
    }

    pub fn new_with_bridge(
        instance_id: impl Into<Arc<str>>,
        storage_root: impl AsRef<std::path::Path>,
        secret_store: Arc<dyn SecretStore>,
        bridge: Arc<RuntimeInputBridge>,
    ) -> Self {
        let instance_id = instance_id.into();
        Self {
            instance_id: instance_id.clone(),
            entries: InputEntryStore::from_storage_root(instance_id, storage_root, secret_store),
            bridge,
        }
    }

    pub async fn with_interaction_mode<F, T>(
        &self,
        mode: RuntimeInputInteractionMode,
        future: F,
    ) -> T
    where
        F: Future<Output = T>,
    {
        RUNTIME_INPUT_INTERACTION
            .scope(
                RuntimeInputInteractionContext {
                    instance_id: self.instance_id.clone(),
                    mode,
                    failure: Arc::new(Mutex::new(None)),
                },
                future,
            )
            .await
    }

    fn is_interactive(&self) -> bool {
        RUNTIME_INPUT_INTERACTION
            .try_with(|context| {
                context.instance_id.as_ref() == self.instance_id.as_ref()
                    && context.mode == RuntimeInputInteractionMode::Interactive
            })
            .unwrap_or(false)
    }

    async fn resolve_in_interaction<T>(
        &self,
        resolution: impl Future<Output = Result<T, InputResolutionError>>,
    ) -> Result<T, InputResolutionError> {
        // SDK batches serialize input resolution but continue after individual failures.
        // Keep a failed foreground interaction terminal within this operation, so queued
        // servers cannot open more prompts after cancellation or a persistence failure.
        // A new operation gets a fresh context; background recovery is unaffected.
        let failure = RUNTIME_INPUT_INTERACTION
            .try_with(|context| {
                (context.instance_id == self.instance_id
                    && context.mode == RuntimeInputInteractionMode::Interactive)
                    .then(|| context.failure.clone())
            })
            .ok()
            .flatten();
        if let Some(failure) = &failure {
            if let Some(error) = failure.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                return Err(error);
            }
        }
        let result = resolution.await;
        if let (Some(failure), Err(error)) = (failure, &result) {
            failure
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| error.clone());
        }
        result
    }

    async fn request_value(
        &self,
        definition: &MCPServerInput,
        reason: RuntimeInputRequestReason,
        storage_kind: InputEntryStorageKind,
    ) -> Result<Value, InputResolutionError> {
        if !self.is_interactive() {
            return Err(resolver_failed(definition.id(), RUNTIME_INPUT_REQUIRED));
        }
        let response = self
            .bridge
            .request(
                &self.instance_id,
                definition,
                reason,
                storage_kind.is_secret(),
            )
            .await
            .map_err(|error| resolver_failed(definition.id(), error))?;
        match response.completion.clone() {
            RuntimeInputCompletion::Cancelled => {
                response.acknowledge(Ok(()));
                Err(resolver_failed(definition.id(), RUNTIME_INPUT_CANCELLED))
            }
            RuntimeInputCompletion::Confirmed { value } => {
                let value = Value::String(value);
                if let Err(error) = validate_pick_value(definition, &value) {
                    let error = if storage_kind.is_secret() {
                        InputResolutionError::InvalidSelection {
                            id: definition.id().to_string(),
                            value: REDACTED_SECRET_SELECTION.to_string(),
                        }
                    } else {
                        error
                    };
                    let acknowledgement = if storage_kind.is_secret() {
                        "Secret selection is not one of the current options".to_string()
                    } else {
                        error.to_string()
                    };
                    response.acknowledge(Err(acknowledgement));
                    return Err(error);
                }
                if let Err(error) = self.entries.upsert(
                    definition.id(),
                    Some(value.clone()),
                    storage_kind.is_secret(),
                ) {
                    response.acknowledge(Err(error.clone()));
                    return Err(resolver_failed(definition.id(), error));
                }
                response.acknowledge(Ok(()));
                Ok(value)
            }
        }
    }
}

fn resolver_failed(input_id: &str, error: impl std::fmt::Display) -> InputResolutionError {
    InputResolutionError::resolver_failed(input_id, error.to_string())
}

fn validate_pick_value(
    definition: &MCPServerInput,
    value: &Value,
) -> Result<(), InputResolutionError> {
    let MCPServerInput::PickString(input) = definition else {
        return Ok(());
    };
    let Some(selected) = value.as_str() else {
        return Err(InputResolutionError::InvalidSelection {
            id: input.id.clone(),
            value: value.to_string(),
        });
    };
    if input.options.iter().any(|option| option.value == selected) {
        Ok(())
    } else {
        Err(InputResolutionError::InvalidSelection {
            id: input.id.clone(),
            value: selected.to_string(),
        })
    }
}

impl RuntimeInputResolver {
    async fn resolve_input_value(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<Value>, InputResolutionError> {
        if matches!(definition, MCPServerInput::Command(_)) {
            // Commands remain session-owned and execute on every real render. They never create
            // an InputEntry or enter the user-supplied value flow.
            return Ok(None);
        }
        let entry = self
            .entries
            .resolve_entry_state(definition.id(), InputEntryStorageKind::Value)
            .map_err(|error| resolver_failed(definition.id(), error))?;
        if let Some(entry) = entry {
            if let Some(value) = entry.value {
                if validate_pick_value(definition, &value).is_ok() {
                    return Ok(Some(value));
                }
                if self.is_interactive() {
                    return self
                        .request_value(
                            definition,
                            RuntimeInputRequestReason::InvalidSelection,
                            entry.storage_kind,
                        )
                        .await
                        .map(Some);
                }
                let selected = value.as_str().unwrap_or_default();
                return Err(InputResolutionError::InvalidSelection {
                    id: definition.id().to_string(),
                    value: if entry.storage_kind.is_secret() {
                        REDACTED_SECRET_SELECTION.to_string()
                    } else {
                        selected.to_string()
                    },
                });
            }
            return self
                .request_value(
                    definition,
                    RuntimeInputRequestReason::Missing,
                    entry.storage_kind,
                )
                .await
                .map(Some);
        }
        self.request_value(
            definition,
            RuntimeInputRequestReason::Missing,
            InputEntryStorageKind::Value,
        )
        .await
        .map(Some)
    }
}

impl RuntimeInputResolver {
    async fn resolve_secret_value(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<String>, InputResolutionError> {
        let entry = self
            .entries
            .resolve_entry_state(definition.id(), InputEntryStorageKind::Secret)
            .map_err(|error| resolver_failed(definition.id(), error))?;
        if let Some(entry) = entry.filter(|entry| entry.storage_kind.is_secret()) {
            if let Some(value) = entry.value {
                return value
                    .as_str()
                    .map(|value| Some(value.to_string()))
                    .ok_or_else(|| {
                        resolver_failed(definition.id(), "secret value is not a string")
                    });
            }
        }
        self.request_value(
            definition,
            RuntimeInputRequestReason::Missing,
            InputEntryStorageKind::Secret,
        )
        .await
        .and_then(|value| {
            value
                .as_str()
                .map(|value| Some(value.to_string()))
                .ok_or_else(|| resolver_failed(definition.id(), "secret value is not a string"))
        })
    }
}

#[async_trait]
impl InputValueResolver for RuntimeInputResolver {
    async fn resolve_input(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<Value>, InputResolutionError> {
        self.resolve_in_interaction(self.resolve_input_value(definition))
            .await
    }
}

#[async_trait]
impl SecretValueResolver for RuntimeInputResolver {
    async fn resolve_secret(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<String>, InputResolutionError> {
        self.resolve_in_interaction(self.resolve_secret_value(definition))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::input_value_store::InputValueStore;
    use crate::services::keychain::{
        delete_input_secret, InMemorySecretStore, KeychainError, SecretStore,
    };
    use crate::services::runtime_input_bridge::{RuntimeInputRequest, RuntimeInputRequestSink};
    use a2c_smcp::smcp_computer::mcp_clients::model::{
        CommandInput, MCPServerInput, PickStringInput, PickStringOption, PromptStringInput,
    };
    use std::collections::HashMap;
    use tokio::sync::{mpsc, oneshot};

    fn definition(id: &str, password: bool) -> MCPServerInput {
        MCPServerInput::PromptString(PromptStringInput {
            id: id.to_string(),
            description: String::new(),
            default: None,
            password: Some(password),
        })
    }

    struct FailingSecretStore;

    struct RecordingSink {
        sender: mpsc::UnboundedSender<RuntimeInputRequest>,
    }

    impl RuntimeInputRequestSink for RecordingSink {
        fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String> {
            self.sender
                .send(request.clone())
                .map_err(|error| error.to_string())
        }
    }

    fn interactive_resolver(
        storage_root: &std::path::Path,
        secret_store: Arc<dyn SecretStore>,
    ) -> (
        Arc<RuntimeInputResolver>,
        Arc<RuntimeInputBridge>,
        mpsc::UnboundedReceiver<RuntimeInputRequest>,
    ) {
        let bridge = Arc::new(RuntimeInputBridge::new());
        let (sender, receiver) = mpsc::unbounded_channel();
        bridge.set_sink(Arc::new(RecordingSink { sender }));
        bridge.set_ready("test", true);
        (
            Arc::new(RuntimeInputResolver::new_with_bridge(
                "computer-a",
                storage_root,
                secret_store,
                bridge.clone(),
            )),
            bridge,
            receiver,
        )
    }

    impl SecretStore for FailingSecretStore {
        fn set_secret(&self, _key: &str, _secret: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Store("keychain unavailable".to_string()))
        }

        fn get_secret(&self, _key: &str) -> Result<Option<String>, KeychainError> {
            Err(KeychainError::Store("keychain unavailable".to_string()))
        }

        fn delete_secret(&self, _key: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Store("keychain unavailable".to_string()))
        }
    }

    #[tokio::test]
    async fn input_resolver_accepts_secret_entries_but_secret_resolver_rejects_plain_entries() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        entries
            .upsert("secret-entry", Some(serde_json::json!("secret")), true)
            .unwrap();
        entries
            .upsert("plain-entry", Some(serde_json::json!("value")), false)
            .unwrap();
        let resolver = RuntimeInputResolver::new("computer-a", directory.path(), secrets);

        assert_eq!(
            InputValueResolver::resolve_input(&resolver, &definition("secret-entry", false))
                .await
                .unwrap(),
            Some(serde_json::json!("secret"))
        );
        assert!(matches!(
            SecretValueResolver::resolve_secret(&resolver, &definition("plain-entry", true)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "plain-entry" && reason == RUNTIME_INPUT_REQUIRED
        ));
        assert_eq!(
            entries.storage_kind("plain-entry").unwrap(),
            Some(InputEntryStorageKind::Value)
        );
    }

    #[tokio::test]
    async fn noninteractive_missing_values_fail_closed_before_sdk_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let resolver = RuntimeInputResolver::new(
            "computer-a",
            directory.path(),
            Arc::new(InMemorySecretStore::default()),
        );

        assert!(matches!(
            InputValueResolver::resolve_input(&resolver, &definition("value", false)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "value" && reason == RUNTIME_INPUT_REQUIRED
        ));
        assert!(matches!(
            SecretValueResolver::resolve_secret(&resolver, &definition("secret", true)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "secret" && reason == RUNTIME_INPUT_REQUIRED
        ));
    }

    #[tokio::test]
    async fn command_inputs_never_enter_the_entry_or_prompt_flow() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        let (resolver, _bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let command = MCPServerInput::Command(CommandInput {
            id: "dynamic-command".to_string(),
            description: "Resolve dynamically".to_string(),
            command: "echo".to_string(),
            args: Some(HashMap::from([(
                "000000".to_string(),
                "runtime".to_string(),
            )])),
        });
        assert_eq!(
            resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(resolver.as_ref(), &command),
                )
                .await
                .unwrap(),
            None
        );
        assert!(requests.try_recv().is_err());
        assert!(entries.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn interactive_context_does_not_leak_into_an_already_running_background_operation() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let (background_entered_tx, background_entered_rx) = oneshot::channel();
        let (release_background_tx, release_background_rx) = oneshot::channel();
        let background_resolver = resolver.clone();
        let background = tokio::spawn(async move {
            background_resolver
                .with_interaction_mode(RuntimeInputInteractionMode::NonInteractive, async {
                    let _ = background_entered_tx.send(());
                    let _ = release_background_rx.await;
                    InputValueResolver::resolve_input(
                        background_resolver.as_ref(),
                        &definition("background", false),
                    )
                    .await
                })
                .await
        });
        background_entered_rx.await.unwrap();

        let (interactive_entered_tx, interactive_entered_rx) = oneshot::channel();
        let (release_interactive_tx, release_interactive_rx) = oneshot::channel();
        let interactive_resolver = resolver.clone();
        let interactive = tokio::spawn(async move {
            interactive_resolver
                .with_interaction_mode(RuntimeInputInteractionMode::Interactive, async {
                    let _ = interactive_entered_tx.send(());
                    let _ = release_interactive_rx.await;
                    InputValueResolver::resolve_input(
                        interactive_resolver.as_ref(),
                        &definition("foreground", false),
                    )
                    .await
                })
                .await
        });
        interactive_entered_rx.await.unwrap();

        release_background_tx.send(()).unwrap();
        assert!(matches!(
            background.await.unwrap(),
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "background" && reason == RUNTIME_INPUT_REQUIRED
        ));
        assert!(requests.try_recv().is_err());

        release_interactive_tx.send(()).unwrap();
        let request = requests.recv().await.unwrap();
        assert_eq!(request.definition.id(), "foreground");
        let completion = bridge.complete(&request.request_id, RuntimeInputCompletion::Cancelled);
        let (completion, interactive) = tokio::join!(completion, interactive);
        completion.unwrap();
        assert!(matches!(
            interactive.unwrap(),
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "foreground" && reason == RUNTIME_INPUT_CANCELLED
        ));
    }

    #[tokio::test]
    async fn interactive_prompt_uses_default_only_in_request_and_saves_confirmed_value() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let definition = MCPServerInput::PromptString(PromptStringInput {
            id: "name".to_string(),
            description: "User name".to_string(),
            default: Some("Ada".to_string()),
            password: Some(false),
        });
        let task_resolver = resolver.clone();
        let task_definition = definition.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(task_resolver.as_ref(), &task_definition),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        assert_eq!(request.definition, definition);
        assert_eq!(request.reason, RuntimeInputRequestReason::Missing);
        assert!(!request.secret);
        assert_eq!(entries.get("name").unwrap(), None);
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "Grace".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);

        completion.unwrap();
        assert_eq!(
            resolution.unwrap().unwrap(),
            Some(serde_json::json!("Grace"))
        );
        assert_eq!(
            entries.get("name").unwrap().unwrap().value,
            Some(serde_json::json!("Grace"))
        );
    }

    #[tokio::test]
    async fn interactive_missing_pick_defaults_to_plain_storage() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let definition = MCPServerInput::PickString(PickStringInput {
            id: "region".to_string(),
            description: "Region".to_string(),
            options: vec![PickStringOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
            default: Some("cn".to_string()),
        });
        let task_resolver = resolver.clone();
        let task_definition = definition.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(task_resolver.as_ref(), &task_definition),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        assert_eq!(request.definition, definition);
        assert_eq!(request.reason, RuntimeInputRequestReason::Missing);
        assert!(!request.secret);
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "cn".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);

        completion.unwrap();
        assert_eq!(resolution.unwrap().unwrap(), Some(serde_json::json!("cn")));
        assert_eq!(
            entries.storage_kind("region").unwrap(),
            Some(InputEntryStorageKind::Value)
        );
        assert_eq!(
            entries.get("region").unwrap().unwrap().value,
            Some(serde_json::json!("cn"))
        );
    }

    #[tokio::test]
    async fn interactive_cancellation_returns_structured_error_without_saving() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let task_resolver = resolver.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(
                        task_resolver.as_ref(),
                        &definition("name", false),
                    ),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        let completion = bridge.complete(&request.request_id, RuntimeInputCompletion::Cancelled);
        let (completion, resolution) = tokio::join!(completion, resolution);

        completion.unwrap();
        assert!(matches!(
            resolution.unwrap(),
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "name" && reason == RUNTIME_INPUT_CANCELLED
        ));
        assert_eq!(entries.get("name").unwrap(), None);
    }

    #[tokio::test]
    async fn cancelled_batch_suppresses_later_prompts_but_new_operation_can_retry() {
        let directory = tempfile::tempdir().unwrap();
        let (resolver, bridge, mut requests) =
            interactive_resolver(directory.path(), Arc::new(InMemorySecretStore::default()));
        let task_resolver = resolver.clone();
        let batch = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(RuntimeInputInteractionMode::Interactive, async {
                    let first = task_resolver
                        .resolve_input(&definition("first", false))
                        .await;
                    let second = task_resolver
                        .resolve_secret(&definition("second", true))
                        .await;
                    (first, second)
                })
                .await
        });
        let request = requests.recv().await.unwrap();
        let completion = bridge.complete(&request.request_id, RuntimeInputCompletion::Cancelled);
        let (completion, batch) = tokio::join!(completion, batch);
        completion.unwrap();
        let (first, second) = batch.unwrap();
        for error in [first.unwrap_err(), second.unwrap_err()] {
            assert!(
                matches!(error, InputResolutionError::ResolverFailed { id, reason }
                if id == "first" && reason == RUNTIME_INPUT_CANCELLED)
            );
        }
        assert!(requests.try_recv().is_err());

        let retry = tokio::spawn(async move {
            resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    resolver.resolve_input(&definition("first", false)),
                )
                .await
        });
        let request = requests.recv().await.unwrap();
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "confirmed".to_string(),
            },
        );
        let (completion, retry) = tokio::join!(completion, retry);
        completion.unwrap();
        assert_eq!(
            retry.unwrap().unwrap(),
            Some(Value::String("confirmed".to_string()))
        );
    }

    #[tokio::test]
    async fn interactive_stale_pick_is_reconfirmed_and_preserves_storage_kind() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        entries
            .upsert("region", Some(serde_json::json!("retired")), true)
            .unwrap();
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let definition = MCPServerInput::PickString(PickStringInput {
            id: "region".to_string(),
            description: "Region".to_string(),
            options: vec![PickStringOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
            default: Some("cn".to_string()),
        });
        let task_resolver = resolver.clone();
        let task_definition = definition.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(task_resolver.as_ref(), &task_definition),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        assert_eq!(request.reason, RuntimeInputRequestReason::InvalidSelection);
        assert!(request.secret);
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "cn".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);

        completion.unwrap();
        assert_eq!(resolution.unwrap().unwrap(), Some(serde_json::json!("cn")));
        assert_eq!(
            entries.storage_kind("region").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
        assert_eq!(entries.get("region").unwrap().unwrap().value, None);
    }

    #[tokio::test]
    async fn invalid_interactive_pick_is_rejected_and_not_saved() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let definition = MCPServerInput::PickString(PickStringInput {
            id: "region".to_string(),
            description: "Region".to_string(),
            options: vec![PickStringOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
            default: None,
        });
        let task_resolver = resolver.clone();
        let task_definition = definition.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(task_resolver.as_ref(), &task_definition),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "retired".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);

        assert!(completion.unwrap_err().to_string().contains("retired"));
        assert!(matches!(
            resolution.unwrap(),
            Err(InputResolutionError::InvalidSelection { id, value })
                if id == "region" && value == "retired"
        ));
        assert_eq!(entries.get("region").unwrap(), None);
    }

    #[tokio::test]
    async fn invalid_interactive_secret_pick_is_redacted_from_both_completion_and_action_errors() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        entries
            .upsert("region", Some(serde_json::json!("old-secret")), true)
            .unwrap();
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let definition = MCPServerInput::PickString(PickStringInput {
            id: "region".to_string(),
            description: "Region".to_string(),
            options: vec![PickStringOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
            default: None,
        });
        let task_resolver = resolver.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(task_resolver.as_ref(), &definition),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        assert!(request.secret);
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "new-private-invalid".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);
        let completion_error = completion.unwrap_err();
        assert!(!completion_error.to_string().contains("new-private-invalid"));
        assert!(matches!(
            resolution.unwrap(),
            Err(InputResolutionError::InvalidSelection { id, value })
                if id == "region" && value == REDACTED_SECRET_SELECTION
        ));
        assert_eq!(
            entries.storage_kind("region").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
    }

    #[tokio::test]
    async fn interactive_password_is_persisted_only_as_a_secret() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let task_resolver = resolver.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    SecretValueResolver::resolve_secret(
                        task_resolver.as_ref(),
                        &definition("token", true),
                    ),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        assert!(request.secret);
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "sensitive".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);

        completion.unwrap();
        assert_eq!(resolution.unwrap().unwrap().as_deref(), Some("sensitive"));
        assert_eq!(
            entries.storage_kind("token").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
        assert_eq!(entries.get("token").unwrap().unwrap().value, None);
    }

    #[tokio::test]
    async fn dangling_secret_metadata_is_reconfirmed_without_plaintext_downgrade() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        entries
            .upsert("token", Some(serde_json::json!("old-secret")), true)
            .unwrap();
        delete_input_secret(secrets.as_ref(), "computer-a", "token").unwrap();
        assert_eq!(
            entries.storage_kind("token").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );

        let (resolver, bridge, mut requests) = interactive_resolver(directory.path(), secrets);
        let task_resolver = resolver.clone();
        let resolution = tokio::spawn(async move {
            task_resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    InputValueResolver::resolve_input(
                        task_resolver.as_ref(),
                        &definition("token", false),
                    ),
                )
                .await
        });

        let request = requests.recv().await.unwrap();
        assert!(request.secret);
        assert_eq!(request.reason, RuntimeInputRequestReason::Missing);
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "replacement-secret".to_string(),
            },
        );
        let (completion, resolution) = tokio::join!(completion, resolution);

        completion.unwrap();
        assert_eq!(
            resolution.unwrap().unwrap(),
            Some(serde_json::json!("replacement-secret"))
        );
        assert_eq!(
            entries.storage_kind("token").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
        assert_eq!(entries.get("token").unwrap().unwrap().value, None);
        assert_eq!(
            InputValueStore::from_storage_root(directory.path())
                .get("token")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn stale_pick_value_returns_structured_invalid_selection_without_deleting_it() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        entries
            .upsert("region", Some(serde_json::json!("retired")), false)
            .unwrap();
        let resolver = RuntimeInputResolver::new("computer-a", directory.path(), secrets);
        let definition = MCPServerInput::PickString(PickStringInput {
            id: "region".to_string(),
            description: "Region".to_string(),
            options: vec![PickStringOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
            default: Some("cn".to_string()),
        });

        assert!(matches!(
            InputValueResolver::resolve_input(&resolver, &definition).await,
            Err(InputResolutionError::InvalidSelection { id, value })
                if id == "region" && value == "retired"
        ));
        assert_eq!(
            entries.get("region").unwrap().unwrap().value,
            Some(serde_json::json!("retired"))
        );
    }

    #[tokio::test]
    async fn stale_secret_pick_value_is_redacted_before_it_leaves_the_resolver() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let entries =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        entries
            .upsert("region", Some(serde_json::json!("private-retired")), true)
            .unwrap();
        let resolver = RuntimeInputResolver::new("computer-a", directory.path(), secrets);
        let definition = MCPServerInput::PickString(PickStringInput {
            id: "region".to_string(),
            description: "Region".to_string(),
            options: vec![PickStringOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
            default: None,
        });

        assert!(matches!(
            InputValueResolver::resolve_input(&resolver, &definition).await,
            Err(InputResolutionError::InvalidSelection { id, value })
                if id == "region"
                    && value == REDACTED_SECRET_SELECTION
                    && !value.contains("private-retired")
        ));
        assert_eq!(
            entries.storage_kind("region").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
    }

    #[tokio::test]
    async fn resolvers_are_isolated_by_computer_instance() {
        let directory = tempfile::tempdir().unwrap();
        let root_a = directory.path().join("computer-a");
        let root_b = directory.path().join("computer-b");
        let secrets = Arc::new(InMemorySecretStore::default());
        InputEntryStore::from_storage_root("computer-a", &root_a, secrets.clone())
            .upsert("shared", Some(serde_json::json!("value-a")), false)
            .unwrap();
        InputEntryStore::from_storage_root("computer-b", &root_b, secrets.clone())
            .upsert("shared", Some(serde_json::json!("value-b")), false)
            .unwrap();
        let resolver_a = RuntimeInputResolver::new("computer-a", &root_a, secrets.clone());
        let resolver_b = RuntimeInputResolver::new("computer-b", &root_b, secrets);

        assert_eq!(
            InputValueResolver::resolve_input(&resolver_a, &definition("shared", false))
                .await
                .unwrap()
                .as_ref(),
            Some(&serde_json::json!("value-a"))
        );
        assert_eq!(
            InputValueResolver::resolve_input(&resolver_b, &definition("shared", false))
                .await
                .unwrap()
                .as_ref(),
            Some(&serde_json::json!("value-b"))
        );
    }

    #[tokio::test]
    async fn secret_read_failure_stops_later_prompts_only_in_the_current_operation() {
        let directory = tempfile::tempdir().unwrap();
        let (resolver, bridge, mut requests) =
            interactive_resolver(directory.path(), Arc::new(FailingSecretStore));
        resolver
            .with_interaction_mode(RuntimeInputInteractionMode::Interactive, async {
                let first = resolver
                    .resolve_secret(&definition("secret", true))
                    .await
                    .unwrap_err();
                let later = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    resolver.resolve_input(&definition("plain", false)),
                )
                .await
                .expect("failed input read must prevent the next prompt")
                .unwrap_err();
                assert_eq!(first.to_string(), later.to_string());
                assert!(first.to_string().contains("keychain unavailable"));
            })
            .await;
        assert!(requests.try_recv().is_err());

        let retry = tokio::spawn(async move {
            resolver
                .with_interaction_mode(
                    RuntimeInputInteractionMode::Interactive,
                    resolver.resolve_input(&definition("plain", false)),
                )
                .await
        });
        let request = tokio::time::timeout(std::time::Duration::from_secs(1), requests.recv())
            .await
            .expect("a new operation must be able to request plain input")
            .unwrap();
        let completion = bridge.complete(
            &request.request_id,
            RuntimeInputCompletion::Confirmed {
                value: "plain-value".to_string(),
            },
        );
        let (completion, retry) = tokio::join!(completion, retry);
        completion.unwrap();
        assert_eq!(
            retry.unwrap().unwrap(),
            Some(Value::String("plain-value".to_string()))
        );
    }

    #[tokio::test]
    async fn secret_store_failures_do_not_affect_plain_values() {
        let directory = tempfile::tempdir().unwrap();
        let resolver =
            RuntimeInputResolver::new("computer-a", directory.path(), Arc::new(FailingSecretStore));

        assert!(matches!(
            InputValueResolver::resolve_input(&resolver, &definition("value", false)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "value" && reason == RUNTIME_INPUT_REQUIRED
        ));
        assert!(matches!(
            SecretValueResolver::resolve_secret(&resolver, &definition("secret", true)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "secret" && reason.contains("keychain unavailable")
        ));
    }
}
