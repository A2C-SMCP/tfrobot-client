use crate::services::input_value_store::InputValueStore;
use crate::services::keychain::{self, SecretStore};
use a2c_smcp::smcp_computer::inputs::{
    InputResolutionError, InputValueResolver, SecretValueResolver,
};
use a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Client-owned bridge between SDK runtime resolution and the OS-backed SecretStore.
///
/// The SDK receives only the resolved value for the current render operation. It never
/// receives a persisted value map and never owns the storage key namespace.
#[derive(Clone)]
pub struct RuntimeInputResolver {
    instance_id: Arc<str>,
    value_store: InputValueStore,
    secret_store: Arc<dyn SecretStore>,
}

impl RuntimeInputResolver {
    pub fn new(
        instance_id: impl Into<Arc<str>>,
        value_store: InputValueStore,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            instance_id: instance_id.into(),
            value_store,
            secret_store,
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
        let value = self
            .value_store
            .get(definition.id())
            .map_err(|error| resolver_failed(definition.id(), error))?;
        if let (MCPServerInput::PickString(input), Some(Value::String(selected))) =
            (definition, value.as_ref())
        {
            if !input.options.iter().any(|option| option.value == *selected) {
                return Err(InputResolutionError::InvalidSelection {
                    id: input.id.clone(),
                    value: selected.clone(),
                });
            }
        }
        Ok(value)
    }
}

#[async_trait]
impl SecretValueResolver for RuntimeInputResolver {
    async fn resolve_secret(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<String>, InputResolutionError> {
        keychain::get_input_secret(
            self.secret_store.as_ref(),
            self.instance_id.as_ref(),
            definition.id(),
        )
        .map_err(|error| resolver_failed(definition.id(), error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keychain::{self, InMemorySecretStore, KeychainError, SecretStore};
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
    async fn value_and_secret_resolvers_use_isolated_namespaces() {
        let directory = tempfile::tempdir().unwrap();
        let values = InputValueStore::from_storage_root(directory.path());
        let secrets = Arc::new(InMemorySecretStore::default());
        values.set("token", &serde_json::json!("value")).unwrap();
        keychain::set_input_secret(secrets.as_ref(), "computer-a", "token", "secret").unwrap();
        let resolver = RuntimeInputResolver::new("computer-a", values, secrets);

        assert_eq!(
            InputValueResolver::resolve_input(&resolver, &definition("token", false))
                .await
                .unwrap(),
            Some(serde_json::json!("value"))
        );
        assert_eq!(
            SecretValueResolver::resolve_secret(&resolver, &definition("token", true))
                .await
                .unwrap()
                .as_deref(),
            Some("secret")
        );
    }

    #[tokio::test]
    async fn missing_values_remain_unresolved_for_sdk_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let resolver = RuntimeInputResolver::new(
            "computer-a",
            InputValueStore::from_storage_root(directory.path()),
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
        let values = InputValueStore::from_storage_root(directory.path());
        values.set("region", &serde_json::json!("retired")).unwrap();
        let resolver = RuntimeInputResolver::new(
            "computer-a",
            values.clone(),
            Arc::new(InMemorySecretStore::default()),
        );
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
            values.get("region").unwrap(),
            Some(serde_json::json!("retired"))
        );
    }

    #[tokio::test]
    async fn resolvers_are_isolated_by_computer_instance() {
        let directory = tempfile::tempdir().unwrap();
        let values_a = InputValueStore::from_storage_root(directory.path().join("computer-a"));
        let values_b = InputValueStore::from_storage_root(directory.path().join("computer-b"));
        values_a
            .set("shared", &serde_json::json!("value-a"))
            .unwrap();
        values_b
            .set("shared", &serde_json::json!("value-b"))
            .unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let resolver_a = RuntimeInputResolver::new("computer-a", values_a, secrets.clone());
        let resolver_b = RuntimeInputResolver::new("computer-b", values_b, secrets);

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
        let resolver = RuntimeInputResolver::new(
            "computer-a",
            InputValueStore::from_storage_root(directory.path()),
            Arc::new(FailingSecretStore),
        );

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
