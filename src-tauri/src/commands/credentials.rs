use crate::services::keychain::{self, PausedCredential};

#[tauri::command]
pub fn list_paused_credentials() -> Vec<PausedCredential> {
    keychain::list_paused_credentials()
}

#[tauri::command]
pub fn retry_credential_access(id: String) -> Result<(), String> {
    keychain::retry_credential_access(id)
}
