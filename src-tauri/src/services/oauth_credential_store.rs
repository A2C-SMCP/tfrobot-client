use crate::services::keychain::{oauth_credential_key, SecretStore};
use a2c_smcp::smcp_computer::mcp_clients::{
    bundle_id::resolve_bundle_id,
    manager::MCPServerManager,
    model::{HttpAuthPolicy, HttpServerConfig},
    MCPServerConfig,
};
use a2c_smcp::smcp_computer::oauth::{
    OAuthClientMode, OAuthCredentialKey, OAuthCredentialStore, OAuthCredentialStoreError,
    OAuthOptions,
};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EffectiveHttpOAuth {
    pub automatic: bool,
    pub interactive: bool,
}

/// Resolve the SDK's backward-compatible HTTP authentication defaults in one client-owned place.
/// In particular, an omitted policy and omitted OAuth block is anonymous-first Auto OAuth.
pub(crate) fn effective_http_oauth(config: &HttpServerConfig) -> Option<EffectiveHttpOAuth> {
    // The SDK treats a literal Authorization header as static credentials and never falls back
    // to OAuth for that request. Mirror that precedence here so host-side UI and credential
    // cleanup cannot misclassify a static-auth server as legacy Auto OAuth.
    if config
        .server_parameters
        .headers
        .keys()
        .any(|header| header.eq_ignore_ascii_case("authorization"))
    {
        return None;
    }
    let interactive = config
        .oauth
        .as_ref()
        .is_none_or(|oauth| matches!(oauth.mode, OAuthClientMode::AuthorizationCode { .. }));
    match config.auth_policy {
        Some(HttpAuthPolicy::Disabled) => None,
        Some(HttpAuthPolicy::OAuth) if config.oauth.is_none() => None,
        Some(HttpAuthPolicy::OAuth) => Some(EffectiveHttpOAuth {
            automatic: false,
            interactive,
        }),
        Some(HttpAuthPolicy::Auto) => Some(EffectiveHttpOAuth {
            automatic: true,
            interactive,
        }),
        None => Some(EffectiveHttpOAuth {
            automatic: config.oauth.is_none(),
            interactive,
        }),
        Some(_) => None,
    }
}

/// Materialize Auto defaults as proactive options only for offline credential deletion. The SDK
/// intentionally does not admit Auto OAuth without a validated challenge, but credential cleanup
/// must be network-free and address the same bundle/resource/mode key after a runtime is gone.
pub(crate) fn oauth_cleanup_config(mut config: MCPServerConfig) -> Option<MCPServerConfig> {
    let MCPServerConfig::Http(http) = &mut config else {
        return None;
    };
    let effective = effective_http_oauth(http)?;
    if http.oauth.is_none() {
        http.oauth = Some(OAuthOptions::default());
    }
    if effective.automatic {
        http.auth_policy = Some(HttpAuthPolicy::OAuth);
    }
    Some(config)
}

/// Persists SDK-owned OAuth credential envelopes in the client-owned OS keychain namespace.
///
/// The adapter binds a trusted Computer instance ID at construction time. OAuth callback input
/// and serialized MCP configuration therefore cannot select another Computer's credential slot.
#[derive(Clone)]
pub struct KeychainOAuthCredentialStore {
    instance_id: Arc<str>,
    store: Arc<dyn SecretStore>,
}

impl KeychainOAuthCredentialStore {
    pub fn new(instance_id: impl Into<Arc<str>>, store: Arc<dyn SecretStore>) -> Self {
        Self {
            instance_id: instance_id.into(),
            store,
        }
    }

    fn storage_key(&self, key: &OAuthCredentialKey) -> String {
        oauth_credential_key(self.instance_id.as_ref(), &key.stable_id())
    }
}

#[async_trait]
impl OAuthCredentialStore for KeychainOAuthCredentialStore {
    async fn load(
        &self,
        key: &OAuthCredentialKey,
    ) -> Result<Option<String>, OAuthCredentialStoreError> {
        let store = self.store.clone();
        let storage_key = self.storage_key(key);
        tokio::task::spawn_blocking(move || store.get_secret(&storage_key))
            .await
            .map_err(|_| OAuthCredentialStoreError::Unavailable)?
            .map_err(|_| OAuthCredentialStoreError::OperationFailed)
    }

    async fn save(
        &self,
        key: &OAuthCredentialKey,
        value: &str,
    ) -> Result<(), OAuthCredentialStoreError> {
        let store = self.store.clone();
        let storage_key = self.storage_key(key);
        let value = value.to_string();
        tokio::task::spawn_blocking(move || store.set_secret(&storage_key, &value))
            .await
            .map_err(|_| OAuthCredentialStoreError::Unavailable)?
            .map_err(|_| OAuthCredentialStoreError::OperationFailed)
    }

    async fn delete(&self, key: &OAuthCredentialKey) -> Result<(), OAuthCredentialStoreError> {
        let store = self.store.clone();
        let storage_key = self.storage_key(key);
        tokio::task::spawn_blocking(move || store.delete_secret(&storage_key))
            .await
            .map_err(|_| OAuthCredentialStoreError::Unavailable)?
            .map_err(|_| OAuthCredentialStoreError::OperationFailed)
    }
}

pub async fn clear_oauth_credentials_for_config(
    instance_id: &str,
    secret_store: Arc<dyn SecretStore>,
    config: MCPServerConfig,
) -> Result<(), String> {
    let Some(config) = oauth_cleanup_config(config) else {
        return Ok(());
    };
    let bundle_id = resolve_bundle_id(&config);
    let manager = MCPServerManager::with_oauth_credential_store(Arc::new(
        KeychainOAuthCredentialStore::new(instance_id.to_string(), secret_store),
    ));
    manager
        .initialize(vec![config])
        .await
        .map_err(|error| error.to_string())?;
    manager
        .clear_oauth(&bundle_id)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keychain::{InMemorySecretStore, KeychainError};
    use a2c_smcp::smcp_computer::mcp_clients::model::{BundleId, HttpServerParameters};
    use a2c_smcp::smcp_computer::oauth::OAuthCredentialRecordKind;

    fn credential_key() -> OAuthCredentialKey {
        OAuthCredentialKey {
            bundle_id: BundleId::try_from("oauth-server").unwrap(),
            resource: "https://resource.example/mcp".to_string(),
            issuer: Some("https://issuer.example".to_string()),
            grant_fingerprint: "dynamic-authorization-code".to_string(),
            record_kind: OAuthCredentialRecordKind::Credentials,
        }
    }

    #[test]
    fn omitted_http_auth_uses_auto_interactive_oauth_and_materializes_offline_cleanup() {
        let config = HttpServerConfig::new(
            "legacy-auto",
            HttpServerParameters {
                url: "https://mcp.example.com/mcp".to_string(),
                headers: Default::default(),
            },
        );
        assert_eq!(
            effective_http_oauth(&config),
            Some(EffectiveHttpOAuth {
                automatic: true,
                interactive: true,
            })
        );

        let MCPServerConfig::Http(cleanup) =
            oauth_cleanup_config(MCPServerConfig::Http(config)).unwrap()
        else {
            panic!("cleanup config must remain HTTP");
        };
        assert_eq!(cleanup.auth_policy, Some(HttpAuthPolicy::OAuth));
        assert!(cleanup.oauth.is_some());
    }

    #[test]
    fn static_authorization_header_takes_precedence_over_legacy_auto_oauth() {
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "aUtHoRiZaTiOn".to_string(),
            "Bearer static-token".to_string(),
        );
        let config = HttpServerConfig::new(
            "static-auth",
            HttpServerParameters {
                url: "https://mcp.example.com/mcp".to_string(),
                headers,
            },
        );

        assert_eq!(effective_http_oauth(&config), None);
        assert!(oauth_cleanup_config(MCPServerConfig::Http(config)).is_none());
    }

    #[tokio::test]
    async fn persists_opaque_envelopes_and_isolates_computers() {
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let first = KeychainOAuthCredentialStore::new("computer-a", secrets.clone());
        let second = KeychainOAuthCredentialStore::new("computer-b", secrets);
        let key = credential_key();

        first.save(&key, "opaque-credentials").await.unwrap();
        let restarted = KeychainOAuthCredentialStore::new("computer-a", first.store.clone());
        assert_eq!(
            restarted.load(&key).await.unwrap().as_deref(),
            Some("opaque-credentials")
        );
        assert_eq!(second.load(&key).await.unwrap(), None);

        restarted.delete(&key).await.unwrap();
        assert_eq!(restarted.load(&key).await.unwrap(), None);
    }

    struct FailingSecretStore;

    impl SecretStore for FailingSecretStore {
        fn set_secret(&self, _key: &str, _secret: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Store("unavailable".to_string()))
        }

        fn get_secret(&self, _key: &str) -> Result<Option<String>, KeychainError> {
            Err(KeychainError::Store("unavailable".to_string()))
        }

        fn delete_secret(&self, _key: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Store("unavailable".to_string()))
        }
    }

    #[tokio::test]
    async fn backend_failures_never_fall_back_to_memory() {
        let store = KeychainOAuthCredentialStore::new("computer-a", Arc::new(FailingSecretStore));
        let key = credential_key();

        assert_eq!(
            store.load(&key).await,
            Err(OAuthCredentialStoreError::OperationFailed)
        );
        assert_eq!(
            store.save(&key, "opaque").await,
            Err(OAuthCredentialStoreError::OperationFailed)
        );
        assert_eq!(
            store.delete(&key).await,
            Err(OAuthCredentialStoreError::OperationFailed)
        );
    }
}
