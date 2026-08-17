use keyring::Entry;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

const SERVICE_NAME: &str = "tfrobot-client";

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("Secret store error: {0}")]
    Store(String),
}

pub trait SecretStore: Send + Sync {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError>;
    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError>;
    fn delete_secret(&self, key: &str) -> Result<(), KeychainError>;

    fn delete_secret_best_effort(&self, key: &str) {
        if let Err(error) = self.delete_secret(key) {
            log::warn!("Failed to delete keychain secret {key}: {error}");
        }
    }
}

#[derive(Debug, Default)]
pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        set_secret(key, secret)
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        get_secret(key)
    }

    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        delete_secret(key)
    }
}

#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl InMemorySecretStore {
    pub fn shared() -> Arc<dyn SecretStore> {
        Arc::new(Self::default())
    }
}

impl SecretStore for InMemorySecretStore {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        self.secrets
            .lock()
            .map_err(|error| KeychainError::Store(error.to_string()))?
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        Ok(self
            .secrets
            .lock()
            .map_err(|error| KeychainError::Store(error.to_string()))?
            .get(key)
            .cloned())
    }

    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        self.secrets
            .lock()
            .map_err(|error| KeychainError::Store(error.to_string()))?
            .remove(key);
        Ok(())
    }
}

pub fn save_credential(server_url: &str, api_key: &str) -> Result<(), KeychainError> {
    set_secret(server_url, api_key)
}

pub fn get_credential(server_url: &str) -> Result<Option<String>, KeychainError> {
    get_secret(server_url)
}

pub fn delete_credential(server_url: &str) -> Result<(), KeychainError> {
    delete_secret(server_url)
}

pub fn set_secret(key: &str, secret: &str) -> Result<(), KeychainError> {
    let entry = Entry::new(SERVICE_NAME, key)?;
    entry.set_password(secret)?;
    Ok(())
}

pub fn get_secret(key: &str) -> Result<Option<String>, KeychainError> {
    let entry = Entry::new(SERVICE_NAME, key)?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_secret(key: &str) -> Result<(), KeychainError> {
    let entry = Entry::new(SERVICE_NAME, key)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_secret_best_effort(key: &str) {
    SystemSecretStore.delete_secret_best_effort(key);
}

pub fn input_secret_key(instance_id: &str, input_id: &str) -> String {
    scoped_input_key("input-secret", instance_id, input_id)
}

pub fn secret_value_key(secret_id: &str) -> String {
    scoped_secret_key("secret", secret_id)
}

/// Stable per-Computer key for an opaque SDK OAuth credential record.
///
/// The SDK-provided identifier already separates bundle, resource, issuer, grant mode, and
/// record kind. Hashing it together with the trusted Computer instance ID adds the host-owned
/// namespace without exposing either identifier to platform keyring metadata.
pub fn oauth_credential_key(instance_id: &str, sdk_stable_id: &str) -> String {
    scoped_input_key("mcp-oauth", instance_id, sdk_stable_id)
}

pub fn set_input_secret(
    store: &dyn SecretStore,
    instance_id: &str,
    input_id: &str,
    secret: &str,
) -> Result<(), KeychainError> {
    store.set_secret(&input_secret_key(instance_id, input_id), secret)
}

pub fn get_input_secret(
    store: &dyn SecretStore,
    instance_id: &str,
    input_id: &str,
) -> Result<Option<String>, KeychainError> {
    store.get_secret(&input_secret_key(instance_id, input_id))
}

pub fn delete_input_secret(
    store: &dyn SecretStore,
    instance_id: &str,
    input_id: &str,
) -> Result<(), KeychainError> {
    store.delete_secret(&input_secret_key(instance_id, input_id))
}

fn scoped_input_key(namespace: &str, instance_id: &str, input_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(instance_id.as_bytes());
    digest.update([0]);
    digest.update(input_id.as_bytes());
    let digest = digest.finalize();
    format!("{namespace}:{}", hex::encode(&digest[..16]))
}

fn scoped_secret_key(namespace: &str, logical_id: &str) -> String {
    let digest = Sha256::digest(logical_id.as_bytes());
    format!("{namespace}:{}", hex::encode(&digest[..16]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::connection_targets::manual_target_keychain_id;

    #[test]
    fn in_memory_secret_store_round_trips_without_system_keychain() {
        let store = InMemorySecretStore::default();

        store.set_secret("key", "secret").unwrap();
        assert_eq!(store.get_secret("key").unwrap().as_deref(), Some("secret"));

        store.delete_secret("key").unwrap();
        assert_eq!(store.get_secret("key").unwrap(), None);
    }

    #[test]
    fn keychain_secret_namespaces_are_stable_and_isolated() {
        assert_ne!(
            input_secret_key("computer-a", "api-key"),
            input_secret_key("computer-b", "api-key")
        );
        assert_ne!(
            input_secret_key("computer-a", "api-key"),
            secret_value_key("api-key")
        );
        assert_eq!(
            oauth_credential_key("computer-a", "sdk-record"),
            oauth_credential_key("computer-a", "sdk-record")
        );
        assert_ne!(
            oauth_credential_key("computer-a", "sdk-record"),
            oauth_credential_key("computer-b", "sdk-record")
        );
        assert_ne!(
            oauth_credential_key("computer-a", "sdk-record"),
            oauth_credential_key("computer-a", "other-record")
        );
        assert!(!input_secret_key("computer-a", "path/with spaces").contains("path/with spaces"));
    }

    #[test]
    fn input_secrets_roundtrip_without_json_serialization() {
        let store = InMemorySecretStore::default();

        set_input_secret(&store, "computer-a", "api-key", "top-secret").unwrap();
        assert_eq!(
            get_input_secret(&store, "computer-a", "api-key")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        delete_input_secret(&store, "computer-a", "api-key").unwrap();
        assert_eq!(
            get_input_secret(&store, "computer-a", "api-key").unwrap(),
            None
        );
    }

    #[test]
    fn client_owned_secret_namespaces_roundtrip_and_delete_independently() {
        let store = InMemorySecretStore::default();
        let secrets = [
            (
                input_secret_key("computer-a", "credentials"),
                "input-secret",
            ),
            (secret_value_key("shared-token"), "explicit-secret"),
            (
                manual_target_keychain_id("target-a"),
                "manual-target-api-key",
            ),
        ];

        for (key, value) in &secrets {
            store.set_secret(key, value).unwrap();
        }
        for (key, value) in &secrets {
            assert_eq!(store.get_secret(key).unwrap().as_deref(), Some(*value));
        }

        for (key, _) in &secrets {
            store.delete_secret(key).unwrap();
            assert_eq!(store.get_secret(key).unwrap(), None);
        }
    }

    #[test]
    #[ignore = "uses the system keychain and may require user approval"]
    fn test_credential_operations() {
        let server_url = format!("https://test-server.example.com/{}", uuid::Uuid::new_v4());
        let api_key = "test-api-key-12345";

        save_credential(&server_url, api_key).unwrap();

        let retrieved = get_credential(&server_url).unwrap();
        assert_eq!(retrieved, Some(api_key.to_string()));

        delete_credential(&server_url).unwrap();

        let after_delete = get_credential(&server_url).unwrap();
        assert_eq!(after_delete, None);
    }
}
