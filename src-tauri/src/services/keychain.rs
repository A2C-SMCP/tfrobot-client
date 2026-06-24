use keyring::Entry;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_secret_store_round_trips_without_system_keychain() {
        let store = InMemorySecretStore::default();

        store.set_secret("key", "secret").unwrap();
        assert_eq!(store.get_secret("key").unwrap().as_deref(), Some("secret"));

        store.delete_secret("key").unwrap();
        assert_eq!(store.get_secret("key").unwrap(), None);
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
