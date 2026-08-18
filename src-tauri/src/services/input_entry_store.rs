use crate::services::config::ConfigService;
use crate::services::input_value_index::{
    self, InputValueIndexProvenance, InputValueStorageKind, LoadedInputValueIndex,
};
use crate::services::input_value_store::InputValueStore;
use crate::services::keychain::{self, SecretStore};
use crate::services::storage::write_json_atomically;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

const FILE_NAME: &str = "input_entries.json";
const SCHEMA_VERSION: u32 = 1;

type RepositoryLock = Mutex<()>;
static REPOSITORY_LOCKS: LazyLock<Mutex<BTreeMap<PathBuf, Weak<RepositoryLock>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputEntryStorageKind {
    Value,
    Secret,
}

impl InputEntryStorageKind {
    pub fn is_secret(self) -> bool {
        matches!(self, Self::Secret)
    }

    fn opposite(self) -> Self {
        match self {
            Self::Value => Self::Secret,
            Self::Secret => Self::Value,
        }
    }
}

impl From<InputValueStorageKind> for InputEntryStorageKind {
    fn from(value: InputValueStorageKind) -> Self {
        match value {
            InputValueStorageKind::Value => Self::Value,
            InputValueStorageKind::Secret => Self::Secret,
        }
    }
}

impl From<InputEntryStorageKind> for InputValueStorageKind {
    fn from(value: InputEntryStorageKind) -> Self {
        match value {
            InputEntryStorageKind::Value => Self::Value,
            InputEntryStorageKind::Secret => Self::Secret,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputEntryView {
    pub key: String,
    pub secret: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedInputEntry {
    pub value: Value,
    pub storage_kind: InputEntryStorageKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct InputEntriesDocument {
    schema_version: u32,
    #[serde(default)]
    entries: BTreeMap<String, InputEntryStorageKind>,
}

/// Authoritative client-owned InputEntry repository.
///
/// The metadata document contains only `(key, storage kind)`. Plain values remain in the
/// per-Computer value store and secret plaintext remains exclusively in the OS Keychain.
#[derive(Clone)]
pub struct InputEntryStore {
    instance_id: Arc<str>,
    storage_root: PathBuf,
    path: PathBuf,
    values: InputValueStore,
    secrets: Arc<dyn SecretStore>,
}

impl InputEntryStore {
    pub fn for_computer(
        config: &ConfigService,
        instance_id: impl Into<Arc<str>>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        let instance_id = instance_id.into();
        Self::from_storage_root(
            instance_id.clone(),
            config.computer_instance_storage_root(instance_id.as_ref()),
            secrets,
        )
    }

    pub fn from_storage_root(
        instance_id: impl Into<Arc<str>>,
        storage_root: impl AsRef<Path>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        let storage_root = storage_root.as_ref().to_path_buf();
        Self {
            instance_id: instance_id.into(),
            path: storage_root.join(FILE_NAME),
            values: InputValueStore::from_storage_root(&storage_root),
            storage_root,
            secrets,
        }
    }

    pub fn list(&self) -> Result<Vec<InputEntryView>, String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        let document = self.load()?;
        document
            .entries
            .into_iter()
            .map(|(key, kind)| self.view_for(key, kind))
            .collect()
    }

    pub fn get(&self, key: &str) -> Result<Option<InputEntryView>, String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        let document = self.load()?;
        document
            .entries
            .get(key)
            .copied()
            .map(|kind| self.view_for(key.to_string(), kind))
            .transpose()
    }

    pub fn storage_kind(&self, key: &str) -> Result<Option<InputEntryStorageKind>, String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        Ok(self.load()?.entries.get(key).copied())
    }

    /// Resolves an entry. `legacy_preference` is consulted only when upgrading pre-InputEntry
    /// storage that has no authoritative metadata yet; it never overrides an existing entry.
    pub fn resolve(
        &self,
        key: &str,
        legacy_preference: InputEntryStorageKind,
    ) -> Result<Option<Value>, String> {
        Ok(self
            .resolve_entry(key, legacy_preference)?
            .map(|entry| entry.value))
    }

    pub fn resolve_entry(
        &self,
        key: &str,
        legacy_preference: InputEntryStorageKind,
    ) -> Result<Option<ResolvedInputEntry>, String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        if let Some(kind) = self.load()?.entries.get(key).copied() {
            return self.read_value(key, kind).map(|value| {
                value.map(|value| ResolvedInputEntry {
                    value,
                    storage_kind: kind,
                })
            });
        }
        let legacy_index =
            input_value_index::load_with_provenance_from_storage_root(&self.storage_root)?;
        let Some(indexed_kinds) = legacy_index.entries.get(key) else {
            let Some(value) = self.read_value(key, legacy_preference)? else {
                return Ok(None);
            };
            self.adopt_legacy_locked(key, legacy_preference, true)?;
            return Ok(Some(ResolvedInputEntry {
                value,
                storage_kind: legacy_preference,
            }));
        };
        let Some(legacy_kind) =
            self.select_indexed_legacy_kind_locked(key, indexed_kinds, Some(legacy_preference))?
        else {
            return Ok(None);
        };
        let Some(value) = self.read_value(key, legacy_kind)? else {
            return Ok(None);
        };
        self.adopt_legacy_locked(
            key,
            legacy_kind,
            legacy_index.provenance != InputValueIndexProvenance::LegacyV1,
        )?;
        Ok(Some(ResolvedInputEntry {
            value,
            storage_kind: legacy_kind,
        }))
    }

    /// Creates or updates one InputEntry. `value = None` preserves the current value and is valid
    /// only for an existing entry (used to migrate a secret without returning its plaintext to UI).
    pub fn upsert(&self, key: &str, value: Option<Value>, secret: bool) -> Result<(), String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        validate_key(key)?;
        if let Some(value) = value.as_ref() {
            if !value.is_string() {
                return Err(format!("InputEntry '{key}' value must be a string"));
            }
        }

        let previous_document = self.load()?;
        let previous_kind = previous_document.entries.get(key).copied();
        let next_kind = if secret {
            InputEntryStorageKind::Secret
        } else {
            InputEntryStorageKind::Value
        };
        let touches_plain = next_kind == InputEntryStorageKind::Value
            || previous_kind == Some(InputEntryStorageKind::Value);
        let touches_secret = next_kind == InputEntryStorageKind::Secret
            || previous_kind == Some(InputEntryStorageKind::Secret);
        let previous_plain = touches_plain.then(|| self.values.get(key)).transpose()?;
        let previous_secret = touches_secret.then(|| self.read_secret(key)).transpose()?;
        let next_value = match value {
            Some(value) => value,
            None => {
                let kind = previous_kind.ok_or_else(|| {
                    format!("InputEntry '{key}' requires a value when it is created")
                })?;
                self.read_value(key, kind)?.ok_or_else(|| {
                    format!("InputEntry '{key}' metadata exists but its value is missing")
                })?
            }
        };

        let mutation: Result<(), String> = (|| {
            self.write_value(key, next_kind, &next_value)?;
            let mut next_document = previous_document.clone();
            next_document.entries.insert(key.to_string(), next_kind);
            self.save(&next_document)?;
            if previous_kind == Some(next_kind.opposite()) {
                self.delete_value(key, next_kind.opposite())?;
            }
            Ok(())
        })();

        if let Err(primary) = mutation {
            return match self.restore_snapshot(
                key,
                &previous_document,
                previous_plain,
                previous_secret,
            ) {
                Ok(()) => Err(format!(
                    "Failed to save InputEntry '{key}'; changes were reverted: {primary}"
                )),
                Err(rollback) => Err(format!(
                    "Failed to save InputEntry '{key}': {primary}; rollback also failed: {rollback}"
                )),
            };
        }
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<(), String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        validate_key(key)?;
        let previous_document = self.load()?;
        if !previous_document.entries.contains_key(key) {
            return Err(format!("InputEntry not found: {key}"));
        }
        let previous_kind = previous_document.entries[key];
        let previous_plain = (previous_kind == InputEntryStorageKind::Value)
            .then(|| self.values.get(key))
            .transpose()?;
        let previous_secret = (previous_kind == InputEntryStorageKind::Secret)
            .then(|| self.read_secret(key))
            .transpose()?;
        let mutation: Result<(), String> = (|| {
            self.delete_value(key, previous_kind)?;
            let mut next_document = previous_document.clone();
            next_document.entries.remove(key);
            self.save(&next_document)
        })();
        if let Err(primary) = mutation {
            return match self.restore_snapshot(
                key,
                &previous_document,
                previous_plain,
                previous_secret,
            ) {
                Ok(()) => Err(format!(
                    "Failed to delete InputEntry '{key}'; changes were reverted: {primary}"
                )),
                Err(rollback) => Err(format!(
                    "Failed to delete InputEntry '{key}': {primary}; rollback also failed: {rollback}"
                )),
            };
        }
        Ok(())
    }

    /// Reconciles legacy value/index storage once. Current SDK definitions are used only as the
    /// tie-breaker for old dual namespaces; after metadata is written, definitions have no storage
    /// authority.
    pub fn migrate_legacy(
        &self,
        legacy_index: LoadedInputValueIndex,
        preferred: &BTreeMap<String, InputEntryStorageKind>,
    ) -> Result<(), String> {
        let operation_lock = self.operation_lock()?;
        let _guard = lock_repository(&operation_lock)?;
        let mut keys: BTreeSet<String> = self.values.list()?.into_keys().collect();
        keys.extend(legacy_index.entries.keys().cloned());
        for key in keys {
            if self.load()?.entries.contains_key(&key) {
                continue;
            }
            let kind = match legacy_index.entries.get(&key) {
                Some(indexed_kinds) => self.select_indexed_legacy_kind_locked(
                    &key,
                    indexed_kinds,
                    preferred.get(&key).copied(),
                )?,
                None => self
                    .values
                    .get(&key)?
                    .is_some()
                    .then_some(InputEntryStorageKind::Value),
            };
            let Some(kind) = kind else { continue };
            self.adopt_legacy_locked(
                &key,
                kind,
                legacy_index.provenance != InputValueIndexProvenance::LegacyV1,
            )?;
        }
        Ok(())
    }

    /// Chooses a legacy backend from the V2 index without expanding its authority. A singleton
    /// kind wins even when an unindexed stale copy physically exists in the opposite backend.
    /// The SDK definition is only a tie-breaker when the index itself records both kinds.
    fn select_indexed_legacy_kind_locked(
        &self,
        key: &str,
        indexed_kinds: &BTreeSet<InputValueStorageKind>,
        preferred: Option<InputEntryStorageKind>,
    ) -> Result<Option<InputEntryStorageKind>, String> {
        let indexed_value = indexed_kinds.contains(&InputValueStorageKind::Value);
        let indexed_secret = indexed_kinds.contains(&InputValueStorageKind::Secret);
        match (indexed_value, indexed_secret) {
            (true, false) => Ok(self
                .values
                .get(key)?
                .is_some()
                .then_some(InputEntryStorageKind::Value)),
            (false, true) => Ok(self
                .read_secret(key)?
                .is_some()
                .then_some(InputEntryStorageKind::Secret)),
            (true, true) => {
                let plain_exists = self.values.get(key)?.is_some();
                let secret_exists = self.read_secret(key)?.is_some();
                Ok(match (plain_exists, secret_exists, preferred) {
                    (true, true, Some(preferred)) => Some(preferred),
                    (true, true, None) => Some(InputEntryStorageKind::Secret),
                    (true, false, _) => Some(InputEntryStorageKind::Value),
                    (false, true, _) => Some(InputEntryStorageKind::Secret),
                    (false, false, _) => None,
                })
            }
            (false, false) => Ok(None),
        }
    }

    fn view_for(&self, key: String, kind: InputEntryStorageKind) -> Result<InputEntryView, String> {
        // Management projection is metadata-first so a dangling Entry remains visible and
        // deletable. Secret rows must never read Keychain plaintext merely to render a list.
        let value = if kind.is_secret() {
            None
        } else {
            self.values.get(&key)?
        };
        Ok(InputEntryView {
            key,
            secret: kind.is_secret(),
            value,
        })
    }

    fn adopt_legacy_locked(
        &self,
        key: &str,
        kind: InputEntryStorageKind,
        cleanup_opposite: bool,
    ) -> Result<(), String> {
        let previous_document = self.load()?;
        if previous_document.entries.contains_key(key) {
            return Ok(());
        }
        if self.read_value(key, kind)?.is_none() {
            return Ok(());
        }
        let mut next_document = previous_document.clone();
        next_document.entries.insert(key.to_string(), kind);
        self.save(&next_document)?;
        if !cleanup_opposite {
            return Ok(());
        }
        if let Err(error) = self.delete_value(key, kind.opposite()) {
            self.save(&previous_document).map_err(|rollback| {
                format!(
                    "Failed to migrate legacy InputEntry '{key}': {error}; metadata rollback also failed: {rollback}"
                )
            })?;
            return Err(format!(
                "Failed to migrate legacy InputEntry '{key}'; changes were reverted: {error}"
            ));
        }
        Ok(())
    }

    fn read_value(&self, key: &str, kind: InputEntryStorageKind) -> Result<Option<Value>, String> {
        match kind {
            InputEntryStorageKind::Value => self.values.get(key),
            InputEntryStorageKind::Secret => Ok(self.read_secret(key)?.map(Value::String)),
        }
    }

    fn write_value(
        &self,
        key: &str,
        kind: InputEntryStorageKind,
        value: &Value,
    ) -> Result<(), String> {
        match kind {
            InputEntryStorageKind::Value => self.values.set(key, value),
            InputEntryStorageKind::Secret => {
                let secret = value
                    .as_str()
                    .ok_or_else(|| format!("InputEntry '{key}' value must be a string"))?;
                keychain::set_input_secret(
                    self.secrets.as_ref(),
                    self.instance_id.as_ref(),
                    key,
                    secret,
                )
                .map_err(|error| error.to_string())
            }
        }
    }

    fn delete_value(&self, key: &str, kind: InputEntryStorageKind) -> Result<(), String> {
        match kind {
            InputEntryStorageKind::Value => self.values.delete(key),
            InputEntryStorageKind::Secret => {
                keychain::delete_input_secret(self.secrets.as_ref(), self.instance_id.as_ref(), key)
                    .map_err(|error| error.to_string())
            }
        }
    }

    fn read_secret(&self, key: &str) -> Result<Option<String>, String> {
        keychain::get_input_secret(self.secrets.as_ref(), self.instance_id.as_ref(), key)
            .map_err(|error| error.to_string())
    }

    fn restore_snapshot(
        &self,
        key: &str,
        document: &InputEntriesDocument,
        plain: Option<Option<Value>>,
        secret: Option<Option<String>>,
    ) -> Result<(), String> {
        if let Some(plain) = plain {
            match plain {
                Some(value) => self.values.set(key, &value)?,
                None => self.values.delete(key)?,
            }
        }
        if let Some(secret) = secret {
            match secret {
                Some(secret) => keychain::set_input_secret(
                    self.secrets.as_ref(),
                    self.instance_id.as_ref(),
                    key,
                    &secret,
                ),
                None => keychain::delete_input_secret(
                    self.secrets.as_ref(),
                    self.instance_id.as_ref(),
                    key,
                ),
            }
            .map_err(|error| error.to_string())?;
        }
        self.save(document)
    }

    fn load(&self) -> Result<InputEntriesDocument, String> {
        if !self.path.exists() {
            return Ok(InputEntriesDocument {
                schema_version: SCHEMA_VERSION,
                entries: BTreeMap::new(),
            });
        }
        let bytes = fs::read(&self.path).map_err(|error| {
            format!(
                "Failed to read InputEntry metadata {}: {error}",
                self.path.display()
            )
        })?;
        let document: InputEntriesDocument = serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "Failed to parse InputEntry metadata {}: {error}",
                self.path.display()
            )
        })?;
        if document.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Unsupported InputEntry metadata schema version {} in {}",
                document.schema_version,
                self.path.display()
            ));
        }
        Ok(document)
    }

    fn save(&self, document: &InputEntriesDocument) -> Result<(), String> {
        write_json_atomically(&self.path, document).map_err(|error| {
            format!(
                "Failed to persist InputEntry metadata {}: {error}",
                self.path.display()
            )
        })
    }

    fn operation_lock(&self) -> Result<Arc<RepositoryLock>, String> {
        let mut locks = REPOSITORY_LOCKS
            .lock()
            .map_err(|error| format!("InputEntry repository lock registry is poisoned: {error}"))?;
        if let Some(lock) = locks.get(&self.path).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(self.path.clone(), Arc::downgrade(&lock));
        Ok(lock)
    }
}

fn lock_repository(lock: &RepositoryLock) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    lock.lock()
        .map_err(|error| format!("InputEntry repository lock is poisoned: {error}"))
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.trim() != key {
        return Err("InputEntry key must be non-empty and trimmed".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keychain::{InMemorySecretStore, KeychainError};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Barrier;

    #[derive(Default)]
    struct FailNextDeleteSecretStore {
        inner: InMemorySecretStore,
        fail_next_delete: AtomicBool,
    }

    impl SecretStore for FailNextDeleteSecretStore {
        fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
            self.inner.set_secret(key, secret)
        }

        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            self.inner.get_secret(key)
        }

        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            if self.fail_next_delete.swap(false, Ordering::SeqCst) {
                return Err(KeychainError::Store("injected delete failure".to_string()));
            }
            self.inner.delete_secret(key)
        }
    }

    #[derive(Default)]
    struct CountingSecretStore {
        inner: InMemorySecretStore,
        calls: AtomicUsize,
    }

    impl CountingSecretStore {
        fn seed_input(&self, instance_id: &str, key: &str, value: &str) {
            keychain::set_input_secret(&self.inner, instance_id, key, value).unwrap();
        }

        fn peek_input(&self, instance_id: &str, key: &str) -> Option<String> {
            keychain::get_input_secret(&self.inner, instance_id, key).unwrap()
        }
    }

    impl SecretStore for CountingSecretStore {
        fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.set_secret(key, secret)
        }

        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.get_secret(key)
        }

        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.delete_secret(key)
        }
    }

    #[test]
    fn storage_kind_switch_moves_one_value_and_never_lists_secret_plaintext() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let store =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());

        store
            .upsert("name", Some(serde_json::json!("zhangsan")), false)
            .unwrap();
        assert_eq!(
            store.list().unwrap(),
            vec![InputEntryView {
                key: "name".to_string(),
                secret: false,
                value: Some(serde_json::json!("zhangsan")),
            }]
        );

        store.upsert("name", None, true).unwrap();
        assert_eq!(store.values.get("name").unwrap(), None);
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "name").unwrap(),
            Some("zhangsan".to_string())
        );
        let serialized = serde_json::to_string(&store.list().unwrap()).unwrap();
        assert!(!serialized.contains("zhangsan"));
        assert_eq!(
            store.list().unwrap(),
            vec![InputEntryView {
                key: "name".to_string(),
                secret: true,
                value: None,
            }]
        );

        store.upsert("name", None, false).unwrap();
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "name").unwrap(),
            None
        );
        assert_eq!(
            store.values.get("name").unwrap(),
            Some(serde_json::json!("zhangsan"))
        );
    }

    #[test]
    fn delete_removes_the_authoritative_entry_and_value() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let store = InputEntryStore::from_storage_root("computer-a", directory.path(), secrets);
        store
            .upsert("region", Some(serde_json::json!("cn")), false)
            .unwrap();

        store.delete("region").unwrap();
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.values.get("region").unwrap(), None);
        assert!(store.delete("region").unwrap_err().contains("not found"));
    }

    #[test]
    fn dangling_metadata_stays_listed_and_deletable_without_secret_reads() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(CountingSecretStore::default());
        let store =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        store
            .upsert("plain", Some(serde_json::json!("value")), false)
            .unwrap();
        store
            .upsert("secret", Some(serde_json::json!("top-secret")), true)
            .unwrap();
        store.values.delete("plain").unwrap();
        keychain::delete_input_secret(&secrets.inner, "computer-a", "secret").unwrap();
        secrets.calls.store(0, Ordering::SeqCst);

        assert_eq!(
            store.list().unwrap(),
            vec![
                InputEntryView {
                    key: "plain".to_string(),
                    secret: false,
                    value: None,
                },
                InputEntryView {
                    key: "secret".to_string(),
                    secret: true,
                    value: None,
                },
            ]
        );
        assert_eq!(secrets.calls.load(Ordering::SeqCst), 0);

        store.delete("plain").unwrap();
        store.delete("secret").unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn legacy_dual_namespace_uses_current_definition_once_then_discards_it() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let store =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        store
            .values
            .set("token", &serde_json::json!("current-plain"))
            .unwrap();
        keychain::set_input_secret(secrets.as_ref(), "computer-a", "token", "stale-secret")
            .unwrap();
        let legacy = BTreeMap::from([(
            "token".to_string(),
            BTreeSet::from([InputValueStorageKind::Value, InputValueStorageKind::Secret]),
        )]);
        let preferred = BTreeMap::from([("token".to_string(), InputEntryStorageKind::Value)]);

        store
            .migrate_legacy(LoadedInputValueIndex::v2(legacy), &preferred)
            .unwrap();
        assert_eq!(
            store
                .resolve("token", InputEntryStorageKind::Secret)
                .unwrap(),
            Some(serde_json::json!("current-plain"))
        );
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "token").unwrap(),
            None
        );
    }

    #[test]
    fn resolve_migrates_the_unique_legacy_kind_before_any_management_read() {
        let directory = tempfile::tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let storage_root = config.computer_instance_storage_root("computer-a");
        let secrets = Arc::new(InMemorySecretStore::default());
        let store =
            InputEntryStore::from_storage_root("computer-a", &storage_root, secrets.clone());

        keychain::set_input_secret(
            secrets.as_ref(),
            "computer-a",
            "legacy-secret",
            "secret-value",
        )
        .unwrap();
        store
            .values
            .set("legacy-secret", &serde_json::json!("stale-plain"))
            .unwrap();
        input_value_index::record(
            &config,
            "computer-a",
            "legacy-secret",
            InputValueStorageKind::Secret,
        )
        .unwrap();
        store
            .values
            .set("legacy-plain", &serde_json::json!("plain-value"))
            .unwrap();
        keychain::set_input_secret(
            secrets.as_ref(),
            "computer-a",
            "legacy-plain",
            "stray-secret",
        )
        .unwrap();
        input_value_index::record(
            &config,
            "computer-a",
            "legacy-plain",
            InputValueStorageKind::Value,
        )
        .unwrap();
        assert!(!storage_root.join(FILE_NAME).exists());

        assert_eq!(
            store
                .resolve_entry("legacy-secret", InputEntryStorageKind::Value)
                .unwrap(),
            Some(ResolvedInputEntry {
                value: serde_json::json!("secret-value"),
                storage_kind: InputEntryStorageKind::Secret,
            })
        );
        assert_eq!(
            store
                .resolve_entry("legacy-plain", InputEntryStorageKind::Secret)
                .unwrap(),
            Some(ResolvedInputEntry {
                value: serde_json::json!("plain-value"),
                storage_kind: InputEntryStorageKind::Value,
            })
        );
        assert_eq!(
            store.storage_kind("legacy-secret").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
        assert_eq!(
            store.storage_kind("legacy-plain").unwrap(),
            Some(InputEntryStorageKind::Value)
        );
        assert_eq!(store.values.get("legacy-secret").unwrap(), None);
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "legacy-plain").unwrap(),
            None
        );
    }

    #[test]
    fn full_migration_honors_singleton_index_kinds_over_stray_copies() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(InMemorySecretStore::default());
        let store =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        store
            .values
            .set("secret-only", &serde_json::json!("stale-plain"))
            .unwrap();
        keychain::set_input_secret(
            secrets.as_ref(),
            "computer-a",
            "secret-only",
            "current-secret",
        )
        .unwrap();
        store
            .values
            .set("value-only", &serde_json::json!("current-value"))
            .unwrap();
        keychain::set_input_secret(secrets.as_ref(), "computer-a", "value-only", "stray-secret")
            .unwrap();
        let legacy = BTreeMap::from([
            (
                "secret-only".to_string(),
                BTreeSet::from([InputValueStorageKind::Secret]),
            ),
            (
                "value-only".to_string(),
                BTreeSet::from([InputValueStorageKind::Value]),
            ),
        ]);
        let opposite_preferences = BTreeMap::from([
            ("secret-only".to_string(), InputEntryStorageKind::Value),
            ("value-only".to_string(), InputEntryStorageKind::Secret),
        ]);

        store
            .migrate_legacy(LoadedInputValueIndex::v2(legacy), &opposite_preferences)
            .unwrap();

        assert_eq!(
            store.storage_kind("secret-only").unwrap(),
            Some(InputEntryStorageKind::Secret)
        );
        assert_eq!(
            store.storage_kind("value-only").unwrap(),
            Some(InputEntryStorageKind::Value)
        );
        assert_eq!(store.values.get("secret-only").unwrap(), None);
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "value-only").unwrap(),
            None
        );
    }

    #[test]
    fn v1_plain_adoption_never_reads_or_deletes_keychain() {
        let directory = tempfile::tempdir().unwrap();
        let storage_root = directory.path();
        let secrets = Arc::new(CountingSecretStore::default());
        let store = InputEntryStore::from_storage_root("computer-a", storage_root, secrets.clone());
        store
            .values
            .set("runtime-value", &serde_json::json!("runtime-plain"))
            .unwrap();
        store
            .values
            .set("listed-value", &serde_json::json!("listed-plain"))
            .unwrap();
        secrets.seed_input("computer-a", "runtime-value", "unowned-secret-one");
        secrets.seed_input("computer-a", "listed-value", "unowned-secret-two");
        write_json_atomically(
            &storage_root.join("input_value_ids.json"),
            &serde_json::json!({
                "schema_version": 1,
                "ids": ["runtime-value", "listed-value"]
            }),
        )
        .unwrap();

        assert_eq!(
            store
                .resolve_entry("runtime-value", InputEntryStorageKind::Value)
                .unwrap(),
            Some(ResolvedInputEntry {
                value: serde_json::json!("runtime-plain"),
                storage_kind: InputEntryStorageKind::Value,
            })
        );
        let loaded =
            input_value_index::load_with_provenance_from_storage_root(storage_root).unwrap();
        assert_eq!(loaded.provenance, InputValueIndexProvenance::LegacyV1);
        store
            .migrate_legacy(
                loaded,
                &BTreeMap::from([("listed-value".to_string(), InputEntryStorageKind::Secret)]),
            )
            .unwrap();

        assert_eq!(secrets.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            secrets.peek_input("computer-a", "runtime-value").as_deref(),
            Some("unowned-secret-one")
        );
        assert_eq!(
            secrets.peek_input("computer-a", "listed-value").as_deref(),
            Some("unowned-secret-two")
        );
        assert_eq!(
            store.storage_kind("runtime-value").unwrap(),
            Some(InputEntryStorageKind::Value)
        );
        assert_eq!(
            store.storage_kind("listed-value").unwrap(),
            Some(InputEntryStorageKind::Value)
        );
    }

    #[test]
    fn failed_storage_switch_restores_the_original_secret_entry() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FailNextDeleteSecretStore::default());
        let store =
            InputEntryStore::from_storage_root("computer-a", directory.path(), secrets.clone());
        store
            .upsert("token", Some(serde_json::json!("top-secret")), true)
            .unwrap();
        secrets.fail_next_delete.store(true, Ordering::SeqCst);

        let error = store.upsert("token", None, false).unwrap_err();

        assert!(error.contains("changes were reverted"));
        assert!(!error.contains("top-secret"));
        assert_eq!(store.values.get("token").unwrap(), None);
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "token")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        assert_eq!(
            store.list().unwrap(),
            vec![InputEntryView {
                key: "token".to_string(),
                secret: true,
                value: None,
            }]
        );
    }

    #[test]
    fn concurrent_repository_instances_do_not_lose_metadata_updates() {
        let directory = tempfile::tempdir().unwrap();
        let storage_root = directory.path().to_path_buf();
        let secrets = Arc::new(InMemorySecretStore::default());
        let barrier = Arc::new(Barrier::new(16));
        let mut threads = Vec::new();
        for index in 0..16 {
            let storage_root = storage_root.clone();
            let secrets = secrets.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let store = InputEntryStore::from_storage_root("computer-a", storage_root, secrets);
                barrier.wait();
                store
                    .upsert(
                        &format!("key-{index:02}"),
                        Some(serde_json::json!(format!("value-{index:02}"))),
                        index % 2 == 0,
                    )
                    .unwrap();
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }

        let store = InputEntryStore::from_storage_root("computer-a", &storage_root, secrets);
        let entries = store.list().unwrap();
        assert_eq!(entries.len(), 16);
        assert_eq!(entries.iter().filter(|entry| entry.secret).count(), 8);
        assert!(entries
            .iter()
            .filter(|entry| entry.secret)
            .all(|entry| entry.value.is_none()));
    }
}
