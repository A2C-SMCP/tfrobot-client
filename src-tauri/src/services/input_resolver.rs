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
    store: Arc<dyn SecretStore>,
}

impl RuntimeInputResolver {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store }
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
        keychain::get_input_value(self.store.as_ref(), definition.id())
            .map_err(|error| resolver_failed(definition.id(), error))
    }
}

#[async_trait]
impl SecretValueResolver for RuntimeInputResolver {
    async fn resolve_secret(
        &self,
        definition: &MCPServerInput,
    ) -> Result<Option<String>, InputResolutionError> {
        if let Some(secret) = keychain::get_input_secret(self.store.as_ref(), definition.id())
            .map_err(|error| resolver_failed(definition.id(), error))?
        {
            return Ok(Some(secret));
        }

        // TFRC-60 stored every input as JSON under input-value. Keep a read-only fallback so
        // existing password values survive the namespace split; the next edit migrates them.
        keychain::get_input_value(self.store.as_ref(), definition.id())
            .map_err(|error| resolver_failed(definition.id(), error))?
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    InputResolutionError::resolver_failed(
                        definition.id(),
                        "legacy secret value is not a string",
                    )
                })
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keychain::{self, InMemorySecretStore, KeychainError, SecretStore};
    use a2c_smcp::smcp_computer::mcp_clients::model::{MCPServerInput, PromptStringInput};

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
        let store = Arc::new(InMemorySecretStore::default());
        keychain::set_input_value(store.as_ref(), "token", &serde_json::json!("value")).unwrap();
        keychain::set_input_secret(store.as_ref(), "token", "secret").unwrap();
        let resolver = RuntimeInputResolver::new(store);

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
        let resolver = RuntimeInputResolver::new(Arc::new(InMemorySecretStore::default()));

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
    async fn secret_resolver_reads_legacy_string_value_without_exposing_non_strings() {
        let store = Arc::new(InMemorySecretStore::default());
        keychain::set_input_value(store.as_ref(), "legacy", &serde_json::json!("old-secret"))
            .unwrap();
        keychain::set_input_value(
            store.as_ref(),
            "invalid",
            &serde_json::json!({"token": true}),
        )
        .unwrap();
        let resolver = RuntimeInputResolver::new(store);

        assert_eq!(
            SecretValueResolver::resolve_secret(&resolver, &definition("legacy", true))
                .await
                .unwrap()
                .as_deref(),
            Some("old-secret")
        );
        assert!(matches!(
            SecretValueResolver::resolve_secret(&resolver, &definition("invalid", true)).await,
            Err(InputResolutionError::ResolverFailed { id, .. }) if id == "invalid"
        ));
    }

    #[tokio::test]
    async fn secret_store_failures_remain_structured_resolver_errors() {
        let resolver = RuntimeInputResolver::new(Arc::new(FailingSecretStore));

        assert!(matches!(
            InputValueResolver::resolve_input(&resolver, &definition("value", false)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "value" && reason.contains("keychain unavailable")
        ));
        assert!(matches!(
            SecretValueResolver::resolve_secret(&resolver, &definition("secret", true)).await,
            Err(InputResolutionError::ResolverFailed { id, reason })
                if id == "secret" && reason.contains("keychain unavailable")
        ));
    }
}
