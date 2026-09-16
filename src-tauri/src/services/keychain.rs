use keyring::Entry;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use thiserror::Error;

const SERVICE_NAME: &str = "tfrobot-client";

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("Secret store error: {0}")]
    Store(String),

    #[error("Credential access is paused. Use Retry credential access, then repeat your action.")]
    AccessPaused,

    #[error(transparent)]
    Shared(Arc<KeychainError>),
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialContext {
    pub computer_id: Option<String>,
    pub resource_id: String,
}

pub trait SecretStore: Send + Sync {
    fn describe(&self, _key: &str, _context: CredentialContext) {}

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
    fn describe(&self, key: &str, context: CredentialContext) {
        SYSTEM_STORE.describe(key, context);
    }
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        system_access(|| SYSTEM_STORE.set_secret(key, secret))
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        system_access(|| SYSTEM_STORE.get_secret(key))
    }

    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        system_access(|| SYSTEM_STORE.delete_secret(key))
    }
}

// Legacy synchronous SecretStore consumers also run inside Tauri's multithread runtime.
// Let Tokio replace the worker while an OS prompt or concurrent flight is waiting.
fn system_access<T>(operation: impl FnOnce() -> T) -> T {
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(operation)
    } else {
        operation()
    }
}

/// Only concurrent readers share a result. Once the flight completes, its secret is dropped
/// with the last waiter. Later operations read the OS again, including connection commit
/// validation, so changes made outside this process remain observable.
type SharedRead = Result<Option<String>, Arc<KeychainError>>;
#[derive(Default)]
struct SecretFlight {
    result: Mutex<Option<SharedRead>>,
    ready: Condvar,
    #[cfg(test)]
    waiters: (Mutex<usize>, Condvar),
}
impl SecretFlight {
    fn wait(&self) -> SharedRead {
        #[cfg(test)]
        {
            *self.waiters.0.lock().unwrap() += 1;
            self.waiters.1.notify_all();
        }
        let mut result = self.result.lock().unwrap_or_else(|e| e.into_inner());
        while result.is_none() {
            result = self.ready.wait(result).unwrap_or_else(|e| e.into_inner());
        }
        result.as_ref().unwrap().clone()
    }
    fn finish(&self, result: SharedRead) {
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        self.ready.notify_all();
    }
}
#[derive(Default)]
struct SecretSlot {
    active: Option<(bool, Arc<SecretFlight>)>,
    paused: Option<PausedCredential>,
    context: Option<CredentialContext>,
}

#[derive(Clone, serde::Serialize)]
pub struct PausedCredential {
    pub id: String,
    pub purpose: &'static str,
    pub context: Option<CredentialContext>,
}

type AccessListener = Arc<dyn Fn() + Send + Sync>;
struct CoordinatedSecretStore {
    inner: Arc<dyn SecretStore>,
    slots: Mutex<HashMap<String, Arc<Mutex<SecretSlot>>>>,
    listener: Mutex<Option<AccessListener>>,
}
impl CoordinatedSecretStore {
    fn new(inner: Arc<dyn SecretStore>) -> Self {
        Self {
            inner,
            slots: Mutex::default(),
            listener: Mutex::default(),
        }
    }
    fn slot(&self, key: &str) -> Arc<Mutex<SecretSlot>> {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.to_owned())
            .or_default()
            .clone()
    }
    fn notify(&self) {
        let listener = self
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(listener) = listener {
            listener();
        }
    }
    fn paused(&self) -> Vec<PausedCredential> {
        let slots: Vec<_> = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        slots
            .into_iter()
            .filter_map(|slot| {
                slot.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .paused
                    .clone()
            })
            .collect()
    }
    fn retry(&self, id: &str) -> bool {
        let slots: Vec<_> = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        for slot in slots {
            let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
            if slot.paused.as_ref().is_some_and(|paused| paused.id == id) {
                slot.paused = None;
                drop(slot);
                self.notify();
                return true;
            }
        }
        false
    }
    fn run(
        &self,
        key: &str,
        read: bool,
        operation: impl FnOnce() -> Result<Option<String>, KeychainError>,
    ) -> Result<Option<String>, KeychainError> {
        let slot = self.slot(key);
        let flight = loop {
            let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
            if state.paused.is_some() {
                return Err(KeychainError::AccessPaused);
            }
            if let Some((active_read, active)) = &state.active {
                let share = read && *active_read;
                let active = active.clone();
                drop(state);
                let result = active.wait();
                if share {
                    return result.map_err(KeychainError::Shared);
                }
                continue;
            }
            let flight = Arc::new(SecretFlight::default());
            state.active = Some((read, flight.clone()));
            break flight;
        };
        let result = operation().map_err(Arc::new);
        let paused = result.as_ref().err().is_some_and(|error| {
            matches!(
                error.as_ref(),
                KeychainError::Keyring(
                    keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_)
                )
            )
        });
        {
            let mut state = slot.lock().unwrap_or_else(|e| e.into_inner());
            if paused {
                state.paused = Some(PausedCredential {
                    id: uuid::Uuid::new_v4().to_string(),
                    purpose: credential_purpose(key),
                    context: state.context.clone(),
                });
            }
            flight.finish(result.clone());
            state.active = None;
        }
        if paused {
            self.notify();
        }
        result.map_err(KeychainError::Shared)
    }
}
impl SecretStore for CoordinatedSecretStore {
    fn describe(&self, key: &str, context: CredentialContext) {
        self.slot(key)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .context = Some(context);
    }
    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        self.run(key, true, || self.inner.get_secret(key))
    }
    fn set_secret(&self, key: &str, value: &str) -> Result<(), KeychainError> {
        self.run(key, false, || {
            self.inner.set_secret(key, value).map(|()| None)
        })
        .map(|_| ())
    }
    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        self.run(key, false, || self.inner.delete_secret(key).map(|()| None))
            .map(|_| ())
    }
}
fn credential_purpose(key: &str) -> &'static str {
    if key.starts_with("mcp-oauth:") {
        "oauth"
    } else if key.starts_with("input-secret:") || key.starts_with("secret:") {
        "input"
    } else {
        "connection"
    }
}
struct RawSystemSecretStore;
impl SecretStore for RawSystemSecretStore {
    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        raw_get_secret(key)
    }
    fn set_secret(&self, key: &str, value: &str) -> Result<(), KeychainError> {
        raw_set_secret(key, value)
    }
    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        raw_delete_secret(key)
    }
}
static SYSTEM_STORE: LazyLock<CoordinatedSecretStore> =
    LazyLock::new(|| CoordinatedSecretStore::new(Arc::new(RawSystemSecretStore)));

/// An explicit business-operation snapshot, discarded by its owner at operation completion.
/// Never use this wrapper for connection commit validation or persistent runtime state.
pub struct OperationSecretStore {
    inner: Arc<dyn SecretStore>,
    values: Mutex<HashMap<String, Option<String>>>,
}
impl OperationSecretStore {
    pub fn shared(inner: Arc<dyn SecretStore>) -> Arc<dyn SecretStore> {
        Arc::new(Self {
            inner,
            values: Mutex::default(),
        })
    }
}
impl SecretStore for OperationSecretStore {
    fn describe(&self, key: &str, context: CredentialContext) {
        self.inner.describe(key, context);
    }
    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| KeychainError::Store("Credential operation lock failed".into()))?;
        if let Some(value) = values.get(key) {
            return Ok(value.clone());
        }
        let value = self.inner.get_secret(key)?;
        values.insert(key.to_string(), value.clone());
        Ok(value)
    }
    fn set_secret(&self, key: &str, value: &str) -> Result<(), KeychainError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| KeychainError::Store("Credential operation lock failed".into()))?;
        values.remove(key);
        self.inner.set_secret(key, value)
    }
    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| KeychainError::Store("Credential operation lock failed".into()))?;
        values.remove(key);
        self.inner.delete_secret(key)
    }
}

pub async fn read_secret_async(
    store: Arc<dyn SecretStore>,
    key: String,
) -> Result<Option<String>, KeychainError> {
    tokio::task::spawn_blocking(move || store.get_secret(&key))
        .await
        .map_err(|_| KeychainError::Store("Credential read task failed".into()))?
}

pub fn set_access_listener(listener: AccessListener) {
    *SYSTEM_STORE
        .listener
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(listener);
}

pub fn list_paused_credentials() -> Vec<PausedCredential> {
    SYSTEM_STORE.paused()
}

/// Re-enables only the selected credential. Does not replay a failed mutation or connection.
pub fn retry_credential_access(id: String) -> Result<(), String> {
    if SYSTEM_STORE.retry(&id) {
        Ok(())
    } else {
        Err("Credential access request is no longer pending".to_string())
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
    SystemSecretStore.set_secret(key, secret)
}

pub fn get_secret(key: &str) -> Result<Option<String>, KeychainError> {
    SystemSecretStore.get_secret(key)
}

pub fn delete_secret(key: &str) -> Result<(), KeychainError> {
    SystemSecretStore.delete_secret(key)
}

fn raw_set_secret(key: &str, secret: &str) -> Result<(), KeychainError> {
    let entry = Entry::new(SERVICE_NAME, key)?;
    entry.set_password(secret)?;
    Ok(())
}

fn raw_get_secret(key: &str) -> Result<Option<String>, KeychainError> {
    let entry = Entry::new(SERVICE_NAME, key)?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn raw_delete_secret(key: &str) -> Result<(), KeychainError> {
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
    store.describe(
        &input_secret_key(instance_id, input_id),
        CredentialContext {
            computer_id: Some(instance_id.to_string()),
            resource_id: input_id.to_string(),
        },
    );
    store.set_secret(&input_secret_key(instance_id, input_id), secret)
}

pub fn get_input_secret(
    store: &dyn SecretStore,
    instance_id: &str,
    input_id: &str,
) -> Result<Option<String>, KeychainError> {
    store.describe(
        &input_secret_key(instance_id, input_id),
        CredentialContext {
            computer_id: Some(instance_id.to_string()),
            resource_id: input_id.to_string(),
        },
    );
    store.get_secret(&input_secret_key(instance_id, input_id))
}

pub fn delete_input_secret(
    store: &dyn SecretStore,
    instance_id: &str,
    input_id: &str,
) -> Result<(), KeychainError> {
    store.describe(
        &input_secret_key(instance_id, input_id),
        CredentialContext {
            computer_id: Some(instance_id.to_string()),
            resource_id: input_id.to_string(),
        },
    );
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

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingStore {
        inner: InMemorySecretStore,
        reads: AtomicUsize,
        denied: AtomicBool,
    }
    impl SecretStore for CountingStore {
        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.denied.load(Ordering::SeqCst) && key.starts_with("blocked") {
                return Err(KeychainError::Keyring(keyring::Error::NoStorageAccess(
                    Box::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
                )));
            }
            self.inner.get_secret(key)
        }
        fn set_secret(&self, key: &str, value: &str) -> Result<(), KeychainError> {
            self.inner.set_secret(key, value)
        }
        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            self.inner.delete_secret(key)
        }
    }

    #[test]
    fn failed_access_stops_background_attempts_until_explicit_scoped_retry() {
        let raw = Arc::new(CountingStore::default());
        raw.denied.store(true, Ordering::SeqCst);
        let store = CoordinatedSecretStore::new(raw.clone());
        assert!(store.get_secret("blocked").is_err());
        let pending = store.paused();
        assert_eq!(pending.len(), 1);
        assert!(matches!(
            store.get_secret("blocked"),
            Err(KeychainError::AccessPaused)
        ));
        assert!(store.set_secret("blocked", "new").is_err());
        assert!(store.delete_secret("blocked").is_err());
        assert_eq!(raw.reads.load(Ordering::SeqCst), 1);
        assert_eq!(store.get_secret("other").unwrap(), None);
        assert!(!store.retry("unknown"));
        raw.denied.store(false, Ordering::SeqCst);
        assert!(store.retry(&pending[0].id));
        assert!(!store.retry(&pending[0].id));
        assert_eq!(
            raw.reads.load(Ordering::SeqCst),
            2,
            "retry must not replay an operation"
        );
        assert_eq!(store.get_secret("blocked").unwrap(), None);
        assert!(store.paused().is_empty());
    }

    #[test]
    fn paused_credentials_keep_business_context_and_retry_isolated_keys() {
        let raw = Arc::new(CountingStore::default());
        raw.denied.store(true, Ordering::SeqCst);
        let store = CoordinatedSecretStore::new(raw);
        for (key, resource) in [("blocked-a", "Calendar"), ("blocked-b", "Files")] {
            store.describe(
                key,
                CredentialContext {
                    computer_id: Some("work".into()),
                    resource_id: resource.into(),
                },
            );
            assert!(store.get_secret(key).is_err());
        }
        let pending = store.paused();
        let calendar = pending
            .iter()
            .find(|entry| entry.context.as_ref().unwrap().resource_id == "Calendar")
            .unwrap();
        assert!(store.retry(&calendar.id));
        assert_eq!(store.paused().len(), 1);
        assert_eq!(
            store.paused()[0].context.as_ref().unwrap().resource_id,
            "Files"
        );
        assert!(matches!(
            store.get_secret("blocked-b"),
            Err(KeychainError::AccessPaused)
        ));
    }

    #[test]
    fn operation_snapshot_drops_after_operation_and_invalidates_on_mutations() {
        let raw = Arc::new(CountingStore::default());
        raw.set_secret("key", "old").unwrap();
        let operation = OperationSecretStore::shared(raw.clone());
        assert_eq!(operation.get_secret("key").unwrap().as_deref(), Some("old"));
        assert_eq!(operation.get_secret("key").unwrap().as_deref(), Some("old"));
        assert_eq!(raw.reads.load(Ordering::SeqCst), 1);
        operation.set_secret("key", "new").unwrap();
        assert_eq!(operation.get_secret("key").unwrap().as_deref(), Some("new"));
        operation.delete_secret("key").unwrap();
        assert_eq!(operation.get_secret("key").unwrap(), None);
        drop(operation);
        raw.set_secret("key", "external").unwrap();
        let next = OperationSecretStore::shared(raw.clone());
        assert_eq!(next.get_secret("key").unwrap().as_deref(), Some("external"));
    }

    #[test]
    fn concurrent_readers_share_one_flight_but_next_read_observes_external_change() {
        struct GatedStore {
            inner: CountingStore,
            entered: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl SecretStore for GatedStore {
            fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
                if self.inner.reads.load(Ordering::SeqCst) == 0 {
                    self.entered.send(()).unwrap();
                    self.release.lock().unwrap().recv().unwrap();
                }
                self.inner.get_secret(key)
            }
            fn set_secret(&self, key: &str, value: &str) -> Result<(), KeychainError> {
                self.inner.set_secret(key, value)
            }
            fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
                self.inner.delete_secret(key)
            }
        }
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let raw = Arc::new(GatedStore {
            inner: CountingStore::default(),
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });
        raw.set_secret("key", "before").unwrap();
        let store = Arc::new(CoordinatedSecretStore::new(raw.clone()));
        let first = {
            let store = store.clone();
            std::thread::spawn(move || store.get_secret("key").unwrap())
        };
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let flight = store
            .slot("key")
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .1
            .clone();
        let second = {
            let store = store.clone();
            std::thread::spawn(move || store.get_secret("key").unwrap())
        };
        let (waiters, timeout) = flight
            .waiters
            .1
            .wait_timeout_while(
                flight.waiters.0.lock().unwrap(),
                std::time::Duration::from_secs(5),
                |count| *count == 0,
            )
            .unwrap();
        assert!(!timeout.timed_out());
        drop(waiters);
        release_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap().as_deref(), Some("before"));
        assert_eq!(second.join().unwrap().as_deref(), Some("before"));
        assert_eq!(raw.inner.reads.load(Ordering::SeqCst), 1);
        raw.set_secret("key", "external").unwrap();
        assert_eq!(
            store.get_secret("key").unwrap().as_deref(),
            Some("external")
        );
        assert_eq!(raw.inner.reads.load(Ordering::SeqCst), 2);
    }

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
