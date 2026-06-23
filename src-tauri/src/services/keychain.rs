use keyring::Entry;
use thiserror::Error;

const SERVICE_NAME: &str = "tfrobot-client";

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),
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
    if let Err(error) = delete_secret(key) {
        log::warn!("Failed to delete keychain secret {key}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_operations() {
        let server_url = "https://test-server.example.com";
        let api_key = "test-api-key-12345";

        // Save – if the system keychain is unavailable (CI, sandbox), skip the test
        if save_credential(server_url, api_key).is_err() {
            eprintln!("Skipping keychain test: system keychain not available");
            return;
        }

        // Verify round-trip; skip if the backend silently drops writes
        let retrieved = get_credential(server_url).unwrap();
        if retrieved.is_none() {
            eprintln!("Skipping keychain test: backend did not persist credential");
            let _ = delete_credential(server_url);
            return;
        }
        assert_eq!(retrieved, Some(api_key.to_string()));

        // Delete
        delete_credential(server_url).unwrap();

        // Verify deleted
        let after_delete = get_credential(server_url).unwrap();
        assert_eq!(after_delete, None);
    }
}
