use crate::commands::inputs::{
    input_definition_from_sdk, input_definition_to_sdk, InputDefinition,
};
use crate::services::config::ConfigService;
use crate::services::input_references::{find_project_input_references, referenced_input_ids};
use crate::services::storage::write_json_atomically;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::{
    delete_config, duplicate_config, export_config, import_config, init_config, load_config,
    load_project_config_doc, migrate_config, save_config, update_config, validate_config,
    ComputerConfigSnapshot, ConfigContext, ConfigCrudError, ConfigEdit, ConfigEntity, EditIntent,
    EntityKey, ProjectConfigDoc, ProvenanceScope, ValidationReport, WriteScope, WriteTargetError,
};
use a2c_smcp::smcp_computer::settings::{
    resolve_mcp_config, resolve_settings, user_mcp_config_path, workdir_mcp_config_path, EnvMap,
    ResolveMcpConfigArgs, ResolveSettingsArgs, ResolvedMcpConfig, SettingsValidationError,
    MANAGED_MCP_FILENAME, TFROBOT_DIRNAME, XDG_CONFIG_HOME_ENV,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

#[cfg(test)]
type AnchorRestorePause = (Arc<std::sync::Barrier>, Arc<std::sync::Barrier>);

type ConfigTransaction = Arc<std::sync::RwLock<()>>;

static CONFIG_TRANSACTIONS: OnceLock<
    std::sync::Mutex<HashMap<PathBuf, std::sync::Weak<std::sync::RwLock<()>>>>,
> = OnceLock::new();

fn config_transaction_for(project_anchor: &Path) -> ConfigTransaction {
    let registry = CONFIG_TRANSACTIONS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut transactions = registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(transaction) = transactions
        .get(project_anchor)
        .and_then(std::sync::Weak::upgrade)
    {
        return transaction;
    }
    transactions.retain(|_, transaction| transaction.strong_count() > 0);
    let transaction = Arc::new(std::sync::RwLock::new(()));
    transactions.insert(project_anchor.to_path_buf(), Arc::downgrade(&transaction));
    transaction
}

/// The client-side boundary for SDK-owned Computer configuration.
///
/// `ConfigService` continues to own client profile/connection data. MCP, skill,
/// marketplace, plugin, and runtime configuration crosses this adapter only.
#[derive(Clone)]
pub struct SdkConfigService {
    config: Arc<ConfigService>,
    #[cfg(test)]
    fail_next_raw_restore: Arc<AtomicBool>,
    #[cfg(test)]
    fail_next_raw_restore_after_backup: Arc<AtomicBool>,
    #[cfg(test)]
    fail_next_anchor_restore_after_project_backup: Arc<AtomicBool>,
    #[cfg(test)]
    anchor_restore_pause_after_project_backup: Arc<std::sync::Mutex<Option<AnchorRestorePause>>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RawRestorePhase {
    Prepared,
    PreviousMoved,
    Committed,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRestoreTransaction {
    phase: RawRestorePhase,
    original_settings_dir_existed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AnchorRestorePhase {
    Prepared,
    PreviousMoved,
    Committed,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchorRestoreTransaction {
    phase: AnchorRestorePhase,
    original_project_dir_existed: bool,
    original_user_dir_existed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SdkConfigPortabilityError {
    #[error("Cannot export invalid SDK MCP configuration: {details}")]
    InvalidSource {
        errors: Vec<SettingsValidationError>,
        details: String,
    },
    #[error("Portable SDK MCP configuration contains plaintext in a sensitive field: {details}")]
    UnsafePlaintext {
        fields: Vec<String>,
        details: String,
    },
    #[error(transparent)]
    Crud(#[from] ConfigCrudError),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemovedHttpOAuthMigration {
    pub removed_fields: usize,
    pub disabled_opt_out_servers: usize,
}

impl SdkConfigPortabilityError {
    fn invalid_source(errors: Vec<SettingsValidationError>) -> Self {
        let details = errors
            .iter()
            .map(|error| {
                format!(
                    "{}:{}: {}",
                    error.source_path.as_deref().unwrap_or("SDK MCP config"),
                    error.field,
                    error.reason
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Self::InvalidSource { errors, details }
    }

    fn unsafe_plaintext(fields: Vec<String>) -> Self {
        let details = fields.join("; ");
        Self::UnsafePlaintext { fields, details }
    }
}

/// Converts the legacy UI's `{{ID}}` spelling while migrating pre-SDK client configuration.
///
/// This must remain confined to [`crate::services::config_migration`]. Applying it during normal
/// CRUD would make literal constants ambiguous and corrupt their exact persisted value.
pub(crate) fn normalize_mcp_input_references(
    config: MCPServerConfig,
) -> Result<MCPServerConfig, String> {
    let mut value = serde_json::to_value(config).map_err(|error| error.to_string())?;
    let Some(object) = value.as_object_mut() else {
        return Err("MCP server config must be an object".to_string());
    };
    if let Some(parameters) = object.get_mut("server_parameters") {
        normalize_input_references_in_value(parameters);
    }
    if let Some(env_file) = object.get_mut("envFile") {
        normalize_input_references_in_value(env_file);
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn normalize_input_references_in_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = normalize_input_references_in_string(text),
        Value::Array(values) => values
            .iter_mut()
            .for_each(normalize_input_references_in_value),
        Value::Object(values) => values
            .values_mut()
            .for_each(normalize_input_references_in_value),
        _ => {}
    }
}

fn normalize_input_references_in_string(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find("{{") {
        normalized.push_str(&remaining[..start]);
        let candidate = &remaining[start + 2..];
        let Some(end) = candidate.find("}}") else {
            normalized.push_str(&remaining[start..]);
            return normalized;
        };
        let id = candidate[..end].trim();
        if !id.is_empty() && !id.contains(['{', '}']) {
            normalized.push_str("${input:");
            normalized.push_str(id);
            normalized.push('}');
        } else {
            normalized.push_str(&remaining[start..start + 2 + end + 2]);
        }
        remaining = &candidate[end + 2..];
    }
    normalized.push_str(remaining);
    normalized
}

/// Resolve only configuration owned by the Computer instance for a portable backup.
///
/// Policy is an ambient, read-only constraint of the machine performing the export. Pointing the
/// SDK resolver at a guaranteed-absent managed file removes that layer before precedence is
/// applied. Filtering Policy winners after resolution would also discard an instance-owned server
/// hidden by a same-name Policy declaration.
fn resolve_portable_mcp_without_policy(
    context: &InstanceConfigContext,
    staging_root: &Path,
) -> ResolvedMcpConfig {
    let absent_policy_path = staging_root
        .join("excluded-policy")
        .join(MANAGED_MCP_FILENAME);
    resolve_mcp_config(ResolveMcpConfigArgs {
        cwd: Some(context.project_anchor()),
        env: Some(context.env()),
        managed_mcp_path: Some(&absent_policy_path),
        ..Default::default()
    })
}

#[derive(Clone)]
pub(crate) struct InstanceConfigContext {
    project_anchor: PathBuf,
    skill_home: PathBuf,
    env: EnvMap,
    config_transaction: ConfigTransaction,
}

impl InstanceConfigContext {
    pub(crate) fn new(project_anchor: PathBuf, skill_home: PathBuf) -> Self {
        let mut env = EnvMap::new();
        env.insert(
            XDG_CONFIG_HOME_ENV.to_string(),
            project_anchor.to_string_lossy().into_owned(),
        );
        Self {
            config_transaction: config_transaction_for(&project_anchor),
            project_anchor,
            skill_home,
            env,
        }
    }

    pub(crate) fn sdk_context(&self) -> ConfigContext<'_> {
        let mut context = ConfigContext::new(&self.project_anchor);
        context.env = Some(&self.env);
        context.home = Some(&self.skill_home);
        context
    }

    pub(crate) fn load(&self) -> ComputerConfigSnapshot {
        self.with_read(|| self.load_unlocked())
    }

    fn load_unlocked(&self) -> ComputerConfigSnapshot {
        load_config(&self.sdk_context())
    }

    fn validate(&self) -> ValidationReport {
        self.with_read(|| self.validate_unlocked())
    }

    fn validate_unlocked(&self) -> ValidationReport {
        let mut errors = resolve_settings(ResolveSettingsArgs {
            cwd: Some(&self.project_anchor),
            env: Some(&self.env),
            ..Default::default()
        })
        .errors;
        errors.extend(
            resolve_mcp_config(ResolveMcpConfigArgs {
                cwd: Some(&self.project_anchor),
                env: Some(&self.env),
                ..Default::default()
            })
            .errors,
        );
        ValidationReport { errors }
    }

    pub(crate) fn with_read<T>(&self, action: impl FnOnce() -> T) -> T {
        let _guard = self
            .config_transaction
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        action()
    }

    pub(crate) fn project_anchor(&self) -> &Path {
        &self.project_anchor
    }

    pub(crate) fn skill_home(&self) -> &Path {
        &self.skill_home
    }

    pub(crate) fn env(&self) -> &EnvMap {
        &self.env
    }
}

impl SdkConfigService {
    pub fn new(config: Arc<ConfigService>) -> Self {
        Self {
            config,
            #[cfg(test)]
            fail_next_raw_restore: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fail_next_raw_restore_after_backup: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fail_next_anchor_restore_after_project_backup: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            anchor_restore_pause_after_project_backup: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn project_anchor(&self, instance_id: &str) -> PathBuf {
        self.config
            .computer_instance_storage_root(instance_id)
            .join("sdk_config")
    }

    fn config_transaction(&self, instance_id: &str) -> Arc<std::sync::RwLock<()>> {
        config_transaction_for(&self.project_anchor(instance_id))
    }

    fn with_config_read<T>(&self, instance_id: &str, action: impl FnOnce() -> T) -> T {
        let transaction = self.config_transaction(instance_id);
        let _guard = transaction
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        action()
    }

    fn with_config_write<T>(&self, instance_id: &str, action: impl FnOnce() -> T) -> T {
        let transaction = self.config_transaction(instance_id);
        let _guard = transaction
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        action()
    }

    pub fn skill_home(&self, instance_id: &str) -> PathBuf {
        self.config
            .get_computer_instance(instance_id)
            .ok()
            .and_then(|instance| instance.local_skills_root)
            .unwrap_or_else(|| self.config.default_local_skills_root(instance_id))
    }

    pub fn env(&self, instance_id: &str) -> EnvMap {
        self.context(instance_id).env
    }

    fn context(&self, instance_id: &str) -> InstanceConfigContext {
        InstanceConfigContext::new(
            self.project_anchor(instance_id),
            self.skill_home(instance_id),
        )
    }

    pub fn init(&self, instance_id: &str) -> Result<(), ConfigCrudError> {
        self.with_config_write(instance_id, || {
            init_config(&self.project_anchor(instance_id))
        })
    }

    pub fn load(&self, instance_id: &str) -> ComputerConfigSnapshot {
        self.with_config_read(instance_id, || self.context(instance_id).load_unlocked())
    }

    /// Reads the merged top-level MCP input definitions projected by the SDK.
    ///
    /// The returned client DTO is a UI/API projection only; definitions remain owned by the
    /// SDK `ProjectConfigDoc` and are never persisted in client profile storage.
    pub fn load_input_definitions(&self, instance_id: &str) -> Vec<InputDefinition> {
        self.load(instance_id)
            .inputs
            .inputs
            .iter()
            .map(input_definition_from_sdk)
            .collect()
    }

    /// Reads only the definitions owned by this Computer's writable project document.
    ///
    /// The SDK merged snapshot is intentionally not suitable for CRUD: copying it back would
    /// shadow local/user/policy definitions in project scope and could make an edit appear to
    /// succeed while a higher-precedence owner remains unchanged.
    pub fn load_project_input_definitions(
        &self,
        instance_id: &str,
    ) -> Result<Vec<InputDefinition>, ConfigCrudError> {
        self.with_config_read(instance_id, || {
            let mcp = self.load_project_mcp_document_unlocked(instance_id)?;
            let Some(encoded) = mcp.get("inputs") else {
                return Ok(Vec::new());
            };
            let definitions = serde_json::from_value::<
                Vec<a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput>,
            >(encoded.clone())
            .map_err(|error| ConfigCrudError::Io {
                path: self.project_anchor(instance_id),
                reason: format!("failed to deserialize project MCP input definitions: {error}"),
            })?;
            Ok(definitions.iter().map(input_definition_from_sdk).collect())
        })
    }

    /// Replaces the current Computer's project-scope top-level MCP input definitions atomically.
    /// Runtime state is deliberately untouched; the SDK rematerializes this raw configuration on
    /// the next actual start or restart.
    pub fn replace_input_definitions(
        &self,
        instance_id: &str,
        definitions: &[InputDefinition],
    ) -> Result<ProjectConfigDoc, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            let previous_mcp = self.load_project_mcp_document_unlocked(instance_id)?;
            let encoded = definitions
                .iter()
                .map(input_definition_to_sdk)
                .map(|definition| {
                    serde_json::to_value(definition).map_err(|error| ConfigCrudError::Io {
                        path: self.project_anchor(instance_id),
                        reason: format!("failed to serialize MCP input definition: {error}"),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut next_mcp = previous_mcp.clone();
            next_mcp.insert("inputs".to_string(), Value::Array(encoded));
            self.save_project_mcp_document_unlocked(instance_id, &next_mcp)?;
            Ok(ProjectConfigDoc {
                mcp: Some(previous_mcp),
                ..Default::default()
            })
        })
    }

    pub(crate) fn load_project_input_document(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, ConfigCrudError> {
        self.with_config_read(instance_id, || {
            Ok(ProjectConfigDoc {
                mcp: Some(self.load_project_mcp_document_unlocked(instance_id)?),
                ..Default::default()
            })
        })
    }

    pub(crate) fn restore_project_input_document(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<(), ConfigCrudError> {
        let empty = Map::new();
        self.with_config_write(instance_id, || {
            self.save_project_mcp_document_unlocked(
                instance_id,
                document.mcp.as_ref().unwrap_or(&empty),
            )
        })
    }

    fn load_project_mcp_document_unlocked(
        &self,
        instance_id: &str,
    ) -> Result<Map<String, Value>, ConfigCrudError> {
        let path = workdir_mcp_config_path(&self.project_anchor(instance_id));
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
            Err(error) => return Err(raw_restore_io(&path, error)),
        };
        serde_json::from_slice::<Map<String, Value>>(&bytes).map_err(|error| ConfigCrudError::Io {
            path,
            reason: format!("failed to deserialize project MCP document: {error}"),
        })
    }

    fn save_project_mcp_document_unlocked(
        &self,
        instance_id: &str,
        document: &Map<String, Value>,
    ) -> Result<(), ConfigCrudError> {
        let path = workdir_mcp_config_path(&self.project_anchor(instance_id));
        #[cfg(test)]
        if self.fail_next_raw_restore.swap(false, Ordering::SeqCst) {
            return Err(ConfigCrudError::Io {
                path,
                reason: "injected raw SDK restore failure".to_string(),
            });
        }
        write_json_atomically(&path, document).map_err(|error| ConfigCrudError::Io {
            path,
            reason: error.to_string(),
        })
    }

    pub fn load_with_validation(
        &self,
        instance_id: &str,
    ) -> Result<(ComputerConfigSnapshot, ValidationReport), ConfigCrudError> {
        self.with_config_read(instance_id, || {
            let context = self.context(instance_id);
            for _ in 0..3 {
                let snapshot_before = context.load_unlocked();
                let validation_before = context.validate_unlocked();
                let snapshot_after = context.load_unlocked();
                let validation_after = context.validate_unlocked();
                if snapshot_before.revision == snapshot_after.revision
                    && validation_before == validation_after
                {
                    return Ok((snapshot_after, validation_after));
                }
            }
            Err(ConfigCrudError::Io {
                path: context.project_anchor().to_path_buf(),
                reason:
                    "SDK configuration changed repeatedly while reading snapshot and validation"
                        .to_string(),
            })
        })
    }

    pub fn save(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<(), ConfigCrudError> {
        self.with_config_write(instance_id, || {
            save_config(&self.project_anchor(instance_id), document)
        })
    }

    pub fn update(
        &self,
        instance_id: &str,
        edits: &[ConfigEdit],
    ) -> Result<ComputerConfigSnapshot, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            let context = self.context(instance_id);
            update_config(&context.sdk_context(), edits)
        })
    }

    /// Upserts MCP declarations into SDK-owned config without touching runtime state.
    ///
    /// Configuration CRUD must not resolve commands, paths, inputs, or secrets. Those checks are
    /// deferred to runtime restart/preflight/start. New declarations use the client's local scope;
    /// existing declarations update at their writable origin through the SDK write-target resolver.
    pub fn upsert_mcp_configs(
        &self,
        instance_id: &str,
        servers: &[MCPServerConfig],
    ) -> Result<ComputerConfigSnapshot, SdkConfigPortabilityError> {
        self.with_config_write(instance_id, || {
            let servers = self.prepare_literal_preserving_mcp_configs(servers)?;
            let context = self.context(instance_id);
            let mut sdk_context = context.sdk_context();
            sdk_context.opts.upsert_new_scope = WriteScope::Local;
            let edits = mcp_upsert_edits(&context, &servers)?;
            Ok(update_config(&sdk_context, &edits)?)
        })
    }

    /// Atomically commits one MCP declaration together with its Client-edited Input definitions.
    ///
    /// The SDK still owns the durable wire model. We stage the complete raw SDK document, ask the
    /// SDK edit executor to select the correct writable server scope, then replace the live SDK
    /// files with one crash-safe raw transaction. The WebView can therefore edit one logical
    /// configuration item without exposing a transient definition/reference split.
    pub fn upsert_mcp_config_with_inputs_atomically(
        &self,
        instance_id: &str,
        config: &MCPServerConfig,
        project_inputs: &[InputDefinition],
        edited_input_ids: &HashSet<String>,
        remove_input_ids_if_unused: &HashSet<String>,
    ) -> Result<ComputerConfigSnapshot, SdkConfigPortabilityError> {
        self.with_config_write(instance_id, || {
            let config = self
                .prepare_literal_preserving_mcp_configs(std::slice::from_ref(config))?
                .into_iter()
                .next()
                .expect("one MCP config was prepared");
            let (_staging, staging_anchor) =
                self.stage_complete_sdk_anchor_unlocked(instance_id)?;
            let mut staged_document = load_project_config_doc(&staging_anchor)?;
            replace_project_inputs(&mut staged_document, project_inputs)?;
            save_config(&staging_anchor, &staged_document)?;

            let staging_context =
                InstanceConfigContext::new(staging_anchor.clone(), self.skill_home(instance_id));
            let mut sdk_context = staging_context.sdk_context();
            sdk_context.opts.upsert_new_scope = WriteScope::Local;
            let edits = mcp_upsert_edits(&staging_context, std::slice::from_ref(&config))?;
            update_config(&sdk_context, &edits)?;

            let mut next_document = load_project_config_doc(&staging_anchor)?;
            let referenced = all_scope_input_references(&staging_context, &next_document)?;
            remove_unreferenced_project_inputs(
                &mut next_document,
                remove_input_ids_if_unused,
                &referenced,
            );
            save_config(&staging_anchor, &next_document)?;
            let report = staging_context.validate();
            if !report.is_valid() {
                return Err(SdkConfigPortabilityError::invalid_source(report.errors));
            }
            ensure_edited_inputs_are_effective(&staging_context, project_inputs, edited_input_ids)?;

            self.commit_complete_sdk_anchor_unlocked(instance_id, &staging_anchor)?;
            Ok(self.context(instance_id).load_unlocked())
        })
    }

    /// Atomically merges an imported set of MCP declarations into the SDK local scope.
    ///
    /// The SDK's entity-edit executor intentionally does not roll back earlier edits when a later
    /// edit fails. Import therefore prepares one complete `mcp.local.json` document and persists
    /// it through one SDK `save_config` call. Existing project/user declarations are shadowed by
    /// the imported local declarations; read-only reconciled origins are rejected before writing.
    pub fn import_mcp_configs_atomically(
        &self,
        instance_id: &str,
        servers: &[MCPServerConfig],
    ) -> Result<ComputerConfigSnapshot, SdkConfigPortabilityError> {
        self.with_config_write(instance_id, || {
            let context = self.context(instance_id);
            let Some(document) = self.prepare_local_mcp_import_document(instance_id, servers)?
            else {
                return Ok(context.load_unlocked());
            };
            save_config(context.project_anchor(), &document)?;
            Ok(context.load_unlocked())
        })
    }

    /// Verifies the complete local-scope merge before a client crash-recovery journal is created.
    /// This catches deterministic read-only provenance, malformed target shape, schema, and
    /// serialization failures without mutating either the SDK config or client input definitions.
    pub fn preflight_import_mcp_configs(
        &self,
        instance_id: &str,
        servers: &[MCPServerConfig],
    ) -> Result<(), SdkConfigPortabilityError> {
        self.with_config_read(instance_id, || {
            self.prepare_local_mcp_import_document(instance_id, servers)?;
            Ok(())
        })
    }

    fn prepare_local_mcp_import_document(
        &self,
        instance_id: &str,
        servers: &[MCPServerConfig],
    ) -> Result<Option<ProjectConfigDoc>, SdkConfigPortabilityError> {
        let servers = self.prepare_literal_preserving_mcp_configs(servers)?;
        if servers.is_empty() {
            return Ok(None);
        }
        let context = self.context(instance_id);
        let snapshot = context.load_unlocked();

        for server in &servers {
            let entity = EntityKey::Mcp(server.name().to_string());
            if let Some(origin) = snapshot.provenance.get(&entity).copied() {
                if !is_writable_provenance(origin) {
                    return Err(
                        ConfigCrudError::WriteTarget(WriteTargetError::ReadOnlyOrigin {
                            entity: entity.to_string(),
                            origin,
                        })
                        .into(),
                    );
                }
            }
        }

        let document = load_project_config_doc(context.project_anchor())?;
        let mut local_mcp = document.mcp_local.unwrap_or_default();
        let mut local_servers = match local_mcp.remove("servers") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(servers)) => servers,
            Some(_) => {
                return Err(ConfigCrudError::Io {
                    path: context.project_anchor().to_path_buf(),
                    reason: "local MCP 'servers' must be a JSON object".to_string(),
                }
                .into());
            }
        };

        for server in &servers {
            let name = server.name().to_string();
            let value = serde_json::to_value(server).map_err(|error| ConfigCrudError::Io {
                path: context.project_anchor().to_path_buf(),
                reason: format!("failed to serialize imported MCP server '{name}': {error}"),
            })?;
            let body = canonical_mcp_server_body(value).map_err(|reason| ConfigCrudError::Io {
                path: context.project_anchor().to_path_buf(),
                reason: format!("invalid imported MCP server '{name}': {reason}"),
            })?;
            local_servers.insert(name, Value::Object(body));
        }

        local_mcp.insert("servers".to_string(), Value::Object(local_servers));
        let mut merged_document = load_project_config_doc(context.project_anchor())?;
        merged_document.mcp_local = Some(local_mcp.clone());
        let report = validate_config(&merged_document);
        if !report.is_valid() {
            return Err(SdkConfigPortabilityError::invalid_source(report.errors));
        }
        Ok(Some(ProjectConfigDoc {
            mcp_local: Some(local_mcp),
            ..Default::default()
        }))
    }

    /// Removes one MCP declaration from SDK-owned config without touching runtime state.
    pub fn remove_mcp_config(
        &self,
        instance_id: &str,
        name: &str,
    ) -> Result<ComputerConfigSnapshot, ConfigCrudError> {
        self.update(
            instance_id,
            &[ConfigEdit::new(
                ConfigEntity::McpServer(name.to_string()),
                EditIntent::Remove,
            )],
        )
    }

    /// Removes one declaration and any project Input definitions that become unreferenced in the
    /// same raw SDK transaction. Definitions still used by another server or scope are retained.
    pub fn remove_mcp_config_with_input_gc_atomically(
        &self,
        instance_id: &str,
        name: &str,
        input_candidates: &HashSet<String>,
    ) -> Result<ComputerConfigSnapshot, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            let (_staging, staging_anchor) =
                self.stage_complete_sdk_anchor_unlocked(instance_id)?;
            let staging_context =
                InstanceConfigContext::new(staging_anchor.clone(), self.skill_home(instance_id));
            update_config(
                &staging_context.sdk_context(),
                &[ConfigEdit::new(
                    ConfigEntity::McpServer(name.to_string()),
                    EditIntent::Remove,
                )],
            )?;
            let mut next_document = load_project_config_doc(&staging_anchor)?;
            let referenced = all_scope_input_references(&staging_context, &next_document)?;
            remove_unreferenced_project_inputs(&mut next_document, input_candidates, &referenced);
            save_config(&staging_anchor, &next_document)?;
            let report = staging_context.validate();
            if !report.is_valid() {
                return Err(ConfigCrudError::Io {
                    path: staging_anchor,
                    reason: format!("staged MCP removal is invalid: {:?}", report.errors),
                });
            }
            self.commit_complete_sdk_anchor_unlocked(
                instance_id,
                staging_context.project_anchor(),
            )?;
            Ok(self.context(instance_id).load_unlocked())
        })
    }

    pub fn validate(&self, document: &ProjectConfigDoc) -> ValidationReport {
        validate_config(document)
    }

    /// Validates every SDK scope resolved for one Computer instance.
    ///
    /// This deliberately reuses the SDK settings and MCP resolvers' schema diagnostics. It does
    /// not resolve inputs or secrets and never probes commands, paths, marketplaces, plugins, or
    /// MCP server availability.
    pub fn validate_instance(
        &self,
        instance_id: &str,
    ) -> Result<ValidationReport, ConfigCrudError> {
        self.with_config_read(instance_id, || {
            Ok(self.context(instance_id).validate_unlocked())
        })
    }

    pub fn migrate(&self, instance_id: &str) -> Result<bool, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            migrate_config(&self.project_anchor(instance_id))
        })
    }

    pub fn delete(&self, instance_id: &str) -> Result<(), ConfigCrudError> {
        self.with_config_write(instance_id, || {
            delete_config(&self.project_anchor(instance_id))
        })
    }

    pub fn duplicate(&self, source_id: &str, target_id: &str) -> Result<(), ConfigCrudError> {
        if source_id == target_id {
            return self.with_config_write(source_id, || {
                duplicate_config(
                    &self.project_anchor(source_id),
                    &self.project_anchor(target_id),
                )
            });
        }
        let (first_id, second_id) = if source_id < target_id {
            (source_id, target_id)
        } else {
            (target_id, source_id)
        };
        let first = self.config_transaction(first_id);
        let second = self.config_transaction(second_id);
        let _first_guard = first
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _second_guard = second
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        duplicate_config(
            &self.project_anchor(source_id),
            &self.project_anchor(target_id),
        )
    }

    pub fn export(&self, instance_id: &str) -> Result<ProjectConfigDoc, ConfigCrudError> {
        self.with_config_read(instance_id, || {
            export_config(&self.project_anchor(instance_id))
        })
    }

    /// Loads the SDK-owned project anchor without crossing the sanitized export boundary.
    /// This is reserved for same-machine lifecycle transactions such as legacy migration.
    pub(crate) fn load_raw_project_config(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            self.load_raw_project_config_unlocked(instance_id)
        })
    }

    fn load_raw_project_config_unlocked(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, ConfigCrudError> {
        let anchor = self.project_anchor(instance_id);
        recover_anchor_restore_transaction(&anchor)?;
        recover_raw_restore_transaction(&anchor)?;
        load_project_config_doc(&anchor)
    }

    /// Copies the complete SDK scope root into an isolated same-filesystem staging anchor.
    /// The instance-specific XDG root lives below this anchor, so this preserves User together
    /// with Project/Local provenance while the SDK resolves its normal write target.
    #[cfg(test)]
    fn stage_complete_sdk_anchor(
        &self,
        instance_id: &str,
    ) -> Result<(tempfile::TempDir, PathBuf), ConfigCrudError> {
        self.with_config_write(instance_id, || {
            self.stage_complete_sdk_anchor_unlocked(instance_id)
        })
    }

    fn stage_complete_sdk_anchor_unlocked(
        &self,
        instance_id: &str,
    ) -> Result<(tempfile::TempDir, PathBuf), ConfigCrudError> {
        let anchor = self.project_anchor(instance_id);
        recover_anchor_restore_transaction(&anchor)?;
        recover_raw_restore_transaction(&anchor)?;
        let parent = anchor.parent().ok_or_else(|| ConfigCrudError::Io {
            path: anchor.clone(),
            reason: "SDK project anchor has no parent directory".to_string(),
        })?;
        fs::create_dir_all(parent).map_err(|error| raw_restore_io(parent, error))?;
        let staging = tempfile::Builder::new()
            .prefix(".mcp-input-stage-")
            .tempdir_in(parent)
            .map_err(|error| ConfigCrudError::Io {
                path: parent.to_path_buf(),
                reason: format!("failed to create MCP config staging directory: {error}"),
            })?;
        let staging_anchor = staging.path().join("sdk-anchor");
        fs::create_dir_all(&staging_anchor)
            .map_err(|error| raw_restore_io(&staging_anchor, error))?;
        for relative in [Path::new(TFROBOT_DIRNAME), Path::new("a2c")] {
            let source = anchor.join(relative);
            if source.exists() {
                copy_directory_tree(&source, &staging_anchor.join(relative))?;
            }
        }
        Ok((staging, staging_anchor))
    }

    /// Commits a fully prepared SDK scope root with one recovery journal. Swapping the complete
    /// instance anchor keeps User/Project/Local files in the same logical transaction.
    #[cfg(test)]
    fn commit_complete_sdk_anchor(
        &self,
        instance_id: &str,
        staged_anchor: &Path,
    ) -> Result<(), ConfigCrudError> {
        self.with_config_write(instance_id, || {
            self.commit_complete_sdk_anchor_unlocked(instance_id, staged_anchor)
        })
    }

    fn commit_complete_sdk_anchor_unlocked(
        &self,
        instance_id: &str,
        staged_anchor: &Path,
    ) -> Result<(), ConfigCrudError> {
        let anchor = self.project_anchor(instance_id);
        recover_anchor_restore_transaction(&anchor)?;
        #[cfg(test)]
        if self.fail_next_raw_restore.swap(false, Ordering::SeqCst) {
            return Err(ConfigCrudError::Io {
                path: anchor,
                reason: "injected complete SDK restore failure".to_string(),
            });
        }

        let transaction_root = anchor_restore_transaction_root(&anchor)?;
        fs::create_dir_all(&transaction_root)
            .map_err(|error| raw_restore_io(&transaction_root, error))?;
        let current_project_dir = anchor.join(TFROBOT_DIRNAME);
        let current_user_dir = anchor.join("a2c");
        let staged_project_dir = staged_anchor.join(TFROBOT_DIRNAME);
        let staged_user_dir = staged_anchor.join("a2c");
        let transaction_staged_project_dir = transaction_root.join("staged-project");
        let transaction_staged_user_dir = transaction_root.join("staged-user");
        if staged_project_dir.exists() {
            fs::rename(&staged_project_dir, &transaction_staged_project_dir)
                .map_err(|error| raw_restore_io(&staged_project_dir, error))?;
        }
        if staged_user_dir.exists() {
            fs::rename(&staged_user_dir, &transaction_staged_user_dir)
                .map_err(|error| raw_restore_io(&staged_user_dir, error))?;
        }
        let previous_project_dir = transaction_root.join("previous-project");
        let previous_user_dir = transaction_root.join("previous-user");
        let mut transaction = AnchorRestoreTransaction {
            phase: AnchorRestorePhase::Prepared,
            original_project_dir_existed: current_project_dir.exists(),
            original_user_dir_existed: current_user_dir.exists(),
        };
        write_anchor_restore_transaction(&transaction_root, &transaction)?;

        if current_project_dir.exists() {
            if let Err(error) = fs::rename(&current_project_dir, &previous_project_dir) {
                return Err(rollback_anchor_restore_after_error(
                    &anchor,
                    raw_restore_io(&current_project_dir, error),
                ));
            }
        }
        #[cfg(test)]
        if self
            .fail_next_anchor_restore_after_project_backup
            .swap(false, Ordering::SeqCst)
        {
            let error = ConfigCrudError::Io {
                path: anchor.clone(),
                reason: "injected complete SDK restore failure after project backup".to_string(),
            };
            return Err(rollback_anchor_restore_after_error(&anchor, error));
        }
        #[cfg(test)]
        if let Some((reached, resume)) = self
            .anchor_restore_pause_after_project_backup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            reached.wait();
            resume.wait();
        }
        if current_user_dir.exists() {
            if let Err(error) = fs::rename(&current_user_dir, &previous_user_dir) {
                return Err(rollback_anchor_restore_after_error(
                    &anchor,
                    raw_restore_io(&current_user_dir, error),
                ));
            }
        }
        transaction.phase = AnchorRestorePhase::PreviousMoved;
        if let Err(error) = write_anchor_restore_transaction(&transaction_root, &transaction) {
            return Err(rollback_anchor_restore_after_error(&anchor, error));
        }

        #[cfg(test)]
        if self
            .fail_next_raw_restore_after_backup
            .swap(false, Ordering::SeqCst)
        {
            let error = ConfigCrudError::Io {
                path: anchor.clone(),
                reason: "injected complete SDK restore failure after backup".to_string(),
            };
            return Err(rollback_anchor_restore_after_error(&anchor, error));
        }

        if let Err(error) = fs::create_dir_all(&anchor) {
            return Err(rollback_anchor_restore_after_error(
                &anchor,
                raw_restore_io(&anchor, error),
            ));
        }
        if transaction_staged_project_dir.exists() {
            if let Err(error) = fs::rename(&transaction_staged_project_dir, &current_project_dir) {
                return Err(rollback_anchor_restore_after_error(
                    &anchor,
                    raw_restore_io(&transaction_staged_project_dir, error),
                ));
            }
        }
        if transaction_staged_user_dir.exists() {
            if let Err(error) = fs::rename(&transaction_staged_user_dir, &current_user_dir) {
                return Err(rollback_anchor_restore_after_error(
                    &anchor,
                    raw_restore_io(&transaction_staged_user_dir, error),
                ));
            }
        }
        transaction.phase = AnchorRestorePhase::Committed;
        if let Err(error) = write_anchor_restore_transaction(&transaction_root, &transaction) {
            return Err(rollback_anchor_restore_after_error(&anchor, error));
        }
        if let Err(error) = fs::remove_dir_all(&transaction_root) {
            log::warn!(
                "Complete SDK restore committed for Computer '{}', but transaction cleanup failed: {}",
                instance_id,
                error
            );
        }
        Ok(())
    }

    /// Migrates the breaking automatic-only HTTP OAuth schema before the candidate SDK validates
    /// the project files. The raw document path preserves unknown fields and both project layers.
    /// A legacy explicit OAuth opt-out cannot be represented by the new SDK, so an otherwise
    /// unauthenticated server is conservatively disabled instead of silently enabling OAuth.
    pub(crate) fn migrate_removed_http_oauth_fields(
        &self,
        instance_id: &str,
    ) -> Result<RemovedHttpOAuthMigration, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            let mut document = self.load_raw_project_config_unlocked(instance_id)?;
            let migration = strip_removed_http_oauth_fields(&mut document);
            if migration.removed_fields > 0 {
                self.restore_raw_project_config_unlocked(instance_id, &document)?;
            }
            Ok(migration)
        })
    }

    /// Bundle identities declared in this Computer's durable project/local MCP files.
    ///
    /// The merged SDK snapshot can project an enabled plugin over an independent declaration
    /// with the same bundle identity. Marketplace teardown still needs the lower durable layer so
    /// disabling that plugin never unmounts the independent declaration it depended on.
    pub(crate) fn project_mcp_bundle_ids(
        &self,
        instance_id: &str,
    ) -> Result<HashSet<a2c_smcp::smcp_computer::mcp_clients::model::BundleId>, ConfigCrudError>
    {
        let document = self.load_raw_project_config(instance_id)?;
        let mut bundle_ids = HashSet::new();
        for mcp in [document.mcp, document.mcp_local].into_iter().flatten() {
            let Some(Value::Object(servers)) = mcp.get("servers") else {
                continue;
            };
            for (name, body) in servers {
                let Value::Object(mut config) = body.clone() else {
                    continue;
                };
                config
                    .entry("name".to_string())
                    .or_insert_with(|| Value::String(name.clone()));
                if let Ok(config) = serde_json::from_value::<MCPServerConfig>(Value::Object(config))
                {
                    bundle_ids.insert(resolve_bundle_id(&config));
                }
            }
        }
        Ok(bundle_ids)
    }

    /// Replaces all four SDK project-anchor files from a raw same-machine snapshot.
    pub(crate) fn restore_raw_project_config(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<(), ConfigCrudError> {
        self.with_config_write(instance_id, || {
            self.restore_raw_project_config_unlocked(instance_id, document)
        })
    }

    fn restore_raw_project_config_unlocked(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<(), ConfigCrudError> {
        let anchor = self.project_anchor(instance_id);
        recover_raw_restore_transaction(&anchor)?;
        #[cfg(test)]
        if self.fail_next_raw_restore.swap(false, Ordering::SeqCst) {
            return Err(ConfigCrudError::Io {
                path: anchor,
                reason: "injected raw SDK restore failure".to_string(),
            });
        }

        let transaction_root = raw_restore_transaction_root(&anchor)?;
        fs::create_dir_all(&transaction_root)
            .map_err(|error| raw_restore_io(&transaction_root, error))?;
        let staged_anchor = transaction_root.join("staged-anchor");
        let staged_settings_dir = staged_anchor.join(TFROBOT_DIRNAME);
        let current_settings_dir = anchor.join(TFROBOT_DIRNAME);
        let previous_settings_dir = transaction_root.join("previous-settings");
        let mut transaction = RawRestoreTransaction {
            phase: RawRestorePhase::Prepared,
            original_settings_dir_existed: current_settings_dir.exists(),
        };
        write_raw_restore_transaction(&transaction_root, &transaction)?;

        if current_settings_dir.exists() {
            copy_directory_tree(&current_settings_dir, &staged_settings_dir)?;
        }
        delete_config(&staged_anchor)?;
        save_config(&staged_anchor, document)?;
        if load_project_config_doc(&staged_anchor)? != *document {
            return Err(ConfigCrudError::Io {
                path: staged_anchor,
                reason: "staged raw SDK restore does not match the requested document".to_string(),
            });
        }

        if current_settings_dir.exists() {
            fs::rename(&current_settings_dir, &previous_settings_dir)
                .map_err(|error| raw_restore_io(&current_settings_dir, error))?;
        }
        transaction.phase = RawRestorePhase::PreviousMoved;
        if let Err(error) = write_raw_restore_transaction(&transaction_root, &transaction) {
            return Err(rollback_raw_restore_after_error(&anchor, error));
        }

        #[cfg(test)]
        if self
            .fail_next_raw_restore_after_backup
            .swap(false, Ordering::SeqCst)
        {
            let error = ConfigCrudError::Io {
                path: anchor,
                reason: "injected raw SDK restore failure after backup".to_string(),
            };
            return Err(rollback_raw_restore_after_error(
                &self.project_anchor(instance_id),
                error,
            ));
        }

        if staged_settings_dir.exists() {
            fs::create_dir_all(&anchor).map_err(|error| raw_restore_io(&anchor, error))?;
            if let Err(error) = fs::rename(&staged_settings_dir, &current_settings_dir) {
                return Err(rollback_raw_restore_after_error(
                    &anchor,
                    raw_restore_io(&staged_settings_dir, error),
                ));
            }
        }
        transaction.phase = RawRestorePhase::Committed;
        if let Err(error) = write_raw_restore_transaction(&transaction_root, &transaction) {
            return Err(rollback_raw_restore_after_error(&anchor, error));
        }
        if let Err(error) = fs::remove_dir_all(&transaction_root) {
            log::warn!(
                "Raw SDK restore committed for Computer '{}', but transaction cleanup failed: {}",
                instance_id,
                error
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_raw_restore_failure(&self) {
        self.fail_next_raw_restore.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn inject_raw_restore_failure_after_backup(&self) {
        self.fail_next_raw_restore_after_backup
            .store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn inject_anchor_restore_failure_after_project_backup(&self) {
        self.fail_next_anchor_restore_after_project_backup
            .store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn pause_anchor_restore_after_project_backup(
        &self,
    ) -> (Arc<std::sync::Barrier>, Arc<std::sync::Barrier>) {
        let reached = Arc::new(std::sync::Barrier::new(2));
        let resume = Arc::new(std::sync::Barrier::new(2));
        *self
            .anchor_restore_pause_after_project_backup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((reached.clone(), resume.clone()));
        (reached, resume)
    }

    /// Export every reconciled instance-owned MCP declaration with user-authored literals intact.
    ///
    /// SDK shareable export intentionally omits local scopes. The client's CLI-native export is
    /// a full backup, so it resolves User/Project/Local declarations without ambient Policy and
    /// validates the typed declarations without applying the SDK's shareable redaction boundary.
    pub fn export_cli_native_mcp(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, SdkConfigPortabilityError> {
        self.with_config_read(instance_id, || {
            let context = self.context(instance_id);
            let source_anchor = context.project_anchor.clone();
            let resolution_staging = tempfile::tempdir().map_err(|error| ConfigCrudError::Io {
                path: source_anchor.clone(),
                reason: format!("failed to create CLI-native export staging directory: {error}"),
            })?;
            let mut resolved =
                resolve_portable_mcp_without_policy(&context, resolution_staging.path());
            let export_errors = std::mem::take(&mut resolved.errors)
                .into_iter()
                .filter(|error| error.field != "inputs" && !error.field.starts_with("inputs."))
                .collect::<Vec<_>>();
            if !export_errors.is_empty() {
                return Err(SdkConfigPortabilityError::invalid_source(export_errors));
            }

            let servers = resolved
                .servers
                .into_values()
                .map(|server| server.config)
                .collect::<Vec<_>>();
            let prepared = self.prepare_literal_preserving_mcp_configs(&servers)?;
            let inputs = resolved
                .inputs
                .iter()
                .map(input_definition_from_sdk)
                .collect::<Vec<_>>();
            let mut document = project_document_from_servers(&prepared)?;
            replace_project_inputs(&mut document, &inputs)?;
            Ok(document)
        })
    }

    pub fn import(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<ValidationReport, ConfigCrudError> {
        self.with_config_write(instance_id, || {
            import_config(&self.project_anchor(instance_id), document)
        })
    }

    /// Applies the SDK import boundary in an isolated staging directory.
    ///
    /// The client import UX has merge semantics, so it cannot replace the live project document
    /// wholesale through `import_config`. Staging delegates known secret-surface redaction,
    /// local-scope exclusion, and schema validation to the SDK. The client then rejects plaintext
    /// that remains in structured sensitive arguments or URL query parameters before merging.
    pub fn prepare_import(
        &self,
        document: &ProjectConfigDoc,
    ) -> Result<(ProjectConfigDoc, ValidationReport), SdkConfigPortabilityError> {
        // Reject structured plaintext before even creating the staging directory: the SDK
        // sanitizer deliberately does not own CLI arguments or URL query parameters.
        ensure_portable_secret_references(document)?;
        let staging = tempfile::tempdir().map_err(|error| ConfigCrudError::Io {
            path: PathBuf::from("<config-import-staging>"),
            reason: format!("failed to create config import staging directory: {error}"),
        })?;
        let report = import_config(staging.path(), document)?;
        let sanitized = export_config(staging.path())?;
        ensure_portable_secret_references(&sanitized)?;
        Ok((sanitized, report))
    }

    /// Converts MCP declarations through the SDK-owned sanitized portability boundary used for
    /// untrusted projections such as remote Client Control responses.
    ///
    /// The preflight guard rejects sensitive CLI/query plaintext before the SDK staging write;
    /// the SDK then redacts env, headers, URL userinfo, and password defaults and validates only
    /// the configuration schema. Callers receive typed, canonicalized declarations that are safe
    /// to persist in SDK config or in the client's crash-recovery journal.
    pub fn prepare_portable_mcp_configs(
        &self,
        servers: &[MCPServerConfig],
    ) -> Result<Vec<MCPServerConfig>, SdkConfigPortabilityError> {
        let document = project_document_from_servers(servers)?;
        let (sanitized, report) = self.prepare_import(&document)?;
        if !report.is_valid() {
            return Err(SdkConfigPortabilityError::invalid_source(report.errors));
        }
        Ok(mcp_configs_from_project_document(sanitized)?)
    }

    /// Canonicalizes and validates user-authored MCP declarations without altering literals.
    ///
    /// Trusted local CRUD and explicit client import/export share this boundary: environment
    /// variables, headers, and URL userinfo may be intentional plaintext constants and must
    /// round-trip exactly. Remote Client Control projections continue to use
    /// [`Self::prepare_portable_mcp_configs`] so this does not widen that trust boundary.
    pub fn prepare_literal_preserving_mcp_configs(
        &self,
        servers: &[MCPServerConfig],
    ) -> Result<Vec<MCPServerConfig>, SdkConfigPortabilityError> {
        let document = project_document_from_servers(servers)?;

        // Keep the existing structured argument/query guard. Unlike env/header values, these
        // locations have no dedicated value-source editor and should still require references
        // when they carry secret intent.
        ensure_portable_secret_references(&document)?;
        let report = validate_config(&document);
        if !report.is_valid() {
            return Err(SdkConfigPortabilityError::invalid_source(report.errors));
        }
        Ok(mcp_configs_from_project_document(document)?)
    }

    /// Decodes an SDK configuration document through its canonical typed boundary.
    pub fn mcp_configs_from_portable_document(
        document: ProjectConfigDoc,
    ) -> Result<Vec<MCPServerConfig>, SdkConfigPortabilityError> {
        Ok(mcp_configs_from_project_document(document)?)
    }

    /// Decodes the merged Input definitions carried by a CLI-native export document.
    pub fn input_definitions_from_portable_document(
        document: &ProjectConfigDoc,
    ) -> Result<Vec<InputDefinition>, SdkConfigPortabilityError> {
        let Some(encoded) = document.mcp.as_ref().and_then(|mcp| mcp.get("inputs")) else {
            return Ok(Vec::new());
        };
        let definitions = serde_json::from_value::<
            Vec<a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput>,
        >(encoded.clone())
        .map_err(|error| ConfigCrudError::Io {
            path: PathBuf::from("<portable-sdk-config>"),
            reason: format!("invalid portable SDK MCP input definitions: {error}"),
        })?;
        Ok(definitions.iter().map(input_definition_from_sdk).collect())
    }

    /// Redacts the MCP portion of a reconciled snapshot before it crosses the Client Control
    /// boundary. The trusted local WebView uses the raw projection so its editor can round-trip
    /// intentional constants; remote automation receives only SDK-sanitized declarations.
    pub fn sanitize_snapshot_for_client_control(
        &self,
        mut snapshot: ComputerConfigSnapshot,
    ) -> Result<ComputerConfigSnapshot, SdkConfigPortabilityError> {
        let configs = snapshot
            .mcp
            .servers
            .iter()
            .map(|server| server.config.clone())
            .collect::<Vec<_>>();
        let mut sanitized = self
            .prepare_portable_mcp_configs(&configs)?
            .into_iter()
            .map(|config| (config.name().to_string(), config))
            .collect::<std::collections::HashMap<_, _>>();
        for server in &mut snapshot.mcp.servers {
            server.config = sanitized
                .remove(&server.name)
                .ok_or_else(|| ConfigCrudError::Io {
                    path: PathBuf::from("<in-memory-mcp-config>"),
                    reason: format!(
                        "SDK Client Control sanitizer omitted MCP server '{}'",
                        server.name
                    ),
                })?;
        }
        Ok(snapshot)
    }

    pub fn owns_path(&self, instance_id: &str, path: &Path) -> bool {
        path.starts_with(self.project_anchor(instance_id))
            || path.starts_with(self.skill_home(instance_id))
    }
}

fn strip_removed_http_oauth_fields(document: &mut ProjectConfigDoc) -> RemovedHttpOAuthMigration {
    let mut migration = RemovedHttpOAuthMigration::default();
    for layer in [&mut document.mcp, &mut document.mcp_local] {
        let Some(layer) = layer.as_mut() else {
            continue;
        };
        let Some(Value::Object(servers)) = layer.get_mut("servers") else {
            continue;
        };
        for server in servers.values_mut() {
            let Value::Object(body) = server else {
                continue;
            };
            let is_http = body
                .get("server_parameters")
                .and_then(Value::as_object)
                .and_then(|parameters| parameters.get("url"))
                .and_then(Value::as_str)
                .is_some();
            if !is_http {
                continue;
            }

            let has_static_authorization = body
                .get("server_parameters")
                .and_then(Value::as_object)
                .and_then(|parameters| parameters.get("headers"))
                .and_then(Value::as_object)
                .is_some_and(|headers| {
                    headers
                        .keys()
                        .any(|header| header.eq_ignore_ascii_case("authorization"))
                });
            let explicit_opt_out = body.get("oauth") == Some(&Value::Bool(false))
                || ["authPolicy", "auth_policy"].into_iter().any(|field| {
                    body.get(field)
                        .and_then(Value::as_str)
                        .is_some_and(|policy| policy.eq_ignore_ascii_case("disabled"))
                });
            if explicit_opt_out && !has_static_authorization {
                let was_disabled = body.get("disabled") == Some(&Value::Bool(true));
                body.insert("disabled".to_string(), Value::Bool(true));
                if !was_disabled {
                    migration.disabled_opt_out_servers += 1;
                }
            }

            for field in ["oauth", "authPolicy", "auth_policy"] {
                if body.remove(field).is_some() {
                    migration.removed_fields += 1;
                }
            }
        }
    }
    migration
}

/// The SDK sanitizer owns known secret-bearing value fields (env, headers, URL userinfo, and
/// password input defaults). This guard covers the remaining structured locations where a client
/// can identify secret intent without guessing whether arbitrary command text is sensitive.
fn ensure_portable_secret_references(
    document: &ProjectConfigDoc,
) -> Result<(), SdkConfigPortabilityError> {
    let Some(servers) = document
        .mcp
        .as_ref()
        .and_then(|mcp| mcp.get("servers"))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };

    let mut unsafe_fields = Vec::new();
    for (name, server) in servers {
        let Some(parameters) = server.get("server_parameters").and_then(Value::as_object) else {
            continue;
        };
        if let Some(args) = parameters.get("args").and_then(Value::as_array) {
            let args = args.iter().map(Value::as_str).collect::<Vec<_>>();
            collect_unsafe_argument_fields(
                &format!("servers.{name}.server_parameters.args"),
                &args,
                &mut unsafe_fields,
            );
        }
        if let Some(raw_url) = parameters.get("url").and_then(Value::as_str) {
            collect_unsafe_query_fields(
                &format!("servers.{name}.server_parameters.url"),
                raw_url,
                &mut unsafe_fields,
            );
        }
    }

    if unsafe_fields.is_empty() {
        Ok(())
    } else {
        Err(SdkConfigPortabilityError::unsafe_plaintext(unsafe_fields))
    }
}

fn collect_unsafe_argument_fields(
    field_prefix: &str,
    args: &[Option<&str>],
    unsafe_fields: &mut Vec<String>,
) {
    for (index, argument) in args.iter().enumerate() {
        let Some(argument) = argument else {
            continue;
        };
        collect_unsafe_query_fields(&format!("{field_prefix}[{index}]"), argument, unsafe_fields);
        if let Some((flag, value)) = argument.split_once('=') {
            if is_sensitive_identifier(flag) && !is_portable_secret_reference(value) {
                unsafe_fields.push(format!("{field_prefix}[{index}] ({flag})"));
            }
            continue;
        }
        if !argument.starts_with('-') || !is_sensitive_identifier(argument) {
            continue;
        }
        let Some(Some(value)) = args.get(index + 1) else {
            continue;
        };
        if !is_portable_secret_reference(value) {
            unsafe_fields.push(format!(
                "{field_prefix}[{}] (value for {argument})",
                index + 1
            ));
        }
    }
}

pub(crate) fn ensure_portable_cli_arguments(
    field_prefix: &str,
    args: &[String],
) -> Result<(), SdkConfigPortabilityError> {
    let args = args
        .iter()
        .map(|argument| Some(argument.as_str()))
        .collect::<Vec<_>>();
    let mut unsafe_fields = Vec::new();
    collect_unsafe_argument_fields(field_prefix, &args, &mut unsafe_fields);
    if unsafe_fields.is_empty() {
        Ok(())
    } else {
        Err(SdkConfigPortabilityError::unsafe_plaintext(unsafe_fields))
    }
}

fn collect_unsafe_query_fields(field_prefix: &str, raw_url: &str, unsafe_fields: &mut Vec<String>) {
    let before_fragment = raw_url.split_once('#').map_or(raw_url, |(url, _)| url);
    let Some((_, query)) = before_fragment.split_once('?') else {
        return;
    };
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if !value.is_empty() && !is_portable_secret_reference(&value) {
            unsafe_fields.push(format!("{field_prefix} query parameter '{key}'"));
        }
    }
}

pub(crate) fn is_writable_provenance(scope: ProvenanceScope) -> bool {
    matches!(
        scope,
        ProvenanceScope::User | ProvenanceScope::Project | ProvenanceScope::Local
    )
}

fn is_sensitive_identifier(raw: &str) -> bool {
    let normalized = raw
        .trim_start_matches('-')
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "credentials",
        "authorization",
        "auth",
        "apikey",
        "accesskey",
        "privatekey",
        "clientsecret",
        "signature",
        "sig",
    ]
    .iter()
    .any(|suffix| normalized == *suffix || normalized.ends_with(suffix))
}

fn is_portable_secret_reference(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut remaining = value;
    while let Some(token) = remaining.strip_prefix("${") {
        let Some(end) = token.find('}') else {
            return false;
        };
        let reference = &token[..end];
        if reference != "REDACTED"
            && reference
                .strip_prefix("input:")
                .is_none_or(|name| name.is_empty())
            && reference
                .strip_prefix("env:")
                .is_none_or(|name| name.is_empty())
        {
            return false;
        }
        remaining = &token[end + 1..];
    }
    remaining.is_empty()
}

fn canonical_mcp_server_body(value: Value) -> Result<Map<String, Value>, String> {
    let mut body = value
        .as_object()
        .cloned()
        .ok_or_else(|| "serialized config must be an object".to_string())?;
    body.remove("name");
    if let Some(Value::String(server_type)) = body.get_mut("type") {
        *server_type = match server_type.as_str() {
            "Stdio" | "STDIO" => "stdio",
            "Sse" | "SSE" => "sse",
            "Http" | "HTTP" | "http" => "streamable",
            other => other,
        }
        .to_string();
    }
    Ok(body)
}

fn mcp_upsert_edits(
    context: &InstanceConfigContext,
    servers: &[MCPServerConfig],
) -> Result<Vec<ConfigEdit>, ConfigCrudError> {
    servers
        .iter()
        .map(|server| {
            let name = server.name().to_string();
            let value = serde_json::to_value(server).map_err(|error| ConfigCrudError::Io {
                path: context.project_anchor().to_path_buf(),
                reason: format!("failed to serialize MCP server '{name}': {error}"),
            })?;
            let body = canonical_mcp_server_body(value).map_err(|reason| ConfigCrudError::Io {
                path: context.project_anchor().to_path_buf(),
                reason: format!("invalid MCP server '{name}': {reason}"),
            })?;
            Ok(ConfigEdit::new(
                ConfigEntity::McpServer(name),
                EditIntent::Upsert(Value::Object(body)),
            ))
        })
        .collect()
}

fn replace_project_inputs(
    document: &mut ProjectConfigDoc,
    definitions: &[InputDefinition],
) -> Result<(), ConfigCrudError> {
    let encoded = definitions
        .iter()
        .map(input_definition_to_sdk)
        .map(|definition| {
            serde_json::to_value(definition).map_err(|error| ConfigCrudError::Io {
                path: PathBuf::from("<staged-mcp-config>"),
                reason: format!("failed to serialize MCP input definition: {error}"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    document
        .mcp
        .get_or_insert_with(Map::new)
        .insert("inputs".to_string(), Value::Array(encoded));
    Ok(())
}

fn remove_unreferenced_project_inputs(
    document: &mut ProjectConfigDoc,
    candidates: &HashSet<String>,
    referenced: &HashSet<String>,
) {
    if candidates.is_empty() {
        return;
    }
    let Some(Value::Array(inputs)) = document.mcp.as_mut().and_then(|mcp| mcp.get_mut("inputs"))
    else {
        return;
    };
    inputs.retain(|input| {
        let Some(id) = input.get("id").and_then(Value::as_str) else {
            return true;
        };
        !candidates.contains(id) || referenced.contains(id)
    });
}

fn all_scope_input_references(
    context: &InstanceConfigContext,
    project_document: &ProjectConfigDoc,
) -> Result<HashSet<String>, ConfigCrudError> {
    let mut referenced = find_project_input_references(project_document)
        .into_iter()
        .map(|location| location.input_id)
        .collect::<HashSet<_>>();

    // User declarations can be hidden by a Project/Local server with the same name and would not
    // appear in the merged snapshot. Scan the raw User layer as well so GC remains conservative.
    let user_path = user_mcp_config_path(Some(context.env()));
    match fs::read(&user_path) {
        Ok(bytes) => {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|error| ConfigCrudError::Io {
                    path: user_path.clone(),
                    reason: format!(
                        "invalid User MCP config while checking Input references: {error}"
                    ),
                })?;
            if let Some(servers) = value.get("servers").and_then(Value::as_object) {
                for server in servers.values() {
                    for field in [server.get("server_parameters"), server.get("envFile")]
                        .into_iter()
                        .flatten()
                    {
                        referenced.extend(referenced_input_ids(field));
                    }
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(raw_restore_io(&user_path, error)),
    }

    // The resolved view contributes read-only ambient scopes (notably Policy). Writable raw
    // layers were scanned above, including declarations currently hidden by a higher layer.
    for server in context.load().mcp.servers {
        let value = serde_json::to_value(server.config).map_err(|error| ConfigCrudError::Io {
            path: context.project_anchor().to_path_buf(),
            reason: format!("failed to inspect MCP Input references: {error}"),
        })?;
        for field in [value.get("server_parameters"), value.get("envFile")]
            .into_iter()
            .flatten()
        {
            referenced.extend(referenced_input_ids(field));
        }
    }
    Ok(referenced)
}

fn ensure_edited_inputs_are_effective(
    context: &InstanceConfigContext,
    project_inputs: &[InputDefinition],
    edited_input_ids: &HashSet<String>,
) -> Result<(), ConfigCrudError> {
    if edited_input_ids.is_empty() {
        return Ok(());
    }
    let effective = context
        .load()
        .inputs
        .inputs
        .into_iter()
        .map(|definition| (definition.id().to_string(), definition))
        .collect::<std::collections::HashMap<_, _>>();
    for expected in project_inputs
        .iter()
        .filter(|definition| edited_input_ids.contains(definition.id()))
    {
        if effective.get(expected.id()) != Some(&input_definition_to_sdk(expected)) {
            return Err(ConfigCrudError::Io {
                path: context.project_anchor().to_path_buf(),
                reason: format!(
                    "Input definition '{}' is shadowed by a higher-priority SDK scope and cannot be edited from this MCP form",
                    expected.id()
                ),
            });
        }
    }
    Ok(())
}

fn project_document_from_servers(
    servers: &[MCPServerConfig],
) -> Result<ProjectConfigDoc, ConfigCrudError> {
    let mut portable_servers = Map::new();
    for server in servers {
        let name = server.name().to_string();
        let value = serde_json::to_value(server).map_err(|error| ConfigCrudError::Io {
            path: PathBuf::from("<in-memory-mcp-config>"),
            reason: format!("failed to serialize MCP server '{name}': {error}"),
        })?;
        let body = canonical_mcp_server_body(value).map_err(|reason| ConfigCrudError::Io {
            path: PathBuf::from("<in-memory-mcp-config>"),
            reason: format!("invalid MCP server '{name}': {reason}"),
        })?;
        portable_servers.insert(name, Value::Object(body));
    }
    Ok(ProjectConfigDoc {
        mcp: Some(Map::from_iter([(
            "servers".to_string(),
            Value::Object(portable_servers),
        )])),
        ..Default::default()
    })
}

fn mcp_configs_from_project_document(
    document: ProjectConfigDoc,
) -> Result<Vec<MCPServerConfig>, ConfigCrudError> {
    let Some(mcp) = document.mcp else {
        return Ok(Vec::new());
    };
    let Some(servers) = mcp.get("servers") else {
        return Ok(Vec::new());
    };
    let servers = servers.as_object().ok_or_else(|| ConfigCrudError::Io {
        path: PathBuf::from("<portable-sdk-config>"),
        reason: "portable SDK mcp.servers must be an object".to_string(),
    })?;

    let mut configs = servers
        .iter()
        .map(|(name, value)| {
            let mut body = value
                .as_object()
                .cloned()
                .ok_or_else(|| ConfigCrudError::Io {
                    path: PathBuf::from("<portable-sdk-config>"),
                    reason: format!("portable SDK MCP server '{name}' must be an object"),
                })?;
            if let Some(explicit_name) = body.get("name") {
                if explicit_name.as_str() != Some(name) {
                    return Err(ConfigCrudError::Io {
                        path: PathBuf::from("<portable-sdk-config>"),
                        reason: format!(
                            "portable SDK MCP server key '{name}' conflicts with its name field"
                        ),
                    });
                }
            }
            body.insert("name".to_string(), Value::String(name.clone()));
            serde_json::from_value(Value::Object(body)).map_err(|error| ConfigCrudError::Io {
                path: PathBuf::from("<portable-sdk-config>"),
                reason: format!("invalid portable SDK MCP server '{name}': {error}"),
            })
        })
        .collect::<Result<Vec<MCPServerConfig>, ConfigCrudError>>()?;
    configs.sort_by(|left, right| left.name().cmp(right.name()));
    Ok(configs)
}

fn raw_restore_transaction_root(anchor: &Path) -> Result<PathBuf, ConfigCrudError> {
    let file_name = anchor
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ConfigCrudError::Io {
            path: anchor.to_path_buf(),
            reason: "SDK project anchor has no valid directory name".to_string(),
        })?;
    Ok(anchor.with_file_name(format!(".{file_name}-raw-restore")))
}

fn anchor_restore_transaction_root(anchor: &Path) -> Result<PathBuf, ConfigCrudError> {
    let file_name = anchor
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ConfigCrudError::Io {
            path: anchor.to_path_buf(),
            reason: "SDK project anchor has no valid directory name".to_string(),
        })?;
    Ok(anchor.with_file_name(format!(".{file_name}-sdk-anchor-restore")))
}

fn anchor_restore_transaction_path(transaction_root: &Path) -> PathBuf {
    transaction_root.join("transaction.json")
}

fn write_anchor_restore_transaction(
    transaction_root: &Path,
    transaction: &AnchorRestoreTransaction,
) -> Result<(), ConfigCrudError> {
    write_json_atomically(
        &anchor_restore_transaction_path(transaction_root),
        transaction,
    )
    .map_err(|error| ConfigCrudError::Io {
        path: transaction_root.to_path_buf(),
        reason: error.to_string(),
    })
}

fn load_anchor_restore_transaction(
    transaction_root: &Path,
) -> Result<AnchorRestoreTransaction, ConfigCrudError> {
    let path = anchor_restore_transaction_path(transaction_root);
    let content = fs::read(&path).map_err(|error| raw_restore_io(&path, error))?;
    serde_json::from_slice(&content).map_err(|error| ConfigCrudError::Io {
        path,
        reason: format!("corrupt complete SDK restore transaction: {error}"),
    })
}

fn recover_anchor_restore_transaction(anchor: &Path) -> Result<(), ConfigCrudError> {
    let transaction_root = anchor_restore_transaction_root(anchor)?;
    if !transaction_root.exists() {
        return Ok(());
    }
    let previous_project_dir = transaction_root.join("previous-project");
    let previous_user_dir = transaction_root.join("previous-user");
    let marker_path = anchor_restore_transaction_path(&transaction_root);
    if !marker_path.exists() {
        if previous_project_dir.exists() || previous_user_dir.exists() {
            return Err(ConfigCrudError::Io {
                path: transaction_root,
                reason: "unmarked complete SDK restore retains previous scope directories"
                    .to_string(),
            });
        }
        fs::remove_dir_all(&transaction_root)
            .map_err(|error| raw_restore_io(&transaction_root, error))?;
        return Ok(());
    }
    let transaction = load_anchor_restore_transaction(&transaction_root)?;
    if transaction.phase != AnchorRestorePhase::Committed {
        let current_project_dir = anchor.join(TFROBOT_DIRNAME);
        let current_user_dir = anchor.join("a2c");
        for (current, previous, originally_existed, label) in [
            (
                current_project_dir,
                previous_project_dir,
                transaction.original_project_dir_existed,
                "Project/Local",
            ),
            (
                current_user_dir,
                previous_user_dir,
                transaction.original_user_dir_existed,
                "User",
            ),
        ] {
            if previous.exists() {
                remove_path_if_present(&current)?;
                if let Some(parent) = current.parent() {
                    fs::create_dir_all(parent).map_err(|error| raw_restore_io(parent, error))?;
                }
                fs::rename(&previous, &current)
                    .map_err(|error| raw_restore_io(&previous, error))?;
            } else if transaction.phase == AnchorRestorePhase::PreviousMoved {
                if originally_existed {
                    return Err(ConfigCrudError::Io {
                        path: transaction_root.clone(),
                        reason: format!(
                            "complete SDK restore lost its previous {label} scope directory"
                        ),
                    });
                }
                remove_path_if_present(&current)?;
            }
        }
    }
    fs::remove_dir_all(&transaction_root)
        .map_err(|error| raw_restore_io(&transaction_root, error))?;
    Ok(())
}

fn rollback_anchor_restore_after_error(anchor: &Path, primary: ConfigCrudError) -> ConfigCrudError {
    match recover_anchor_restore_transaction(anchor) {
        Ok(()) => primary,
        Err(rollback) => ConfigCrudError::Io {
            path: anchor.to_path_buf(),
            reason: format!("{primary}; complete SDK restore rollback also failed: {rollback}"),
        },
    }
}

fn raw_restore_transaction_path(transaction_root: &Path) -> PathBuf {
    transaction_root.join("transaction.json")
}

fn write_raw_restore_transaction(
    transaction_root: &Path,
    transaction: &RawRestoreTransaction,
) -> Result<(), ConfigCrudError> {
    write_json_atomically(&raw_restore_transaction_path(transaction_root), transaction).map_err(
        |error| ConfigCrudError::Io {
            path: transaction_root.to_path_buf(),
            reason: error.to_string(),
        },
    )
}

fn load_raw_restore_transaction(
    transaction_root: &Path,
) -> Result<RawRestoreTransaction, ConfigCrudError> {
    let path = raw_restore_transaction_path(transaction_root);
    let content = fs::read(&path).map_err(|error| raw_restore_io(&path, error))?;
    serde_json::from_slice(&content).map_err(|error| ConfigCrudError::Io {
        path,
        reason: format!("corrupt raw SDK restore transaction: {error}"),
    })
}

fn recover_raw_restore_transaction(anchor: &Path) -> Result<(), ConfigCrudError> {
    let transaction_root = raw_restore_transaction_root(anchor)?;
    if !transaction_root.exists() {
        return Ok(());
    }
    let previous_settings_dir = transaction_root.join("previous-settings");
    let marker_path = raw_restore_transaction_path(&transaction_root);
    if !marker_path.exists() {
        if previous_settings_dir.exists() {
            return Err(ConfigCrudError::Io {
                path: transaction_root,
                reason: "unmarked raw SDK restore retains a previous settings directory"
                    .to_string(),
            });
        }
        fs::remove_dir_all(&transaction_root)
            .map_err(|error| raw_restore_io(&transaction_root, error))?;
        return Ok(());
    }
    let transaction = load_raw_restore_transaction(&transaction_root)?;
    let current_settings_dir = anchor.join(TFROBOT_DIRNAME);

    if transaction.phase != RawRestorePhase::Committed {
        if previous_settings_dir.exists() {
            remove_path_if_present(&current_settings_dir)?;
            if let Some(parent) = current_settings_dir.parent() {
                fs::create_dir_all(parent).map_err(|error| raw_restore_io(parent, error))?;
            }
            fs::rename(&previous_settings_dir, &current_settings_dir)
                .map_err(|error| raw_restore_io(&previous_settings_dir, error))?;
        } else if transaction.phase == RawRestorePhase::PreviousMoved {
            if transaction.original_settings_dir_existed {
                return Err(ConfigCrudError::Io {
                    path: transaction_root,
                    reason: "raw SDK restore lost its previous settings directory".to_string(),
                });
            }
            remove_path_if_present(&current_settings_dir)?;
        }
    }

    fs::remove_dir_all(&transaction_root)
        .map_err(|error| raw_restore_io(&transaction_root, error))?;
    Ok(())
}

fn rollback_raw_restore_after_error(anchor: &Path, primary: ConfigCrudError) -> ConfigCrudError {
    match recover_raw_restore_transaction(anchor) {
        Ok(()) => primary,
        Err(rollback) => ConfigCrudError::Io {
            path: anchor.to_path_buf(),
            reason: format!("{primary}; raw SDK restore rollback also failed: {rollback}"),
        },
    }
}

fn copy_directory_tree(source: &Path, destination: &Path) -> Result<(), ConfigCrudError> {
    fs::create_dir_all(destination).map_err(|error| raw_restore_io(destination, error))?;
    for entry in fs::read_dir(source).map_err(|error| raw_restore_io(source, error))? {
        let entry = entry.map_err(|error| raw_restore_io(source, error))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| raw_restore_io(&source_path, error))?;
        if file_type.is_dir() {
            copy_directory_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| raw_restore_io(&source_path, error))?;
        } else {
            return Err(ConfigCrudError::Io {
                path: source_path,
                reason: "raw SDK restore cannot stage symbolic links or special files".to_string(),
            });
        }
    }
    Ok(())
}

fn remove_path_if_present(path: &Path) -> Result<(), ConfigCrudError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(raw_restore_io(path, error)),
    };
    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|error| raw_restore_io(path, error))
    } else {
        fs::remove_file(path).map_err(|error| raw_restore_io(path, error))
    }
}

fn raw_restore_io(path: &Path, error: impl std::fmt::Display) -> ConfigCrudError {
    ConfigCrudError::Io {
        path: path.to_path_buf(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2c_smcp::smcp_computer::settings::config::{ConfigEntity, EditIntent, ProjectConfigDoc};
    use serde_json::json;
    use tempfile::tempdir;

    fn project_doc_with_marker(marker: &str) -> ProjectConfigDoc {
        ProjectConfigDoc {
            settings: Some(json!({"marker": marker}).as_object().unwrap().clone()),
            mcp: Some(
                json!({
                    "servers": {
                        marker: {
                            "type": "stdio",
                            "server_parameters": {"command": marker}
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..ProjectConfigDoc::default()
        }
    }

    #[test]
    fn project_input_crud_never_copies_or_edits_local_scope_definitions() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        sdk_config
            .save(
                "computer-a",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "inputs": [{
                                "type": "PromptString",
                                "id": "project-token",
                                "description": "Project token"
                            }]
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    mcp_local: Some(
                        json!({
                            "inputs": [{
                                "type": "PromptString",
                                "id": "local-token",
                                "description": "Local token"
                            }]
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let project = sdk_config
            .load_project_input_definitions("computer-a")
            .unwrap();
        assert_eq!(project.len(), 1);
        assert_eq!(project[0].id(), "project-token");

        sdk_config
            .replace_input_definitions(
                "computer-a",
                &[InputDefinition::PromptString {
                    id: "replacement".to_string(),
                    label: Some("Replacement".to_string()),
                    description: None,
                    default: None,
                    password: Some(false),
                }],
            )
            .unwrap();

        let raw = sdk_config.load_raw_project_config("computer-a").unwrap();
        assert_eq!(raw.mcp.as_ref().unwrap()["inputs"][0]["id"], "replacement");
        assert_eq!(
            raw.mcp_local.as_ref().unwrap()["inputs"][0]["id"],
            "local-token"
        );
    }

    fn legacy_http_oauth_document() -> ProjectConfigDoc {
        ProjectConfigDoc {
            settings: Some(
                json!({"futureSetting": {"preserve": true}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            mcp: Some(
                json!({
                    "futureLayerField": "preserve-me",
                    "servers": {
                        "remote": {
                            "type": "streamable",
                            "authPolicy": "auto",
                            "oauth": {
                                "resource": "https://mcp.example/mcp",
                                "client_name": "TFRobot"
                            },
                            "futureServerField": {"preserve": true},
                            "server_parameters": {
                                "url": "https://mcp.example/mcp",
                                "headers": {"X-Routing": "preserve-me"}
                            }
                        },
                        "static": {
                            "type": "streamable",
                            "auth_policy": "disabled",
                            "server_parameters": {
                                "url": "https://static.example/mcp",
                                "headers": {"authorization": "Bearer ${input:token}"}
                            }
                        },
                        "stdio": {
                            "type": "stdio",
                            "oauth": {"extensionOwned": true},
                            "server_parameters": {"command": "node"}
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            mcp_local: Some(
                json!({
                    "servers": {
                        "opt-out": {
                            "type": "sse",
                            "oauth": false,
                            "server_parameters": {"url": "https://public.example/sse"}
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..ProjectConfigDoc::default()
        }
    }

    #[test]
    fn removed_http_oauth_migration_is_selective_and_idempotent() {
        let mut document = legacy_http_oauth_document();
        let first = strip_removed_http_oauth_fields(&mut document);
        assert_eq!(first.removed_fields, 4);
        assert_eq!(first.disabled_opt_out_servers, 1);
        assert!(document.mcp.as_ref().unwrap()["servers"]["remote"]
            .get("oauth")
            .is_none());
        assert!(document.mcp.as_ref().unwrap()["servers"]["remote"]
            .get("authPolicy")
            .is_none());
        assert_eq!(
            document.mcp.as_ref().unwrap()["servers"]["remote"]["futureServerField"],
            json!({"preserve": true})
        );
        assert_eq!(
            document.mcp.as_ref().unwrap()["servers"]["static"]["server_parameters"]["headers"]
                ["authorization"],
            "Bearer ${input:token}"
        );
        assert_eq!(
            document.mcp.as_ref().unwrap()["servers"]["stdio"]["oauth"],
            json!({"extensionOwned": true})
        );
        assert_eq!(
            document.mcp_local.as_ref().unwrap()["servers"]["opt-out"]["disabled"],
            true
        );

        assert_eq!(
            strip_removed_http_oauth_fields(&mut document),
            RemovedHttpOAuthMigration::default()
        );
    }

    #[test]
    fn removed_http_oauth_migration_persists_through_raw_transaction() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        sdk_config
            .save("computer-a", &legacy_http_oauth_document())
            .unwrap();

        let first = sdk_config
            .migrate_removed_http_oauth_fields("computer-a")
            .unwrap();
        assert_eq!(first.removed_fields, 4);
        assert_eq!(first.disabled_opt_out_servers, 1);
        assert!(sdk_config
            .validate_instance("computer-a")
            .unwrap()
            .is_valid());
        assert_eq!(
            sdk_config
                .migrate_removed_http_oauth_fields("computer-a")
                .unwrap(),
            RemovedHttpOAuthMigration::default()
        );
    }

    #[test]
    fn raw_restore_replaces_sdk_files_and_preserves_non_sdk_files() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let before = project_doc_with_marker("before");
        let after = project_doc_with_marker("after");
        sdk_config.save("computer-a", &before).unwrap();
        let extension = sdk_config
            .project_anchor("computer-a")
            .join(TFROBOT_DIRNAME)
            .join("extension-state.bin");
        fs::write(&extension, b"preserve-me").unwrap();

        sdk_config
            .restore_raw_project_config("computer-a", &after)
            .unwrap();

        assert_eq!(
            sdk_config.load_raw_project_config("computer-a").unwrap(),
            after
        );
        assert_eq!(fs::read(extension).unwrap(), b"preserve-me");
    }

    #[test]
    fn complete_sdk_restore_rolls_back_project_and_user_scopes_together() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let before = project_doc_with_marker("before");
        let after = project_doc_with_marker("after");
        sdk_config.save("computer-a", &before).unwrap();
        let anchor = sdk_config.project_anchor("computer-a");
        let user_path = anchor.join("a2c/mcp.json");
        let client_profile = anchor.join("profile.json");
        fs::create_dir_all(user_path.parent().unwrap()).unwrap();
        fs::write(&user_path, br#"{"marker":"before-user"}"#).unwrap();
        fs::write(&client_profile, br#"{"name":"before-stage"}"#).unwrap();

        let (_staging, staged_anchor) = sdk_config.stage_complete_sdk_anchor("computer-a").unwrap();
        save_config(&staged_anchor, &after).unwrap();
        let staged_user = staged_anchor.join("a2c/mcp.json");
        fs::write(&staged_user, br#"{"marker":"after-user"}"#).unwrap();
        fs::write(&client_profile, br#"{"name":"client-owned-update"}"#).unwrap();
        sdk_config.inject_raw_restore_failure_after_backup();
        sdk_config
            .commit_complete_sdk_anchor("computer-a", &staged_anchor)
            .unwrap_err();

        assert_eq!(
            sdk_config.load_raw_project_config("computer-a").unwrap(),
            before
        );
        assert_eq!(
            fs::read_to_string(user_path).unwrap(),
            r#"{"marker":"before-user"}"#
        );
        assert_eq!(
            fs::read_to_string(client_profile).unwrap(),
            r#"{"name":"client-owned-update"}"#
        );
        assert!(!anchor_restore_transaction_root(&anchor).unwrap().exists());
    }

    #[test]
    fn complete_sdk_restore_immediately_rolls_back_between_live_scope_backups() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let before = project_doc_with_marker("before");
        let after = project_doc_with_marker("after");
        sdk_config.save("computer-a", &before).unwrap();
        let anchor = sdk_config.project_anchor("computer-a");
        let user_path = anchor.join("a2c/mcp.json");
        fs::create_dir_all(user_path.parent().unwrap()).unwrap();
        fs::write(&user_path, br#"{"marker":"before-user"}"#).unwrap();

        let (_staging, staged_anchor) = sdk_config.stage_complete_sdk_anchor("computer-a").unwrap();
        save_config(&staged_anchor, &after).unwrap();
        let staged_user = staged_anchor.join("a2c/mcp.json");
        fs::write(&staged_user, br#"{"marker":"after-user"}"#).unwrap();
        sdk_config.inject_anchor_restore_failure_after_project_backup();

        sdk_config
            .commit_complete_sdk_anchor("computer-a", &staged_anchor)
            .unwrap_err();

        // No follow-up mutation or recovery call is needed before ordinary reads see old state.
        let snapshot = sdk_config.load("computer-a");
        assert!(snapshot
            .mcp
            .servers
            .iter()
            .any(|server| server.name == "before"));
        assert!(!snapshot
            .mcp
            .servers
            .iter()
            .any(|server| server.name == "after"));
        assert_eq!(
            fs::read_to_string(user_path).unwrap(),
            r#"{"marker":"before-user"}"#
        );
        assert!(!anchor_restore_transaction_root(&anchor).unwrap().exists());
    }

    #[test]
    fn complete_sdk_restore_isolates_merged_and_recovery_reads_until_commit() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let before = project_doc_with_marker("before");
        let after = project_doc_with_marker("after");
        sdk_config.save("computer-a", &before).unwrap();
        sdk_config
            .save("computer-b", &project_doc_with_marker("other"))
            .unwrap();
        let anchor = sdk_config.project_anchor("computer-a");
        let user_path = anchor.join("a2c/mcp.json");
        fs::create_dir_all(user_path.parent().unwrap()).unwrap();
        fs::write(&user_path, br#"{"marker":"before-user"}"#).unwrap();
        let (_staging, staged_anchor) = sdk_config.stage_complete_sdk_anchor("computer-a").unwrap();
        save_config(&staged_anchor, &after).unwrap();
        fs::write(
            staged_anchor.join("a2c/mcp.json"),
            br#"{"marker":"after-user"}"#,
        )
        .unwrap();

        let (commit_reached, resume_commit) =
            sdk_config.pause_anchor_restore_after_project_backup();
        let commit_service = sdk_config.clone();
        let commit_anchor = staged_anchor.clone();
        let commit = std::thread::spawn(move || {
            commit_service.commit_complete_sdk_anchor("computer-a", &commit_anchor)
        });
        commit_reached.wait();

        let (merged_started_tx, merged_started_rx) = std::sync::mpsc::channel();
        let (merged_done_tx, merged_done_rx) = std::sync::mpsc::channel();
        let merged_service = sdk_config.clone();
        let merged_reader = std::thread::spawn(move || {
            merged_started_tx.send(()).unwrap();
            let snapshot = merged_service.load("computer-a");
            merged_done_tx.send(snapshot).unwrap();
        });
        merged_started_rx.recv().unwrap();

        let (raw_started_tx, raw_started_rx) = std::sync::mpsc::channel();
        let (raw_done_tx, raw_done_rx) = std::sync::mpsc::channel();
        let raw_service = sdk_config.clone();
        let raw_reader = std::thread::spawn(move || {
            raw_started_tx.send(()).unwrap();
            let document = raw_service.load_raw_project_config("computer-a");
            raw_done_tx.send(document).unwrap();
        });
        raw_started_rx.recv().unwrap();

        let (other_done_tx, other_done_rx) = std::sync::mpsc::channel();
        let other_service = sdk_config.clone();
        let other_reader = std::thread::spawn(move || {
            other_done_tx
                .send(other_service.load("computer-b"))
                .unwrap();
        });
        let other = other_done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("an active transaction must not block another Computer");
        assert!(other
            .mcp
            .servers
            .iter()
            .any(|server| server.name == "other"));
        other_reader.join().unwrap();

        assert!(matches!(
            merged_done_rx.recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(
            raw_done_rx.recv_timeout(std::time::Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        resume_commit.wait();
        commit.join().unwrap().unwrap();
        let snapshot = merged_done_rx.recv().unwrap();
        let raw = raw_done_rx.recv().unwrap().unwrap();
        merged_reader.join().unwrap();
        raw_reader.join().unwrap();

        assert!(snapshot
            .mcp
            .servers
            .iter()
            .any(|server| server.name == "after"));
        assert!(!snapshot
            .mcp
            .servers
            .iter()
            .any(|server| server.name == "before"));
        assert_eq!(raw, after);
        assert!(!anchor_restore_transaction_root(&anchor).unwrap().exists());
    }

    #[test]
    fn raw_restore_recovery_restores_previous_directory_after_interrupted_swap() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let before = project_doc_with_marker("before");
        sdk_config.save("computer-a", &before).unwrap();
        let anchor = sdk_config.project_anchor("computer-a");
        let transaction_root = raw_restore_transaction_root(&anchor).unwrap();
        fs::create_dir_all(&transaction_root).unwrap();
        write_raw_restore_transaction(
            &transaction_root,
            &RawRestoreTransaction {
                phase: RawRestorePhase::PreviousMoved,
                original_settings_dir_existed: true,
            },
        )
        .unwrap();
        fs::rename(
            anchor.join(TFROBOT_DIRNAME),
            transaction_root.join("previous-settings"),
        )
        .unwrap();

        assert_eq!(
            sdk_config.load_raw_project_config("computer-a").unwrap(),
            before
        );
        assert!(!transaction_root.exists());
    }

    #[test]
    fn raw_restore_recovery_discards_unmarked_preparation_directory() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let before = project_doc_with_marker("before");
        sdk_config.save("computer-a", &before).unwrap();
        let anchor = sdk_config.project_anchor("computer-a");
        let transaction_root = raw_restore_transaction_root(&anchor).unwrap();
        fs::create_dir_all(transaction_root.join("staged-anchor")).unwrap();
        fs::write(transaction_root.join("staged-anchor/partial"), b"partial").unwrap();

        assert_eq!(
            sdk_config.load_raw_project_config("computer-a").unwrap(),
            before
        );
        assert!(!transaction_root.exists());
    }

    #[test]
    fn lifecycle_is_scoped_to_one_computer_instance() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("computer-a").unwrap();

        let anchor = sdk_config.project_anchor("computer-a");
        assert!(anchor.join(".tfrobot/settings.json").is_file());
        assert!(anchor.join(".tfrobot/mcp.json").is_file());
        assert!(!sdk_config
            .project_anchor("computer-b")
            .join(".tfrobot/mcp.json")
            .exists());
        let snapshot = sdk_config.load("computer-a");
        assert!(!snapshot.revision.0.is_empty());
    }

    #[test]
    fn duplicate_and_delete_do_not_cross_instance_boundaries() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        sdk_config.duplicate("source", "target").unwrap();
        sdk_config.delete("source").unwrap();

        assert!(!sdk_config
            .project_anchor("source")
            .join(".tfrobot/mcp.json")
            .exists());
        assert!(sdk_config
            .project_anchor("target")
            .join(".tfrobot/mcp.json")
            .is_file());
    }

    #[test]
    fn duplicate_propagates_corrupt_source_error_without_mutating_target() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        sdk_config.init("target").unwrap();
        let source_mcp = sdk_config
            .project_anchor("source")
            .join(".tfrobot/mcp.json");
        let target_mcp = sdk_config
            .project_anchor("target")
            .join(".tfrobot/mcp.json");
        std::fs::write(&source_mcp, "{not-json").unwrap();
        let target_before = std::fs::read(&target_mcp).unwrap();

        let error = sdk_config.duplicate("source", "target").unwrap_err();

        assert!(matches!(error, ConfigCrudError::Io { ref path, .. } if path == &source_mcp));
        assert_eq!(std::fs::read(&target_mcp).unwrap(), target_before);
    }

    #[test]
    fn cli_native_export_rejects_corrupt_source_config() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        let source_mcp = sdk_config
            .project_anchor("source")
            .join(".tfrobot/mcp.json");
        std::fs::write(&source_mcp, "{not-json").unwrap();

        let error = sdk_config.export_cli_native_mcp("source").unwrap_err();

        match error {
            SdkConfigPortabilityError::InvalidSource { errors, .. } => {
                let expected_suffix = Path::new(".tfrobot").join("mcp.json");
                assert!(errors.iter().any(|error| {
                    error.field == "<file>"
                        && error
                            .source_path
                            .as_deref()
                            .is_some_and(|path| Path::new(path).ends_with(&expected_suffix))
                }));
            }
            other => panic!("expected invalid source error, got {other:?}"),
        }
    }

    #[test]
    fn cli_native_export_rejects_partial_backup_when_one_server_is_invalid() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "valid": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "node"}
                                },
                                "broken": {"type": "carrier-pigeon"}
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let error = sdk_config.export_cli_native_mcp("source").unwrap_err();

        match error {
            SdkConfigPortabilityError::InvalidSource { errors, .. } => {
                assert!(errors.iter().any(|error| error.field == "servers.broken"));
            }
            other => panic!("expected invalid source error, got {other:?}"),
        }
    }

    #[test]
    fn cli_native_export_ignores_sdk_input_diagnostics_owned_by_the_client() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "valid": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "node"}
                                }
                            },
                            "inputs": [{"id": 7}]
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let exported = sdk_config.export_cli_native_mcp("source").unwrap();

        assert!(exported
            .mcp
            .as_ref()
            .and_then(|mcp| mcp.get("servers"))
            .and_then(Value::as_object)
            .is_some_and(|servers| servers.contains_key("valid")));
    }

    #[test]
    fn portable_mcp_resolution_excludes_policy_and_recovers_shadowed_instance_server() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "shared-name": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "instance-command"}
                                }
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let policy_directory = tempdir().unwrap();
        let policy_path = policy_directory.path().join(MANAGED_MCP_FILENAME);
        std::fs::write(
            &policy_path,
            serde_json::to_vec(&json!({
                "servers": {
                    "policy-only": {
                        "type": "stdio",
                        "server_parameters": {"command": "policy-only-command"}
                    },
                    "shared-name": {
                        "type": "stdio",
                        "server_parameters": {"command": "policy-command"}
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let context = sdk_config.context("source");
        let resolved_with_policy = resolve_mcp_config(ResolveMcpConfigArgs {
            cwd: Some(context.project_anchor()),
            env: Some(context.env()),
            managed_mcp_path: Some(&policy_path),
            ..Default::default()
        });
        assert!(resolved_with_policy.servers.contains_key("policy-only"));
        assert_eq!(
            serde_json::to_value(&resolved_with_policy.servers["shared-name"].config).unwrap()
                ["server_parameters"]["command"],
            "policy-command"
        );

        let staging = tempdir().unwrap();
        let portable = resolve_portable_mcp_without_policy(&context, staging.path());

        assert!(!portable.servers.contains_key("policy-only"));
        assert_eq!(
            serde_json::to_value(&portable.servers["shared-name"].config).unwrap()
                ["server_parameters"]["command"],
            "instance-command"
        );
    }

    #[test]
    fn portable_mcp_resolution_ignores_malformed_policy_diagnostics() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "instance-server": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "instance-command"}
                                }
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let policy_directory = tempdir().unwrap();
        let policy_path = policy_directory.path().join(MANAGED_MCP_FILENAME);
        std::fs::write(&policy_path, "{not-json").unwrap();
        let context = sdk_config.context("source");
        let resolved_with_policy = resolve_mcp_config(ResolveMcpConfigArgs {
            cwd: Some(context.project_anchor()),
            env: Some(context.env()),
            managed_mcp_path: Some(&policy_path),
            ..Default::default()
        });
        assert!(resolved_with_policy
            .errors
            .iter()
            .any(|error| error.source_path.as_deref() == policy_path.to_str()));

        let staging = tempdir().unwrap();
        let portable = resolve_portable_mcp_without_policy(&context, staging.path());

        assert!(portable.errors.is_empty());
        assert!(portable.servers.contains_key("instance-server"));
    }

    #[test]
    fn crud_and_portability_round_trip_through_the_adapter() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        let before = sdk_config.load("source").revision;
        let after = sdk_config
            .update(
                "source",
                &[ConfigEdit::new(
                    ConfigEntity::McpServer("audit".to_string()),
                    EditIntent::Upsert(json!({
                        "type": "stdio",
                        "server_parameters": {"command": "node"}
                    })),
                )],
            )
            .unwrap();
        assert_ne!(before, after.revision);
        assert_eq!(after.mcp.servers[0].name, "audit");

        let exported = sdk_config.export("source").unwrap();
        assert!(sdk_config.validate(&exported).is_valid());

        sdk_config.save("saved", &exported).unwrap();
        assert_eq!(sdk_config.load("saved").mcp.servers[0].name, "audit");

        let report = sdk_config.import("imported", &exported).unwrap();
        assert!(report.is_valid());
        assert_eq!(sdk_config.load("imported").mcp.servers[0].name, "audit");
    }

    #[test]
    fn portability_guard_rejects_sensitive_argument_and_query_plaintext() {
        let document = ProjectConfigDoc {
            mcp: Some(
                json!({
                    "servers": {
                        "stdio-secret": {
                            "type": "stdio",
                            "server_parameters": {
                                "command": "node",
                                "args": [
                                    "server.js",
                                    "--api-key",
                                    "sk-live",
                                    "--auth",
                                    "-still-plaintext"
                                ]
                            }
                        },
                        "http-secret": {
                            "type": "streamable",
                            "server_parameters": {
                                "url": "https://example.com/mcp?access_token=plain-token"
                            }
                        },
                        "relative-http-secret": {
                            "type": "streamable",
                            "server_parameters": {
                                "url": "mcp?client_secret=relative-plain-secret"
                            }
                        },
                        "ordinary-query-literal": {
                            "type": "streamable",
                            "server_parameters": {
                                "url": "mcp?mode=debug"
                            }
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..Default::default()
        };

        let error = ensure_portable_secret_references(&document).unwrap_err();

        match error {
            SdkConfigPortabilityError::UnsafePlaintext { fields, .. } => {
                assert!(fields
                    .iter()
                    .any(|field| field.contains("args[2]") && field.contains("--api-key")));
                assert!(fields
                    .iter()
                    .any(|field| field.contains("args[4]") && field.contains("--auth")));
                assert!(fields.iter().any(|field| {
                    field.contains("server_parameters.url") && field.contains("access_token")
                }));
                assert!(fields.iter().any(|field| {
                    field.contains("relative-http-secret") && field.contains("client_secret")
                }));
                assert!(fields.iter().any(|field| {
                    field.contains("ordinary-query-literal") && field.contains("mode")
                }));
            }
            other => panic!("expected unsafe plaintext error, got {other:?}"),
        }
    }

    #[test]
    fn prepare_import_rejects_sensitive_plaintext_before_sdk_staging_validation() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        let document = ProjectConfigDoc {
            mcp: Some(
                json!({
                    "servers": {
                        "invalid-and-sensitive": {
                            "type": "unsupported-transport",
                            "server_parameters": {
                                "args": ["--token", "plain-before-staging"]
                            }
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..Default::default()
        };

        assert!(matches!(
            sdk_config.prepare_import(&document),
            Err(SdkConfigPortabilityError::UnsafePlaintext { .. })
        ));
    }

    #[test]
    fn portability_guard_allows_references_and_non_sensitive_literals() {
        let document = ProjectConfigDoc {
            mcp: Some(
                json!({
                    "servers": {
                        "portable": {
                            "type": "stdio",
                            "server_parameters": {
                                "command": "node",
                                "args": [
                                    "server.js",
                                    "--port=3000",
                                    "--token",
                                    "${input:api-token}",
                                    "--client-secret=${env:CLIENT_SECRET}"
                                ]
                            }
                        },
                        "http-portable": {
                            "type": "streamable",
                            "server_parameters": {
                                "url": "https://example.com/mcp?token=%24%7Binput%3Aapi-token%7D&mode=%24%7Binput%3Amode%7D"
                            }
                        },
                        "relative-http-portable": {
                            "type": "streamable",
                            "server_parameters": {
                                "url": "mcp?client_secret=%24%7Benv%3ACLIENT_SECRET%7D"
                            }
                        },
                        "fragment-is-not-query": {
                            "type": "streamable",
                            "server_parameters": {
                                "url": "mcp#fragment?client_secret=not-a-query-value"
                            }
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..Default::default()
        };

        ensure_portable_secret_references(&document).unwrap();
    }

    #[test]
    fn validate_migrate_and_path_ownership_are_instance_scoped() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        let invalid = ProjectConfigDoc {
            mcp: Some(
                json!({"servers": {"broken": {"type": "carrier-pigeon"}}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            ..Default::default()
        };
        assert!(!sdk_config.validate(&invalid).is_valid());

        sdk_config.init("computer-a").unwrap();
        assert!(!sdk_config.migrate("computer-a").unwrap());
        assert!(sdk_config.owns_path(
            "computer-a",
            &sdk_config
                .project_anchor("computer-a")
                .join(".tfrobot/mcp.json")
        ));
        assert!(!sdk_config.owns_path(
            "computer-a",
            &sdk_config
                .project_anchor("computer-b")
                .join(".tfrobot/mcp.json")
        ));
    }
}
