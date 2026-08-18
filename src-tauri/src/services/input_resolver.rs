use crate::services::input_entry_store::{InputEntryStorageKind, InputEntryStore};
use crate::services::keychain::SecretStore;
use a2c_smcp::smcp_computer::inputs::{
    InputResolutionError, InputValueResolver, SecretValueResolver,
};
use a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const REDACTED_SECRET_SELECTION: &str = "<redacted secret selection>";

/// Client-owned bridge between SDK runtime resolution and the OS-backed SecretStore.
///
/// The SDK receives only the resolved value for the current render operation. It never
/// receives a persisted value map and never owns the storage key namespace.
#[derive(Clone)]
pub struct RuntimeInputResolver {
    entries: InputEntryStore,
}

impl RuntimeInputResolver {
    pub fn new(
        instance_id: impl Into<Arc<str>>,
        storage_root: impl AsRef<std::path::Path>,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            entries: InputEntryStore::from_storage_root(instance_id, storage_root, secret_store),
        }
    }
}

fn resolver_failed(input_id: &str, error: impl std::fmt::Display) -> InputResolutionError {
    InputResolutionError::resolver_failed(input_id, error.to_string())
}

#[async_trait]
impl InputValueResolver for RuntimeInputResolver {
    async fn resolve_input(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<Value>, InputResolutionError> {
        let entry = self
            .entries
            .resolve_entry(definition.id(), InputEntryStorageKind::Value)
            .map_err(|error| resolver_failed(definition.id(), error))?;
        if let (MCPServerInput::PickString(input), Some(Value::String(selected))) =
            (definition, entry.as_ref().map(|entry| &entry.value))
        {
            if !input.options.iter().any(|option| option.value == *selected) {
                return Err(InputResolutionError::InvalidSelection {
                    id: input.id.clone(),
                    value: if entry
                        .as_ref()
                        .is_some_and(|entry| entry.storage_kind.is_secret())
                    {
                        REDACTED_SECRET_SELECTION.to_string()
                    } else {
                        selected.clone()
                    },
                });
            }
        }
        Ok(entry.map(|entry| entry.value))
    }
}

#[async_trait]
impl SecretValueResolver for RuntimeInputResolver {
    async fn resolve_secret(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<String>, InputResolutionError> {
        self.entries
            .resolve(definition.id(), InputEntryStorageKind::Secret)
            .map(|value| value.and_then(|value| value.as_str().map(str::to_string)))
            .map_err(|error| resolver_failed(definition.id(), error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keychain::{InMemorySecretStore, KeychainError, SecretStore};
    use a2c_smcp::smcp_computer::mcp_clients::model::{
        MCPServerInput, PickStringInput, PickStringOption, PromptStringInput,
    };

    fn definition(id: &str, password: bool) -> MCPServerInput {
        MCPServerInput::PromptString(PromptStringInput {
            id: id.to_string(),
            description: String::new(),
            default: None,
            password: Some(password),
        })
    }

    struct FailingSecretStore;

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
    async fn entry_storage_kind_is_independent_of_sdk_definition_kind() {
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
        assert_eq!(
            SecretValueResolver::resolve_secret(&resolver, &definition("plain-entry", true))
                .await
                .unwrap()
                .as_deref(),
            Some("value")
        );
    }

    #[tokio::test]
    async fn missing_values_remain_unresolved_for_sdk_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let resolver = RuntimeInputResolver::new(
            "computer-a",
            directory.path(),
            Arc::new(InMemorySecretStore::default()),
        );

        assert_eq!(
            InputValueResolver::resolve_input(&resolver, &definition("value", false))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            SecretValueResolver::resolve_secret(&resolver, &definition("secret", true))
                .await
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
    async fn secret_store_failures_do_not_affect_plain_values() {
        let directory = tempfile::tempdir().unwrap();
        let resolver =
            RuntimeInputResolver::new("computer-a", directory.path(), Arc::new(FailingSecretStore));

        assert_eq!(
            InputValueResolver::resolve_input(&resolver, &definition("value", false))
                .await
                .unwrap(),
            None
        );
        assert!(matches!(
            SecretValueResolver::resolve_secret(&resolver, &definition("secret", true)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "secret" && reason.contains("keychain unavailable")
        ));
    }
}
