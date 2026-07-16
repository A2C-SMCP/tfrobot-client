use crate::services::config::ConfigService;
use crate::services::storage::write_json_atomically;
use a2c_smcp::smcp_computer::settings::config::{
    delete_config, duplicate_config, export_config, import_config, init_config, load_config,
    load_project_config_doc, migrate_config, save_config, update_config, validate_config,
    ComputerConfigSnapshot, ConfigContext, ConfigCrudError, ConfigEdit, ProjectConfigDoc,
    ValidationReport,
};
use a2c_smcp::smcp_computer::settings::{
    resolve_mcp_config, EnvMap, ResolveMcpConfigArgs, ResolvedMcpConfig, SettingsValidationError,
    MANAGED_MCP_FILENAME, TFROBOT_DIRNAME, XDG_CONFIG_HOME_ENV,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

#[derive(Debug, thiserror::Error)]
pub enum SdkConfigExportError {
    #[error("Cannot export invalid SDK MCP configuration: {details}")]
    InvalidSource {
        errors: Vec<SettingsValidationError>,
        details: String,
    },
    #[error(transparent)]
    Crud(#[from] ConfigCrudError),
}

impl SdkConfigExportError {
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
}

impl InstanceConfigContext {
    pub(crate) fn new(project_anchor: PathBuf, skill_home: PathBuf) -> Self {
        let mut env = EnvMap::new();
        env.insert(
            XDG_CONFIG_HOME_ENV.to_string(),
            project_anchor.to_string_lossy().into_owned(),
        );
        Self {
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
        load_config(&self.sdk_context())
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
        }
    }

    pub fn project_anchor(&self, instance_id: &str) -> PathBuf {
        self.config
            .computer_instance_storage_root(instance_id)
            .join("sdk_config")
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
        init_config(&self.project_anchor(instance_id))
    }

    pub fn load(&self, instance_id: &str) -> ComputerConfigSnapshot {
        self.context(instance_id).load()
    }

    pub fn save(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<(), ConfigCrudError> {
        save_config(&self.project_anchor(instance_id), document)
    }

    pub fn update(
        &self,
        instance_id: &str,
        edits: &[ConfigEdit],
    ) -> Result<ComputerConfigSnapshot, ConfigCrudError> {
        let context = self.context(instance_id);
        update_config(&context.sdk_context(), edits)
    }

    pub fn validate(&self, document: &ProjectConfigDoc) -> ValidationReport {
        validate_config(document)
    }

    pub fn migrate(&self, instance_id: &str) -> Result<bool, ConfigCrudError> {
        migrate_config(&self.project_anchor(instance_id))
    }

    pub fn delete(&self, instance_id: &str) -> Result<(), ConfigCrudError> {
        delete_config(&self.project_anchor(instance_id))
    }

    pub fn duplicate(&self, source_id: &str, target_id: &str) -> Result<(), ConfigCrudError> {
        duplicate_config(
            &self.project_anchor(source_id),
            &self.project_anchor(target_id),
        )
    }

    pub fn export(&self, instance_id: &str) -> Result<ProjectConfigDoc, ConfigCrudError> {
        export_config(&self.project_anchor(instance_id))
    }

    /// Loads the SDK-owned project anchor without crossing the sanitized export boundary.
    /// This is reserved for same-machine lifecycle transactions such as legacy migration.
    pub(crate) fn load_raw_project_config(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, ConfigCrudError> {
        let anchor = self.project_anchor(instance_id);
        recover_raw_restore_transaction(&anchor)?;
        load_project_config_doc(&anchor)
    }

    /// Replaces all four SDK project-anchor files from a raw same-machine snapshot.
    pub(crate) fn restore_raw_project_config(
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

    /// Export every reconciled instance-owned MCP declaration through the SDK sanitizer.
    ///
    /// SDK shareable export intentionally omits local scopes. The client's CLI-native export is
    /// a full backup, so it resolves User/Project/Local declarations without ambient Policy,
    /// stages them in an isolated project document, and delegates redaction to the SDK.
    pub fn export_cli_native_mcp(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, SdkConfigExportError> {
        let context = self.context(instance_id);
        let source_anchor = context.project_anchor.clone();
        let staging = tempfile::tempdir().map_err(|error| ConfigCrudError::Io {
            path: source_anchor.clone(),
            reason: format!("failed to create CLI-native export staging directory: {error}"),
        })?;
        let mut resolved = resolve_portable_mcp_without_policy(&context, staging.path());
        let export_errors = std::mem::take(&mut resolved.errors)
            .into_iter()
            .filter(|error| error.field != "inputs" && !error.field.starts_with("inputs."))
            .collect::<Vec<_>>();
        if !export_errors.is_empty() {
            return Err(SdkConfigExportError::invalid_source(export_errors));
        }

        let mut servers = Map::new();
        for (name, server) in resolved.servers {
            let value =
                serde_json::to_value(server.config).map_err(|error| ConfigCrudError::Io {
                    path: source_anchor.clone(),
                    reason: format!(
                        "failed to serialize MCP server '{}' for CLI-native export: {error}",
                        name
                    ),
                })?;
            let mut body = value
                .as_object()
                .cloned()
                .ok_or_else(|| ConfigCrudError::Io {
                    path: source_anchor.clone(),
                    reason: format!("serialized MCP server '{}' is not a JSON object", name),
                })?;
            body.remove("name");
            servers.insert(name, Value::Object(body));
        }

        let mut mcp = Map::new();
        mcp.insert("servers".to_string(), Value::Object(servers));
        save_config(
            staging.path(),
            &ProjectConfigDoc {
                mcp: Some(mcp),
                ..Default::default()
            },
        )?;
        Ok(export_config(staging.path())?)
    }

    pub fn import(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<ValidationReport, ConfigCrudError> {
        import_config(&self.project_anchor(instance_id), document)
    }

    pub fn owns_path(&self, instance_id: &str, path: &Path) -> bool {
        path.starts_with(self.project_anchor(instance_id))
            || path.starts_with(self.skill_home(instance_id))
    }
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
            SdkConfigExportError::InvalidSource { errors, .. } => {
                assert!(errors.iter().any(|error| {
                    error.field == "<file>" && error.source_path.as_deref() == source_mcp.to_str()
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
            SdkConfigExportError::InvalidSource { errors, .. } => {
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
