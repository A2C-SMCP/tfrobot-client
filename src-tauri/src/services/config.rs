use crate::commands::inputs::InputDefinition;
use crate::services::client_computers::{
    ClientComputersPathError, ClientComputersPaths, GlobalConfigFile, COMPUTER_INPUTS_FILE_NAME,
    COMPUTER_PROFILE_FILE_NAME,
};
use crate::services::computer::{
    ComputerInputDefinition, ComputerInputsConfig, ComputerInstance, ComputerInstancesConfig,
    ComputerProfile, ManagedMcpServer, SdkContextConfig, COMPUTER_INPUTS_SCHEMA_VERSION,
    COMPUTER_PROFILE_SCHEMA_VERSION, SDK_CONTEXT_SCHEMA_VERSION,
};
use crate::services::connection_targets::{
    ConnectionTargetsConfig, GlobalManualSmcpTarget, GlobalManualTargetsConfig, ManualSmcpTarget,
    MANUAL_TARGETS_SCHEMA_VERSION,
};
use crate::services::storage::{write_json_atomically, AtomicJsonWriteError};
#[cfg(test)]
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Service for persisting all configuration data to disk
pub struct ConfigService {
    config_dir: PathBuf,
    computer_instances_file: PathBuf,
    connection_targets_file: PathBuf,
    client_computers_paths: ClientComputersPaths,
    directory_transaction_lock: Mutex<()>,
    #[cfg(test)]
    directory_rename_actions: Mutex<VecDeque<DirectoryRenameTestAction>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum DirectoryRenameTestAction {
    Proceed,
    Fail,
    FailAfterCreatingDestinationFile,
}

#[derive(Debug)]
pub struct ComputerProfileDiscoveryError {
    pub path: PathBuf,
    pub error: ConfigError,
}

#[derive(Debug, Default)]
pub struct ComputerProfileDiscovery {
    pub config: ComputerInstancesConfig,
    pub errors: Vec<ComputerProfileDiscoveryError>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComputerDirectoryTransaction {
    schema_version: u32,
    instance_id: String,
    phase: ComputerDirectoryTransactionPhase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ComputerDirectoryTransactionPhase {
    Preparing,
    Ready,
}

impl ConfigService {
    /// Create a new ConfigService with the given app data directory
    pub fn new(app_data_dir: PathBuf) -> Result<Self, std::io::Error> {
        let client_computers_paths = ClientComputersPaths::from_app_data_dir(&app_data_dir);
        Self::new_with_client_computers_paths(app_data_dir, client_computers_paths)
    }

    pub fn new_with_client_computers_paths(
        app_data_dir: PathBuf,
        client_computers_paths: ClientComputersPaths,
    ) -> Result<Self, std::io::Error> {
        fs::create_dir_all(&app_data_dir)?;

        Ok(Self {
            computer_instances_file: app_data_dir.join("computer_instances.json"),
            connection_targets_file: app_data_dir.join("connection_targets.json"),
            client_computers_paths,
            directory_transaction_lock: Mutex::new(()),
            #[cfg(test)]
            directory_rename_actions: Mutex::new(VecDeque::new()),
            config_dir: app_data_dir,
        })
    }

    pub fn client_computers_root(&self) -> &Path {
        self.client_computers_paths.root()
    }

    pub fn computer_profile_path(&self, instance_id: &str) -> Result<PathBuf, ConfigError> {
        self.client_computers_paths
            .computer_profile(instance_id)
            .map_err(|error| match error {
                ClientComputersPathError::InvalidInstanceId(id) => {
                    ConfigError::InvalidComputerProfileId(id)
                }
            })
    }

    pub fn computer_instance_root(&self, instance_id: &str) -> Result<PathBuf, ConfigError> {
        self.client_computers_paths
            .instance_root(instance_id)
            .map_err(map_client_computers_path_error)
    }

    pub fn sdk_context_path(&self, instance_id: &str) -> Result<PathBuf, ConfigError> {
        self.client_computers_paths
            .sdk_context(instance_id)
            .map_err(map_client_computers_path_error)
    }

    pub fn computer_inputs_path(&self, instance_id: &str) -> Result<PathBuf, ConfigError> {
        self.client_computers_paths
            .computer_inputs(instance_id)
            .map_err(map_client_computers_path_error)
    }

    pub fn migration_state_path(&self) -> PathBuf {
        self.client_computers_paths.migration_state()
    }

    pub fn legacy_computer_instances_path(&self) -> &Path {
        &self.computer_instances_file
    }

    pub fn legacy_connection_targets_path(&self) -> &Path {
        &self.connection_targets_file
    }

    pub fn save_computer_profile(&self, profile: &ComputerProfile) -> Result<(), ConfigError> {
        validate_schema_version(
            "computer profile",
            profile.schema_version,
            COMPUTER_PROFILE_SCHEMA_VERSION,
        )?;
        let path = self.computer_profile_path(&profile.id)?;
        save_json_file(&path, profile)
    }

    pub fn load_computer_profile(
        &self,
        instance_directory_id: &str,
    ) -> Result<ComputerProfile, ConfigError> {
        let path = self.computer_profile_path(instance_directory_id)?;
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_string_lossy().into_owned()));
        }
        let profile: ComputerProfile = load_required_json_file(&path)?;
        validate_schema_version(
            "computer profile",
            profile.schema_version,
            COMPUTER_PROFILE_SCHEMA_VERSION,
        )?;
        if profile.id != instance_directory_id {
            return Err(ConfigError::CorruptedComputerProfile {
                directory_id: instance_directory_id.to_string(),
                profile_id: profile.id,
            });
        }
        Ok(profile)
    }

    pub fn load_sdk_context(&self, instance_id: &str) -> Result<SdkContextConfig, ConfigError> {
        let path = self.sdk_context_path(instance_id)?;
        let config: SdkContextConfig = load_new_artifact_or_default(&path)?;
        validate_schema_version(
            "SDK context",
            config.schema_version,
            SDK_CONTEXT_SCHEMA_VERSION,
        )?;
        Ok(config)
    }

    pub fn save_sdk_context(
        &self,
        instance_id: &str,
        context: &SdkContextConfig,
    ) -> Result<(), ConfigError> {
        validate_schema_version(
            "SDK context",
            context.schema_version,
            SDK_CONTEXT_SCHEMA_VERSION,
        )?;
        save_json_file(&self.sdk_context_path(instance_id)?, context)
    }

    pub fn save_computer_directory(
        &self,
        profile: &ComputerProfile,
        context: &SdkContextConfig,
        inputs: &[InputDefinition],
    ) -> Result<(), ConfigError> {
        let _guard = self.lock_computer_directories()?;
        self.recover_computer_directory_transactions_unlocked()?;
        self.save_computer_directory_transaction_unlocked(profile, context, inputs, false)
    }

    pub fn load_computer_inputs(
        &self,
        instance_id: &str,
    ) -> Result<ComputerInputsConfig, ConfigError> {
        let path = self.computer_inputs_path(instance_id)?;
        let mut config: ComputerInputsConfig = load_new_artifact_or_default(&path)?;
        sanitize_computer_inputs_config(&mut config);
        validate_computer_inputs_config(&config)?;
        Ok(config)
    }

    pub fn save_computer_inputs(
        &self,
        instance_id: &str,
        config: &ComputerInputsConfig,
    ) -> Result<(), ConfigError> {
        let mut config = config.clone();
        sanitize_computer_inputs_config(&mut config);
        validate_computer_inputs_config(&config)?;
        save_json_file(&self.computer_inputs_path(instance_id)?, &config)
    }

    pub fn load_global_manual_targets(&self) -> Result<GlobalManualTargetsConfig, ConfigError> {
        let path = self.global_config_path(GlobalConfigFile::ManualTargets);
        let config: GlobalManualTargetsConfig = load_new_artifact_or_default(&path)?;
        validate_global_manual_targets_config(&config)?;
        Ok(config)
    }

    pub fn global_manual_targets_path(&self) -> PathBuf {
        self.global_config_path(GlobalConfigFile::ManualTargets)
    }

    pub fn save_global_manual_targets(
        &self,
        config: &GlobalManualTargetsConfig,
    ) -> Result<(), ConfigError> {
        validate_global_manual_targets_config(config)?;
        save_json_file(
            &self.global_config_path(GlobalConfigFile::ManualTargets),
            config,
        )
    }

    fn global_config_path(&self, artifact: GlobalConfigFile) -> PathBuf {
        self.client_computers_paths.global_config(artifact)
    }

    pub fn default_local_skills_root(&self, instance_id: &str) -> PathBuf {
        self.computer_instance_storage_root(instance_id)
            .join("skill_home")
    }

    pub fn computer_instance_storage_root(&self, instance_id: &str) -> PathBuf {
        self.computer_skill_home_base()
            .join(instance_storage_dir_name(instance_id))
    }

    pub fn computer_skill_home_base(&self) -> PathBuf {
        self.config_dir.join("computer_instances")
    }

    /// Read legacy inline MCP declarations only for one-time profile migration.
    ///
    /// SDK-owned MCP configuration must otherwise cross `SdkConfigService`. Keeping this
    /// deliberately named, read-only entry point prevents the legacy profile aggregate from
    /// becoming a second persistence boundary while older `computer_instances.json` files remain
    /// deserializable.
    pub fn load_legacy_mcp_configs_for_migration(
        &self,
        instance_id: &str,
    ) -> Result<Vec<ManagedMcpServer>, ConfigError> {
        let instances = self.load_legacy_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.mcp_servers.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    // --- Input Definitions ---

    pub fn load_inputs_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<Vec<InputDefinition>, ConfigError> {
        self.load_computer_profile(instance_id)?;
        Ok(self
            .load_computer_inputs(instance_id)?
            .inputs
            .iter()
            .map(InputDefinition::from)
            .collect())
    }

    pub fn save_inputs_for_instance(
        &self,
        instance_id: &str,
        inputs: &[InputDefinition],
    ) -> Result<ComputerInstance, ConfigError> {
        self.load_computer_profile(instance_id)?;
        self.save_computer_inputs(
            instance_id,
            &ComputerInputsConfig {
                schema_version: COMPUTER_INPUTS_SCHEMA_VERSION,
                inputs: inputs.iter().map(ComputerInputDefinition::from).collect(),
            },
        )?;
        self.get_computer_instance(instance_id)
    }

    // --- Computer Instances ---

    pub fn load_computer_instances(&self) -> Result<ComputerInstancesConfig, ConfigError> {
        let discovery = self.discover_computer_instances()?;
        for error in &discovery.errors {
            log::warn!(
                "Ignoring invalid Computer profile {}: {}",
                error.path.display(),
                error.error
            );
        }
        Ok(discovery.config)
    }

    pub fn discover_computer_instances(&self) -> Result<ComputerProfileDiscovery, ConfigError> {
        let _guard = self.lock_computer_directories()?;
        self.recover_computer_directory_transactions_unlocked()?;
        let root = self.client_computers_paths.instances_root();
        if !root.exists() {
            return Ok(ComputerProfileDiscovery::default());
        }

        let mut instances = Vec::new();
        let mut errors = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if !file_type.is_dir() {
                errors.push(ComputerProfileDiscoveryError {
                    path,
                    error: ConfigError::InvalidArtifact {
                        path: entry.path(),
                        reason: format!(
                            "expected an instance directory containing {COMPUTER_PROFILE_FILE_NAME}"
                        ),
                    },
                });
                continue;
            }
            let Some(directory_id) = entry.file_name().to_str().map(str::to_string) else {
                errors.push(ComputerProfileDiscoveryError {
                    path: path.clone(),
                    error: ConfigError::InvalidArtifact {
                        path,
                        reason: "instance directory name is not valid UTF-8".to_string(),
                    },
                });
                continue;
            };
            match self.load_computer_profile(&directory_id) {
                Ok(profile) => {
                    let mut instance = ComputerInstance::from(profile);
                    match self.load_computer_inputs(&directory_id) {
                        Ok(inputs) => {
                            instance.inputs =
                                inputs.inputs.iter().map(InputDefinition::from).collect()
                        }
                        Err(error) => {
                            errors.push(ComputerProfileDiscoveryError {
                                path: self.computer_inputs_path(&directory_id)?,
                                error,
                            });
                            continue;
                        }
                    }
                    match self.load_sdk_context(&directory_id) {
                        Ok(context) => instance.local_skills_root = context.skill_home_override,
                        Err(error) => {
                            errors.push(ComputerProfileDiscoveryError {
                                path: self.sdk_context_path(&directory_id)?,
                                error,
                            });
                            continue;
                        }
                    }
                    instances.push(instance);
                }
                Err(error) => errors.push(ComputerProfileDiscoveryError {
                    path: path.join(COMPUTER_PROFILE_FILE_NAME),
                    error,
                }),
            }
        }
        instances.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(ComputerProfileDiscovery {
            config: ComputerInstancesConfig {
                schema_version: 1,
                instances,
            },
            errors,
        })
    }

    pub fn load_legacy_computer_instances(&self) -> Result<ComputerInstancesConfig, ConfigError> {
        load_json_file(&self.computer_instances_file)
    }

    pub fn save_computer_instances(
        &self,
        instances: &ComputerInstancesConfig,
    ) -> Result<(), ConfigError> {
        for instance in &instances.instances {
            let profile = ComputerProfile::from(instance);
            let context = SdkContextConfig {
                schema_version: SDK_CONTEXT_SCHEMA_VERSION,
                skill_home_override: instance.local_skills_root.clone(),
            };
            self.save_computer_directory(&profile, &context, &instance.inputs)?;
        }
        Ok(())
    }

    pub fn get_computer_instance(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let _guard = self.lock_computer_directories()?;
        self.recover_computer_directory_transactions_unlocked()?;
        self.load_computer_instance_unlocked(id)
    }

    fn load_computer_instance_unlocked(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let mut instance = ComputerInstance::from(self.load_computer_profile(id)?);
        instance.inputs = self
            .load_computer_inputs(id)?
            .inputs
            .iter()
            .map(InputDefinition::from)
            .collect();
        instance.local_skills_root = self.load_sdk_context(id)?.skill_home_override;
        Ok(instance)
    }

    pub fn add_computer_instance(&self, instance: ComputerInstance) -> Result<(), ConfigError> {
        let _guard = self.lock_computer_directories()?;
        self.recover_computer_directory_transactions_unlocked()?;
        let path = self.computer_profile_path(&instance.id)?;
        if path.exists() {
            return Err(ConfigError::AlreadyExists(instance.id));
        }
        self.save_computer_directory_transaction_unlocked(
            &ComputerProfile::from(&instance),
            &SdkContextConfig {
                schema_version: SDK_CONTEXT_SCHEMA_VERSION,
                skill_home_override: instance.local_skills_root.clone(),
            },
            &instance.inputs,
            true,
        )
    }

    pub fn rename_computer_instance(
        &self,
        id: &str,
        name: String,
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(id, |instance| {
            instance.name = name;
        })
    }

    pub fn update_computer_instance<F>(
        &self,
        id: &str,
        update: F,
    ) -> Result<ComputerInstance, ConfigError>
    where
        F: FnOnce(&mut ComputerInstance),
    {
        let _guard = self.lock_computer_directories()?;
        self.recover_computer_directory_transactions_unlocked()?;
        let mut instance = self.load_computer_instance_unlocked(id)?;
        update(&mut instance);
        self.save_computer_directory_transaction_unlocked(
            &ComputerProfile::from(&instance),
            &SdkContextConfig {
                schema_version: SDK_CONTEXT_SCHEMA_VERSION,
                skill_home_override: instance.local_skills_root.clone(),
            },
            &instance.inputs,
            false,
        )?;
        Ok(instance)
    }

    fn save_computer_directory_transaction_unlocked(
        &self,
        profile: &ComputerProfile,
        context: &SdkContextConfig,
        inputs: &[InputDefinition],
        require_absent: bool,
    ) -> Result<(), ConfigError> {
        validate_schema_version(
            "computer profile",
            profile.schema_version,
            COMPUTER_PROFILE_SCHEMA_VERSION,
        )?;
        validate_schema_version(
            "SDK context",
            context.schema_version,
            SDK_CONTEXT_SCHEMA_VERSION,
        )?;
        let mut inputs_config = ComputerInputsConfig {
            schema_version: COMPUTER_INPUTS_SCHEMA_VERSION,
            inputs: inputs.iter().map(ComputerInputDefinition::from).collect(),
        };
        sanitize_computer_inputs_config(&mut inputs_config);
        validate_computer_inputs_config(&inputs_config)?;
        let instance_root = self.computer_instance_root(&profile.id)?;
        if require_absent && instance_root.exists() {
            return Err(ConfigError::AlreadyExists(profile.id.clone()));
        }
        fs::create_dir_all(self.client_computers_paths.instances_root())?;

        let transactions_root = self.client_computers_paths.root().join(".transactions");
        fs::create_dir_all(&transactions_root)?;
        let transaction_root = transactions_root.join(uuid::Uuid::new_v4().to_string());
        let staged_root = transaction_root.join("new");
        let previous_root = transaction_root.join("old");
        let preparation = (|| {
            fs::create_dir_all(&staged_root)?;
            let mut transaction = ComputerDirectoryTransaction {
                schema_version: 1,
                instance_id: profile.id.clone(),
                phase: ComputerDirectoryTransactionPhase::Preparing,
            };
            save_json_file(&transaction_root.join("transaction.json"), &transaction)?;
            save_json_file(&staged_root.join(COMPUTER_PROFILE_FILE_NAME), profile)?;
            save_json_file(
                &staged_root.join(crate::services::client_computers::SDK_CONTEXT_FILE_NAME),
                context,
            )?;
            save_json_file(&staged_root.join(COMPUTER_INPUTS_FILE_NAME), &inputs_config)?;
            let (staged_profile, staged_context, staged_inputs) =
                validate_staged_computer_directory(&staged_root, &profile.id)?;
            if staged_profile != *profile
                || staged_context != *context
                || staged_inputs != inputs_config
            {
                return Err(ConfigError::InvalidArtifact {
                    path: staged_root.clone(),
                    reason: "staged Computer directory does not match the requested profile, SDK context, and Inputs"
                        .to_string(),
                });
            }
            transaction.phase = ComputerDirectoryTransactionPhase::Ready;
            save_json_file(&transaction_root.join("transaction.json"), &transaction)?;
            Ok::<(), ConfigError>(())
        })();
        if let Err(error) = preparation {
            let _ = fs::remove_dir_all(&transaction_root);
            let _ = fs::remove_dir(&transactions_root);
            return Err(error);
        }

        if instance_root.exists() {
            self.rename_computer_directory(&instance_root, &previous_root)?;
        }
        if let Err(error) = self.rename_computer_directory(&staged_root, &instance_root) {
            if previous_root.exists() {
                if let Err(rollback_error) =
                    self.rename_computer_directory(&previous_root, &instance_root)
                {
                    return Err(ConfigError::DirectoryTransactionRollback {
                        instance_id: profile.id.clone(),
                        primary: error.to_string(),
                        rollback: rollback_error.to_string(),
                        transaction_root: transaction_root.display().to_string(),
                    });
                }
            }
            let _ = fs::remove_dir_all(&transaction_root);
            return Err(ConfigError::Io(error));
        }
        if previous_root.exists() {
            if let Err(error) = fs::remove_dir_all(&previous_root) {
                log::warn!(
                    "Computer '{}' directory update committed, but previous directory cleanup failed: {}",
                    profile.id,
                    error
                );
                return Ok(());
            }
        }
        if let Err(error) = fs::remove_dir_all(&transaction_root) {
            log::warn!(
                "Computer '{}' directory update committed, but transaction cleanup failed: {}",
                profile.id,
                error
            );
            return Ok(());
        }
        let _ = fs::remove_dir(&transactions_root);
        Ok(())
    }

    fn rename_computer_directory(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        #[cfg(test)]
        match self
            .directory_rename_actions
            .lock()
            .expect("directory rename action lock poisoned")
            .pop_front()
            .unwrap_or(DirectoryRenameTestAction::Proceed)
        {
            DirectoryRenameTestAction::Proceed => {}
            DirectoryRenameTestAction::Fail => {
                return Err(std::io::Error::other("injected directory rename failure"));
            }
            DirectoryRenameTestAction::FailAfterCreatingDestinationFile => {
                fs::write(to, b"injected directory rename blocker")?;
                return Err(std::io::Error::other(
                    "injected directory rename failure after creating destination file",
                ));
            }
        }

        fs::rename(from, to)
    }

    #[cfg(test)]
    pub(crate) fn inject_directory_rename_actions(
        &self,
        actions: impl IntoIterator<Item = DirectoryRenameTestAction>,
    ) {
        self.directory_rename_actions
            .lock()
            .expect("directory rename action lock poisoned")
            .extend(actions);
    }

    fn recover_computer_directory_transactions_unlocked(&self) -> Result<(), ConfigError> {
        let transactions_root = self.client_computers_paths.root().join(".transactions");
        if !transactions_root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&transactions_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                return Err(ConfigError::InvalidArtifact {
                    path: entry.path(),
                    reason: "expected a Computer directory transaction".to_string(),
                });
            }
            let transaction_root = entry.path();
            let marker_path = transaction_root.join("transaction.json");
            let staged_root = transaction_root.join("new");
            let previous_root = transaction_root.join("old");
            if !marker_path.exists() {
                if previous_root.exists() {
                    return Err(ConfigError::InvalidArtifact {
                        path: transaction_root,
                        reason:
                            "markerless Computer directory transaction retains a previous directory"
                                .to_string(),
                    });
                }
                fs::remove_dir_all(entry.path())?;
                continue;
            }
            let transaction: ComputerDirectoryTransaction = load_required_json_file(&marker_path)?;
            if transaction.schema_version != 1 {
                return Err(ConfigError::UnsupportedSchemaVersion {
                    artifact: "Computer directory transaction",
                    expected: 1,
                    actual: transaction.schema_version,
                });
            }
            if transaction.phase == ComputerDirectoryTransactionPhase::Preparing {
                if previous_root.exists() {
                    return Err(ConfigError::InvalidArtifact {
                        path: transaction_root,
                        reason: format!(
                            "preparing Computer '{}' transaction unexpectedly retains a previous directory",
                            transaction.instance_id
                        ),
                    });
                }
                fs::remove_dir_all(entry.path())?;
                continue;
            }
            let instance_root = self.computer_instance_root(&transaction.instance_id)?;
            if instance_root.exists() && previous_root.exists() && staged_root.exists() {
                return Err(ConfigError::InvalidArtifact {
                    path: transaction_root,
                    reason: format!(
                        "Computer '{}' destination is occupied while both previous and staged directories are retained",
                        transaction.instance_id
                    ),
                });
            }
            if !instance_root.exists() {
                if previous_root.exists() {
                    fs::rename(&previous_root, &instance_root)?;
                } else if staged_root.exists() {
                    validate_staged_computer_directory(&staged_root, &transaction.instance_id)?;
                    fs::rename(&staged_root, &instance_root)?;
                } else {
                    return Err(ConfigError::InvalidArtifact {
                        path: transaction_root,
                        reason: "transaction has no current, previous, or staged directory"
                            .to_string(),
                    });
                }
            }
            fs::remove_dir_all(entry.path())?;
        }
        let _ = fs::remove_dir(&transactions_root);
        Ok(())
    }

    pub(crate) fn discard_computer_directory_transactions(&self) -> Result<(), ConfigError> {
        let _guard = self.lock_computer_directories()?;
        let transactions_root = self.client_computers_paths.root().join(".transactions");
        if transactions_root.exists() {
            fs::remove_dir_all(transactions_root)?;
        }
        Ok(())
    }

    pub fn remove_computer_instance(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let _guard = self.lock_computer_directories()?;
        self.recover_computer_directory_transactions_unlocked()?;
        let removed = self.load_computer_instance_unlocked(id)?;
        let instance_root = self.computer_instance_root(id)?;
        let trash_root = self.client_computers_paths.root().join(".trash");
        fs::create_dir_all(&trash_root)?;
        let quarantined = trash_root.join(format!("{id}-{}", uuid::Uuid::new_v4().as_hyphenated()));
        fs::rename(&instance_root, &quarantined)?;
        if let Err(error) = fs::remove_dir_all(&quarantined) {
            log::warn!(
                "Computer profile '{}' was removed, but its quarantined directory {} could not be cleaned up: {}",
                id,
                quarantined.display(),
                error
            );
        } else {
            let _ = fs::remove_dir(&trash_root);
        }
        Ok(removed)
    }

    fn lock_computer_directories(&self) -> Result<std::sync::MutexGuard<'_, ()>, ConfigError> {
        self.directory_transaction_lock
            .lock()
            .map_err(|error| ConfigError::Lock(error.to_string()))
    }

    // --- Connection Targets ---

    pub fn load_connection_targets(&self) -> Result<ConnectionTargetsConfig, ConfigError> {
        let config = self.load_global_manual_targets()?;
        Ok(ConnectionTargetsConfig {
            schema_version: config.schema_version,
            manual_smcp_targets: config
                .manual_smcp_targets
                .into_iter()
                .map(|target| ManualSmcpTarget {
                    id: target.id,
                    name: target.name,
                    url: target.url,
                    namespace: target.namespace,
                    office_id: target.office_id,
                    headers: target.routing_headers,
                })
                .collect(),
        })
    }

    pub fn save_connection_targets(
        &self,
        targets: &ConnectionTargetsConfig,
    ) -> Result<(), ConfigError> {
        self.save_global_manual_targets(&GlobalManualTargetsConfig {
            schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
            manual_smcp_targets: targets
                .manual_smcp_targets
                .iter()
                .map(GlobalManualSmcpTarget::from)
                .collect(),
        })
    }

    pub fn load_legacy_connection_targets(&self) -> Result<ConnectionTargetsConfig, ConfigError> {
        load_json_file(&self.connection_targets_file)
    }

    pub fn list_manual_smcp_targets(&self) -> Result<Vec<ManualSmcpTarget>, ConfigError> {
        Ok(self.load_connection_targets()?.manual_smcp_targets)
    }

    pub fn save_manual_smcp_target(
        &self,
        mut target: ManualSmcpTarget,
    ) -> Result<ManualSmcpTarget, ConfigError> {
        target.id = stable_or_existing_manual_target_id(&target.id, &target);
        let mut targets = self.load_connection_targets()?;
        targets
            .manual_smcp_targets
            .retain(|existing| existing.id != target.id);
        targets.manual_smcp_targets.push(target.clone());
        self.save_connection_targets(&targets)?;
        Ok(target)
    }

    pub fn get_manual_smcp_target(&self, id: &str) -> Result<ManualSmcpTarget, ConfigError> {
        self.load_connection_targets()?
            .manual_smcp_targets
            .into_iter()
            .find(|target| target.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))
    }

    pub fn delete_manual_smcp_target(&self, id: &str) -> Result<ManualSmcpTarget, ConfigError> {
        let mut targets = self.load_connection_targets()?;
        let index = targets
            .manual_smcp_targets
            .iter()
            .position(|target| target.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        let removed = targets.manual_smcp_targets.remove(index);
        self.save_connection_targets(&targets)?;
        Ok(removed)
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }
}

fn map_client_computers_path_error(error: ClientComputersPathError) -> ConfigError {
    match error {
        ClientComputersPathError::InvalidInstanceId(id) => {
            ConfigError::InvalidComputerProfileId(id)
        }
    }
}

pub fn normalize_manual_smcp_target(mut target: ManualSmcpTarget) -> ManualSmcpTarget {
    target.id = stable_or_existing_manual_target_id(&target.id, &target);
    target
}

fn stable_or_existing_manual_target_id(id: &str, target: &ManualSmcpTarget) -> String {
    let id = id.trim();
    if id.is_empty() {
        stable_manual_target_id_from_parts(
            &target.name,
            &target.url,
            &target.office_id,
            &target.headers,
        )
    } else {
        id.to_string()
    }
}

fn stable_manual_target_id_from_parts(
    name: &str,
    url: &str,
    office_id: &str,
    headers: &HashMap<String, String>,
) -> String {
    let mut hasher = Sha256::new();
    hash_string_field(&mut hasher, name);
    hash_string_field(&mut hasher, url);
    hash_string_field(&mut hasher, office_id);
    let mut header_pairs: Vec<_> = headers.iter().collect();
    header_pairs.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in header_pairs {
        hash_string_field(&mut hasher, key);
        hash_string_field(&mut hasher, value);
    }
    let digest = hasher.finalize();
    format!("manual-{}", hex::encode(&digest[..16]))
}

fn hash_string_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_be_bytes());
    hasher.update(value.as_bytes());
}

fn load_json_file<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, ConfigError> {
    if !path.exists() {
        return Ok(T::default());
    }
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(T::default());
    }
    let data: T = serde_json::from_str(&content)?;
    Ok(data)
}

fn load_required_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Err(ConfigError::InvalidArtifact {
            path: path.to_path_buf(),
            reason: "file is empty".to_string(),
        });
    }
    serde_json::from_str(&content).map_err(ConfigError::Json)
}

fn load_new_artifact_or_default<T>(path: &Path) -> Result<T, ConfigError>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    load_required_json_file(path)
}

fn validate_schema_version(
    artifact: &'static str,
    actual: u32,
    expected: u32,
) -> Result<(), ConfigError> {
    if actual != expected {
        return Err(ConfigError::UnsupportedSchemaVersion {
            artifact,
            expected,
            actual,
        });
    }
    Ok(())
}

fn validate_computer_inputs_config(config: &ComputerInputsConfig) -> Result<(), ConfigError> {
    validate_schema_version(
        "Computer inputs",
        config.schema_version,
        COMPUTER_INPUTS_SCHEMA_VERSION,
    )?;
    validate_unique_artifact_ids(
        "Computer input",
        config.inputs.iter().map(|input| input.id()),
    )?;

    for input in &config.inputs {
        if let ComputerInputDefinition::PromptString {
            id,
            default: Some(default),
            password: Some(true),
            ..
        } = input
        {
            if !default.is_empty() {
                return Err(ConfigError::SecretPlaintextInComputerInput {
                    input_id: id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn sanitize_computer_inputs_config(config: &mut ComputerInputsConfig) {
    for input in &mut config.inputs {
        if let ComputerInputDefinition::PromptString {
            default, password, ..
        } = input
        {
            if *password == Some(true) {
                *default = None;
            }
        }
    }
}

fn validate_global_manual_targets_config(
    config: &GlobalManualTargetsConfig,
) -> Result<(), ConfigError> {
    validate_schema_version(
        "global manual targets",
        config.schema_version,
        MANUAL_TARGETS_SCHEMA_VERSION,
    )?;
    validate_unique_artifact_ids(
        "global manual target",
        config
            .manual_smcp_targets
            .iter()
            .map(|target| target.id.as_str()),
    )?;

    for target in &config.manual_smcp_targets {
        for (header, value) in &target.routing_headers {
            HeaderName::from_bytes(header.as_bytes()).map_err(|error| {
                ConfigError::InvalidRoutingHeaderInManualTarget {
                    target_id: target.id.clone(),
                    header: header.clone(),
                    reason: error.to_string(),
                }
            })?;
            HeaderValue::from_bytes(value.as_bytes()).map_err(|error| {
                ConfigError::InvalidRoutingHeaderInManualTarget {
                    target_id: target.id.clone(),
                    header: header.clone(),
                    reason: error.to_string(),
                }
            })?;
        }
    }
    Ok(())
}

fn validate_unique_artifact_ids<'a>(
    artifact: &'static str,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() || id != id.trim() {
            return Err(ConfigError::InvalidArtifactEntityId {
                artifact,
                id: id.to_string(),
            });
        }
        if !seen.insert(id) {
            return Err(ConfigError::DuplicateArtifactEntityId {
                artifact,
                id: id.to_string(),
            });
        }
    }
    Ok(())
}

pub(crate) fn sanitize_path_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

pub(crate) fn instance_storage_dir_name(instance_id: &str) -> String {
    let sanitized = sanitize_path_component(instance_id);
    if !instance_id.is_empty()
        && instance_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return sanitized;
    }

    let digest = Sha256::digest(instance_id.as_bytes());
    format!("{}-{}", sanitized, hex::encode(&digest[..4]))
}

fn save_json_file<T: serde::Serialize + ?Sized>(path: &Path, data: &T) -> Result<(), ConfigError> {
    write_json_atomically(path, data)?;
    Ok(())
}

fn validate_staged_computer_directory(
    staged_root: &Path,
    instance_id: &str,
) -> Result<(ComputerProfile, SdkContextConfig, ComputerInputsConfig), ConfigError> {
    let profile_path = staged_root.join(COMPUTER_PROFILE_FILE_NAME);
    let profile: ComputerProfile = load_required_json_file(&profile_path)?;
    validate_schema_version(
        "computer profile",
        profile.schema_version,
        COMPUTER_PROFILE_SCHEMA_VERSION,
    )?;
    if profile.id != instance_id {
        return Err(ConfigError::CorruptedComputerProfile {
            directory_id: instance_id.to_string(),
            profile_id: profile.id,
        });
    }

    let context_path = staged_root.join(crate::services::client_computers::SDK_CONTEXT_FILE_NAME);
    let context: SdkContextConfig = load_required_json_file(&context_path)?;
    validate_schema_version(
        "SDK context",
        context.schema_version,
        SDK_CONTEXT_SCHEMA_VERSION,
    )?;
    let inputs_path = staged_root.join(COMPUTER_INPUTS_FILE_NAME);
    let mut inputs: ComputerInputsConfig = load_required_json_file(&inputs_path)?;
    sanitize_computer_inputs_config(&mut inputs);
    validate_computer_inputs_config(&inputs)?;
    Ok((profile, context, inputs))
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("configuration lock error: {0}")]
    Lock(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(
        "Computer '{instance_id}' directory update failed: {primary}; rollback failed: {rollback}; transaction retained at {transaction_root}"
    )]
    DirectoryTransactionRollback {
        instance_id: String,
        primary: String,
        rollback: String,
        transaction_root: String,
    },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    AtomicJsonWrite(#[from] AtomicJsonWriteError),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid Computer profile id: {0}")]
    InvalidComputerProfileId(String),

    #[error(
        "corrupted Computer profile: directory id '{directory_id}' does not match profile id '{profile_id}'"
    )]
    CorruptedComputerProfile {
        directory_id: String,
        profile_id: String,
    },

    #[error("unsupported {artifact} schema version {actual}; expected {expected}")]
    UnsupportedSchemaVersion {
        artifact: &'static str,
        expected: u32,
        actual: u32,
    },

    #[error("invalid config artifact at {path}: {reason}")]
    InvalidArtifact { path: PathBuf, reason: String },

    #[error("Computer input '{input_id}' contains a password default; store it in keychain")]
    SecretPlaintextInComputerInput { input_id: String },

    #[error("{artifact} has invalid entity id '{id}'")]
    InvalidArtifactEntityId { artifact: &'static str, id: String },

    #[error("{artifact} has duplicate entity id '{id}'")]
    DuplicateArtifactEntityId { artifact: &'static str, id: String },

    #[error("manual target '{target_id}' contains invalid routing header '{header}': {reason}")]
    InvalidRoutingHeaderInManualTarget {
        target_id: String,
        header: String,
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::McpServerManagedBy;
    use crate::services::connection_targets::GlobalManualSmcpTarget;
    use tempfile::tempdir;

    const TEST_INSTANCE_ID: &str = "computer-a";

    fn setup() -> (ConfigService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = ConfigService::new(tmp.path().to_path_buf()).unwrap();
        svc.add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer"))
            .unwrap();
        (svc, tmp)
    }

    fn setup_empty() -> (ConfigService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = ConfigService::new(tmp.path().to_path_buf()).unwrap();
        (svc, tmp)
    }

    #[test]
    fn computer_profile_roundtrip_uses_client_owned_directory_schema() {
        let (svc, tmp) = setup_empty();
        let mut profile = ComputerProfile::new("computer-a", "Computer A");
        profile.description = Some("Desktop agent".to_string());
        profile.connection_policy.auto_connect = true;

        svc.save_computer_profile(&profile).unwrap();

        assert_eq!(svc.load_computer_profile("computer-a").unwrap(), profile);
        assert_eq!(
            svc.computer_profile_path("computer-a").unwrap(),
            tmp.path()
                .join("client_computers/instances/computer-a/profile.json")
        );
    }

    #[test]
    fn computer_profile_serialization_excludes_runtime_sdk_and_secret_fields() {
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.inputs.push(InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: "API key".to_string(),
            description: None,
            default: None,
            password: Some(true),
        });
        instance
            .input_values
            .insert("api-key".to_string(), serde_json::json!("secret-value"));

        let serialized = serde_json::to_value(ComputerProfile::from(&instance)).unwrap();
        let object = serialized.as_object().unwrap();

        assert_eq!(
            object
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            ["connection_policy", "id", "name", "schema_version"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        assert!(!serialized.to_string().contains("secret-value"));
        assert!(!object.contains_key("inputs"));
        assert!(!object.contains_key("mcp_servers"));
        assert!(!object.contains_key("input_values"));
    }

    #[test]
    fn computer_profile_rejects_directory_and_profile_id_mismatch() {
        let (svc, _tmp) = setup_empty();
        let path = svc.computer_profile_path("directory-id").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            serde_json::to_string_pretty(&ComputerProfile::new("profile-id", "Computer")).unwrap(),
        )
        .unwrap();

        let error = svc.load_computer_profile("directory-id").unwrap_err();
        assert!(matches!(
            error,
            ConfigError::CorruptedComputerProfile {
                directory_id,
                profile_id
            } if directory_id == "directory-id" && profile_id == "profile-id"
        ));
    }

    #[test]
    fn computer_profile_rejects_unsafe_instance_id() {
        let (svc, _tmp) = setup_empty();

        assert!(matches!(
            svc.computer_profile_path("../outside").unwrap_err(),
            ConfigError::InvalidComputerProfileId(id) if id == "../outside"
        ));
    }

    #[test]
    fn versioned_client_owned_artifacts_reject_unknown_schema_versions() {
        let (svc, _tmp) = setup_empty();
        let profile = ComputerProfile {
            schema_version: COMPUTER_PROFILE_SCHEMA_VERSION + 1,
            ..ComputerProfile::new("computer-a", "Computer")
        };
        let inputs = ComputerInputsConfig {
            schema_version: COMPUTER_INPUTS_SCHEMA_VERSION + 1,
            inputs: Vec::new(),
        };

        assert!(matches!(
            svc.save_computer_profile(&profile).unwrap_err(),
            ConfigError::UnsupportedSchemaVersion {
                artifact: "computer profile",
                ..
            }
        ));
        assert!(matches!(
            svc.save_computer_inputs(TEST_INSTANCE_ID, &inputs)
                .unwrap_err(),
            ConfigError::UnsupportedSchemaVersion {
                artifact: "Computer inputs",
                ..
            }
        ));
    }

    #[test]
    fn computer_inputs_and_global_manual_targets_are_stored_separately() {
        let (svc, tmp) = setup_empty();
        let inputs = ComputerInputsConfig {
            schema_version: COMPUTER_INPUTS_SCHEMA_VERSION,
            inputs: vec![ComputerInputDefinition::PromptString {
                id: "region".to_string(),
                label: "Region".to_string(),
                description: None,
                default: Some("us-east".to_string()),
                password: None,
            }],
        };
        let targets = GlobalManualTargetsConfig {
            schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
            manual_smcp_targets: vec![GlobalManualSmcpTarget {
                id: "target-a".to_string(),
                name: "Target A".to_string(),
                url: "https://smcp.example.com".to_string(),
                namespace: "/smcp".to_string(),
                office_id: "office-a".to_string(),
                routing_headers: HashMap::from([("X-TF-Region".to_string(), "cn".to_string())]),
            }],
        };

        svc.save_computer_inputs(TEST_INSTANCE_ID, &inputs).unwrap();
        svc.save_global_manual_targets(&targets).unwrap();

        let loaded_inputs = svc.load_computer_inputs(TEST_INSTANCE_ID).unwrap();
        let loaded_targets = svc.load_global_manual_targets().unwrap();
        assert_eq!(loaded_inputs.inputs[0].id(), "region");
        assert_eq!(loaded_targets.manual_smcp_targets[0].id, "target-a");
        assert!(tmp
            .path()
            .join("client_computers/instances/computer-a/inputs.json")
            .exists());
        assert!(tmp
            .path()
            .join("client_computers/global/manual_targets.json")
            .exists());
        assert!(!std::fs::read_to_string(
            tmp.path()
                .join("client_computers/instances/computer-a/inputs.json")
        )
        .unwrap()
        .contains("input_values"));
    }

    #[test]
    fn computer_inputs_strip_password_default_plaintext() {
        let (svc, _tmp) = setup_empty();
        let inputs = ComputerInputsConfig {
            schema_version: COMPUTER_INPUTS_SCHEMA_VERSION,
            inputs: vec![ComputerInputDefinition::PromptString {
                id: "api-key".to_string(),
                label: "API key".to_string(),
                description: None,
                default: Some("plaintext-secret".to_string()),
                password: Some(true),
            }],
        };

        svc.save_computer_inputs(TEST_INSTANCE_ID, &inputs).unwrap();
        let inputs_path = svc.computer_inputs_path(TEST_INSTANCE_ID).unwrap();
        let stored = std::fs::read_to_string(&inputs_path).unwrap();
        assert!(!stored.contains("plaintext-secret"));
        assert!(matches!(
            svc.load_computer_inputs(TEST_INSTANCE_ID)
                .unwrap()
                .inputs
                .as_slice(),
            [ComputerInputDefinition::PromptString { default: None, .. }]
        ));

        std::fs::write(inputs_path, serde_json::to_vec_pretty(&inputs).unwrap()).unwrap();
        assert!(matches!(
            svc.load_computer_inputs(TEST_INSTANCE_ID)
                .unwrap()
                .inputs
                .as_slice(),
            [ComputerInputDefinition::PromptString { default: None, .. }]
        ));
    }

    #[test]
    fn global_manual_targets_persist_custom_routing_headers_without_name_guessing() {
        let (svc, tmp) = setup_empty();
        let targets = GlobalManualTargetsConfig {
            schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
            manual_smcp_targets: vec![GlobalManualSmcpTarget {
                id: "target-a".to_string(),
                name: "Target A".to_string(),
                url: "https://smcp.example.com".to_string(),
                namespace: "/smcp".to_string(),
                office_id: "office-a".to_string(),
                routing_headers: HashMap::from([
                    ("Authorization".to_string(), "Bearer plaintext".to_string()),
                    ("X-Correlation-Id".to_string(), "trace-a".to_string()),
                    ("X-Client-Secret".to_string(), "routing-value".to_string()),
                ]),
            }],
        };

        svc.save_global_manual_targets(&targets).unwrap();

        assert_eq!(svc.load_global_manual_targets().unwrap(), targets);
        let persisted = std::fs::read_to_string(
            tmp.path()
                .join("client_computers/global/manual_targets.json"),
        )
        .unwrap();
        assert!(persisted.contains("Authorization"));
        assert!(persisted.contains("X-Correlation-Id"));
        assert!(!persisted.contains("api_key"));
    }

    #[test]
    fn global_manual_targets_reject_invalid_http_header_syntax() {
        let (svc, _tmp) = setup_empty();
        let mut target = GlobalManualSmcpTarget {
            id: "target-a".to_string(),
            name: "Target A".to_string(),
            url: "https://smcp.example.com".to_string(),
            namespace: "/smcp".to_string(),
            office_id: "office-a".to_string(),
            routing_headers: HashMap::from([(" Invalid Header ".to_string(), "value".to_string())]),
        };

        let config = |target| GlobalManualTargetsConfig {
            schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
            manual_smcp_targets: vec![target],
        };
        assert!(matches!(
            svc.save_global_manual_targets(&config(target.clone()))
                .unwrap_err(),
            ConfigError::InvalidRoutingHeaderInManualTarget { header, .. }
                if header == " Invalid Header "
        ));

        target.routing_headers =
            HashMap::from([("X-Correlation-Id".to_string(), "line\r\nbreak".to_string())]);
        assert!(matches!(
            svc.save_global_manual_targets(&config(target)).unwrap_err(),
            ConfigError::InvalidRoutingHeaderInManualTarget { header, .. }
                if header == "X-Correlation-Id"
        ));
    }

    #[test]
    fn global_artifact_entity_ids_must_be_non_empty_and_unique_on_save() {
        let (svc, _tmp) = setup_empty();
        let prompt = |id: &str| ComputerInputDefinition::PromptString {
            id: id.to_string(),
            label: "Input".to_string(),
            description: None,
            default: None,
            password: None,
        };
        let target = |id: &str| GlobalManualSmcpTarget {
            id: id.to_string(),
            name: "Target".to_string(),
            url: "https://smcp.example.com".to_string(),
            namespace: "/smcp".to_string(),
            office_id: "office-a".to_string(),
            routing_headers: HashMap::new(),
        };

        let invalid_inputs = ComputerInputsConfig {
            schema_version: COMPUTER_INPUTS_SCHEMA_VERSION,
            inputs: vec![prompt(" ")],
        };
        assert!(matches!(
            svc.save_computer_inputs(TEST_INSTANCE_ID, &invalid_inputs)
                .unwrap_err(),
            ConfigError::InvalidArtifactEntityId {
                artifact: "Computer input",
                ..
            }
        ));

        let duplicate_targets = GlobalManualTargetsConfig {
            schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
            manual_smcp_targets: vec![target("target-a"), target("target-a")],
        };
        assert!(matches!(
            svc.save_global_manual_targets(&duplicate_targets)
                .unwrap_err(),
            ConfigError::DuplicateArtifactEntityId {
                artifact: "global manual target",
                id
            } if id == "target-a"
        ));
    }

    #[test]
    fn global_artifact_entity_ids_are_validated_on_load() {
        let (svc, _tmp) = setup_empty();
        let inputs_path = svc.computer_inputs_path(TEST_INSTANCE_ID).unwrap();
        std::fs::create_dir_all(inputs_path.parent().unwrap()).unwrap();
        std::fs::write(
            inputs_path,
            r#"{
              "schema_version": 1,
              "inputs": [
                {"type": "Command", "id": "duplicate", "label": "One", "command": "one"},
                {"type": "Command", "id": "duplicate", "label": "Two", "command": "two"}
              ]
            }"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_computer_inputs(TEST_INSTANCE_ID).unwrap_err(),
            ConfigError::DuplicateArtifactEntityId {
                artifact: "Computer input",
                id
            } if id == "duplicate"
        ));

        let targets_path = svc.global_config_path(GlobalConfigFile::ManualTargets);
        std::fs::create_dir_all(targets_path.parent().unwrap()).unwrap();
        std::fs::write(
            targets_path,
            r#"{
              "schema_version": 1,
              "manual_smcp_targets": [{
                "id": "",
                "name": "Target",
                "url": "https://smcp.example.com",
                "office_id": "office-a"
              }]
            }"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_global_manual_targets().unwrap_err(),
            ConfigError::InvalidArtifactEntityId {
                artifact: "global manual target",
                ..
            }
        ));
    }

    #[test]
    fn missing_input_and_global_target_artifacts_return_current_version_defaults() {
        let (svc, _tmp) = setup_empty();

        let inputs = svc.load_computer_inputs(TEST_INSTANCE_ID).unwrap();
        let targets = svc.load_global_manual_targets().unwrap();

        assert_eq!(inputs.schema_version, COMPUTER_INPUTS_SCHEMA_VERSION);
        assert!(inputs.inputs.is_empty());
        assert_eq!(targets.schema_version, MANUAL_TARGETS_SCHEMA_VERSION);
        assert!(targets.manual_smcp_targets.is_empty());
    }

    #[test]
    fn computer_profile_rejects_unknown_fields_and_corrupted_json() {
        let (svc, _tmp) = setup_empty();
        let path = svc.computer_profile_path("computer-a").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{
              "schema_version": 1,
              "id": "computer-a",
              "name": "Computer",
              "connection_policy": {"auto_connect": false},
              "input_values": {"api-key": "plaintext"}
            }"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_computer_profile("computer-a").unwrap_err(),
            ConfigError::Json(_)
        ));

        std::fs::write(path, "not json").unwrap();
        assert!(matches!(
            svc.load_computer_profile("computer-a").unwrap_err(),
            ConfigError::Json(_)
        ));
    }

    #[test]
    fn profile_discovery_is_deterministic_and_isolates_invalid_directories() {
        let (svc, _tmp) = setup_empty();
        svc.save_computer_profile(&ComputerProfile::new("computer-b", "B"))
            .unwrap();
        svc.save_computer_profile(&ComputerProfile::new("computer-a", "A"))
            .unwrap();
        svc.save_computer_profile(&ComputerProfile::new("bad-context", "Bad context"))
            .unwrap();
        std::fs::write(
            svc.sdk_context_path("bad-context").unwrap(),
            r#"{"schema_version": 99}"#,
        )
        .unwrap();

        let corrupt_path = svc.computer_profile_path("corrupt").unwrap();
        std::fs::create_dir_all(corrupt_path.parent().unwrap()).unwrap();
        std::fs::write(&corrupt_path, "not json").unwrap();
        let mismatched_path = svc.computer_profile_path("directory-id").unwrap();
        std::fs::create_dir_all(mismatched_path.parent().unwrap()).unwrap();
        std::fs::write(
            &mismatched_path,
            serde_json::to_vec(&ComputerProfile::new("profile-id", "Mismatch")).unwrap(),
        )
        .unwrap();

        let discovery = svc.discover_computer_instances().unwrap();

        assert_eq!(
            discovery
                .config
                .instances
                .iter()
                .map(|instance| instance.id.as_str())
                .collect::<Vec<_>>(),
            vec!["computer-a", "computer-b"]
        );
        assert_eq!(discovery.errors.len(), 3);
        assert!(discovery
            .errors
            .iter()
            .any(|error| error.path == corrupt_path));
        assert!(discovery
            .errors
            .iter()
            .any(|error| error.path == mismatched_path));
        assert!(discovery
            .errors
            .iter()
            .any(|error| { error.path == svc.sdk_context_path("bad-context").unwrap() }));
    }

    #[test]
    fn discovery_recovers_interrupted_directory_swap_without_partial_profile() {
        let (svc, _tmp) = setup_empty();
        svc.add_computer_instance(ComputerInstance::new("computer-a", "Original"))
            .unwrap();
        let instance_root = svc.computer_instance_root("computer-a").unwrap();
        let transactions_root = svc.client_computers_paths.root().join(".transactions");
        let transaction_root = transactions_root.join("interrupted");
        let previous_root = transaction_root.join("old");
        let staged_root = transaction_root.join("new");
        std::fs::create_dir_all(&staged_root).unwrap();
        save_json_file(
            &transaction_root.join("transaction.json"),
            &ComputerDirectoryTransaction {
                schema_version: 1,
                instance_id: "computer-a".to_string(),
                phase: ComputerDirectoryTransactionPhase::Ready,
            },
        )
        .unwrap();
        save_json_file(
            &staged_root.join(COMPUTER_PROFILE_FILE_NAME),
            &ComputerProfile::new("computer-a", "Replacement"),
        )
        .unwrap();
        save_json_file(
            &staged_root.join(crate::services::client_computers::SDK_CONTEXT_FILE_NAME),
            &SdkContextConfig::default(),
        )
        .unwrap();
        save_json_file(
            &staged_root.join(COMPUTER_INPUTS_FILE_NAME),
            &ComputerInputsConfig::default(),
        )
        .unwrap();
        std::fs::rename(&instance_root, &previous_root).unwrap();

        let discovered = svc.discover_computer_instances().unwrap();

        assert_eq!(discovered.config.instances[0].name, "Original");
        assert!(instance_root.join(COMPUTER_PROFILE_FILE_NAME).is_file());
        assert!(instance_root
            .join(crate::services::client_computers::SDK_CONTEXT_FILE_NAME)
            .is_file());
        assert!(!transactions_root.exists());
    }

    #[test]
    fn discovery_discards_markerless_partial_directory_transaction() {
        let (svc, _tmp) = setup_empty();
        let transactions_root = svc.client_computers_paths.root().join(".transactions");
        let transaction_root = transactions_root.join("markerless");
        let staged_root = transaction_root.join("new");
        std::fs::create_dir_all(&staged_root).unwrap();
        save_json_file(
            &staged_root.join(COMPUTER_PROFILE_FILE_NAME),
            &ComputerProfile::new("computer-a", "Partial"),
        )
        .unwrap();

        let discovery = svc.discover_computer_instances().unwrap();

        assert!(discovery.config.instances.is_empty());
        assert!(discovery.errors.is_empty());
        assert!(!svc.computer_instance_root("computer-a").unwrap().exists());
        assert!(!transactions_root.exists());
    }

    #[test]
    fn discovery_does_not_publish_transaction_with_only_profile_staged() {
        let (svc, _tmp) = setup_empty();
        let transactions_root = svc.client_computers_paths.root().join(".transactions");
        let transaction_root = transactions_root.join("only-profile");
        let staged_root = transaction_root.join("new");
        std::fs::create_dir_all(&staged_root).unwrap();
        save_json_file(
            &transaction_root.join("transaction.json"),
            &ComputerDirectoryTransaction {
                schema_version: 1,
                instance_id: "computer-a".to_string(),
                phase: ComputerDirectoryTransactionPhase::Preparing,
            },
        )
        .unwrap();
        save_json_file(
            &staged_root.join(COMPUTER_PROFILE_FILE_NAME),
            &ComputerProfile::new("computer-a", "Partial"),
        )
        .unwrap();

        let discovery = svc.discover_computer_instances().unwrap();

        assert!(discovery.config.instances.is_empty());
        assert!(discovery.errors.is_empty());
        assert!(!svc.computer_instance_root("computer-a").unwrap().exists());
        assert!(!transactions_root.exists());
    }

    #[test]
    fn discovery_discards_preparing_transaction_with_no_artifacts() {
        let (svc, _tmp) = setup_empty();
        let transactions_root = svc.client_computers_paths.root().join(".transactions");
        let transaction_root = transactions_root.join("marker-only");
        std::fs::create_dir_all(&transaction_root).unwrap();
        save_json_file(
            &transaction_root.join("transaction.json"),
            &ComputerDirectoryTransaction {
                schema_version: 1,
                instance_id: "computer-a".to_string(),
                phase: ComputerDirectoryTransactionPhase::Preparing,
            },
        )
        .unwrap();

        let discovery = svc.discover_computer_instances().unwrap();

        assert!(discovery.config.instances.is_empty());
        assert!(discovery.errors.is_empty());
        assert!(!svc.computer_instance_root("computer-a").unwrap().exists());
        assert!(!transactions_root.exists());
    }

    #[test]
    fn discovery_publishes_only_complete_ready_directory_transaction() {
        let (svc, tmp) = setup_empty();
        std::fs::create_dir_all(svc.client_computers_paths.instances_root()).unwrap();
        let transactions_root = svc.client_computers_paths.root().join(".transactions");
        let transaction_root = transactions_root.join("ready");
        let staged_root = transaction_root.join("new");
        std::fs::create_dir_all(&staged_root).unwrap();
        let skill_home = tmp.path().join("custom-skill-home");
        save_json_file(
            &staged_root.join(COMPUTER_PROFILE_FILE_NAME),
            &ComputerProfile::new("computer-a", "Ready"),
        )
        .unwrap();
        save_json_file(
            &staged_root.join(crate::services::client_computers::SDK_CONTEXT_FILE_NAME),
            &SdkContextConfig {
                schema_version: SDK_CONTEXT_SCHEMA_VERSION,
                skill_home_override: Some(skill_home.clone()),
            },
        )
        .unwrap();
        save_json_file(
            &staged_root.join(COMPUTER_INPUTS_FILE_NAME),
            &ComputerInputsConfig::default(),
        )
        .unwrap();
        save_json_file(
            &transaction_root.join("transaction.json"),
            &ComputerDirectoryTransaction {
                schema_version: 1,
                instance_id: "computer-a".to_string(),
                phase: ComputerDirectoryTransactionPhase::Ready,
            },
        )
        .unwrap();

        let discovery = svc.discover_computer_instances().unwrap();

        assert_eq!(discovery.config.instances.len(), 1);
        assert_eq!(discovery.config.instances[0].name, "Ready");
        assert_eq!(
            discovery.config.instances[0].local_skills_root,
            Some(skill_home)
        );
        assert!(discovery.errors.is_empty());
        assert!(!transactions_root.exists());
    }

    #[test]
    fn failed_directory_swap_and_rollback_retains_original_for_recovery() {
        let (svc, _tmp) = setup_empty();
        svc.add_computer_instance(ComputerInstance::new("computer-a", "Original"))
            .unwrap();
        svc.inject_directory_rename_actions([
            DirectoryRenameTestAction::Proceed,
            DirectoryRenameTestAction::Fail,
            DirectoryRenameTestAction::Fail,
        ]);

        let error = svc
            .rename_computer_instance("computer-a", "Replacement".to_string())
            .unwrap_err();

        assert!(matches!(
            error,
            ConfigError::DirectoryTransactionRollback {
                ref instance_id,
                ref primary,
                ref rollback,
                ..
            } if instance_id == "computer-a"
                && primary.contains("injected directory rename failure")
                && rollback.contains("injected directory rename failure")
        ));
        let transactions_root = svc.client_computers_paths.root().join(".transactions");
        let transaction_roots = std::fs::read_dir(&transactions_root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(transaction_roots.len(), 1);
        let retained_profile: ComputerProfile = load_required_json_file(
            &transaction_roots[0]
                .join("old")
                .join(COMPUTER_PROFILE_FILE_NAME),
        )
        .unwrap();
        assert_eq!(retained_profile.name, "Original");

        let discovered = svc.discover_computer_instances().unwrap();

        assert_eq!(discovered.config.instances[0].name, "Original");
        assert!(!transactions_root.exists());
    }

    #[test]
    fn new_artifacts_reject_nested_unknown_fields() {
        let (svc, _tmp) = setup_empty();

        let profile_path = svc.computer_profile_path("computer-a").unwrap();
        std::fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
        std::fs::write(
            profile_path,
            r#"{
              "schema_version": 1,
              "id": "computer-a",
              "name": "Computer",
              "connection_policy": {
                "auto_connect": false,
                "input_values": {"api-key": "plaintext"}
              }
            }"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_computer_profile("computer-a").unwrap_err(),
            ConfigError::Json(_)
        ));

        let inputs_path = svc.computer_inputs_path(TEST_INSTANCE_ID).unwrap();
        std::fs::create_dir_all(inputs_path.parent().unwrap()).unwrap();
        std::fs::write(
            inputs_path,
            r#"{
              "schema_version": 1,
              "inputs": [{
                "type": "PromptString",
                "id": "api-key",
                "label": "API key",
                "resolved_value": "plaintext"
              }]
            }"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_computer_inputs(TEST_INSTANCE_ID).unwrap_err(),
            ConfigError::Json(_)
        ));

        let targets_path = svc.global_config_path(GlobalConfigFile::ManualTargets);
        std::fs::create_dir_all(targets_path.parent().unwrap()).unwrap();
        std::fs::write(
            targets_path,
            r#"{
              "schema_version": 1,
              "manual_smcp_targets": [{
                "id": "target-a",
                "name": "Target A",
                "url": "https://smcp.example.com",
                "office_id": "office-a",
                "api_key": "plaintext"
              }]
            }"#,
        )
        .unwrap();
        assert!(matches!(
            svc.load_global_manual_targets().unwrap_err(),
            ConfigError::Json(_)
        ));
    }

    #[test]
    fn existing_empty_global_artifacts_are_reported_as_corrupted() {
        let (svc, _tmp) = setup_empty();
        for path in [
            svc.computer_inputs_path(TEST_INSTANCE_ID).unwrap(),
            svc.global_config_path(GlobalConfigFile::ManualTargets),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }

        assert!(matches!(
            svc.load_computer_inputs(TEST_INSTANCE_ID).unwrap_err(),
            ConfigError::InvalidArtifact { .. }
        ));
        assert!(matches!(
            svc.load_global_manual_targets().unwrap_err(),
            ConfigError::InvalidArtifact { .. }
        ));
    }

    #[test]
    fn global_artifacts_reject_unknown_schema_versions_when_loaded() {
        let (svc, _tmp) = setup_empty();
        let inputs_path = svc.computer_inputs_path(TEST_INSTANCE_ID).unwrap();
        std::fs::create_dir_all(inputs_path.parent().unwrap()).unwrap();
        std::fs::write(inputs_path, r#"{"schema_version": 2, "inputs": []}"#).unwrap();
        let targets_path = svc.global_config_path(GlobalConfigFile::ManualTargets);
        std::fs::create_dir_all(targets_path.parent().unwrap()).unwrap();
        std::fs::write(
            targets_path,
            r#"{"schema_version": 2, "manual_smcp_targets": []}"#,
        )
        .unwrap();

        assert!(matches!(
            svc.load_computer_inputs(TEST_INSTANCE_ID).unwrap_err(),
            ConfigError::UnsupportedSchemaVersion {
                artifact: "Computer inputs",
                ..
            }
        ));
        assert!(matches!(
            svc.load_global_manual_targets().unwrap_err(),
            ConfigError::UnsupportedSchemaVersion {
                artifact: "global manual targets",
                ..
            }
        ));
    }

    #[test]
    fn existing_global_manual_targets_requires_explicit_schema_version() {
        let (svc, _tmp) = setup_empty();
        let path = svc.global_config_path(GlobalConfigFile::ManualTargets);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, r#"{"manual_smcp_targets": []}"#).unwrap();

        assert!(matches!(
            svc.load_global_manual_targets().unwrap_err(),
            ConfigError::Json(_)
        ));
    }

    #[test]
    fn new_profile_repository_does_not_change_legacy_instances_file() {
        let (svc, tmp) = setup_empty();
        let legacy = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![ComputerInstance::new("legacy", "Legacy")],
        };
        save_json_file(&tmp.path().join("computer_instances.json"), &legacy).unwrap();
        svc.save_computer_profile(&ComputerProfile::new("new-profile", "New"))
            .unwrap();

        let legacy = svc.load_legacy_computer_instances().unwrap();
        assert_eq!(legacy.instances.len(), 1);
        assert_eq!(legacy.instances[0].id, "legacy");
        assert_eq!(svc.load_computer_instances().unwrap().instances.len(), 1);
        assert!(tmp.path().join("computer_instances.json").exists());
    }

    // --- Legacy MCP migration ---

    #[test]
    fn legacy_mcp_migration_read_does_not_treat_new_profiles_as_legacy() {
        let (svc, _tmp) = setup();
        assert!(matches!(
            svc.load_legacy_mcp_configs_for_migration(TEST_INSTANCE_ID),
            Err(ConfigError::NotFound(id)) if id == TEST_INSTANCE_ID
        ));
    }

    #[test]
    fn test_empty_computer_instances_remain_empty() {
        let (svc, _tmp) = setup_empty();
        let instances = svc.load_computer_instances().unwrap();

        assert!(instances.instances.is_empty());
    }

    #[test]
    fn default_skill_roots_avoid_sanitized_id_collisions() {
        let (svc, _tmp) = setup_empty();

        assert_eq!(
            instance_storage_dir_name("computer-a"),
            "computer-a".to_string()
        );
        assert_ne!(
            svc.default_local_skills_root("instance/one"),
            svc.default_local_skills_root("instance_one")
        );
        assert_ne!(
            instance_storage_dir_name("instance/one"),
            instance_storage_dir_name("instance:one")
        );
    }

    #[test]
    fn explicit_migration_read_loads_legacy_inline_mcp_as_user_managed() {
        let (svc, tmp) = setup_empty();
        std::fs::write(
            tmp.path().join("computer_instances.json"),
            r#"{
              "schema_version": 1,
              "instances": [
                {
                  "id": "legacy",
                  "name": "Legacy",
                  "mcp_servers": [
                    {
                      "type": "Stdio",
                      "name": "legacy-inline",
                      "server_parameters": {
                        "command": "node",
                        "args": ["server.js"],
                        "env": {}
                      }
                    }
                  ],
                  "inputs": [],
                  "input_values": {},
                  "connection_policy": {
                    "target": null,
                    "auto_connect": false
                  }
                }
              ]
            }"#,
        )
        .unwrap();

        let servers = svc.load_legacy_mcp_configs_for_migration("legacy").unwrap();

        assert_eq!(servers.len(), 1);
        let server = &servers[0];
        assert_eq!(server.name(), "legacy-inline");
        assert!(!server.is_plugin_owned());
        assert!(matches!(server.managed_by, McpServerManagedBy::User));
    }

    #[test]
    fn test_legacy_config_files_are_ignored() {
        let (svc, tmp) = setup_empty();
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "legacy-server",
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap();

        save_json_file(&tmp.path().join("mcp_servers.json"), &[config]).unwrap();

        let instances = svc.load_computer_instances().unwrap();
        assert!(instances.instances.is_empty());
    }

    #[test]
    fn test_corrupted_legacy_files_are_ignored() {
        let (svc, tmp) = setup_empty();
        std::fs::write(tmp.path().join("mcp_servers.json"), "not json").unwrap();
        std::fs::write(tmp.path().join("inputs.json"), "not json").unwrap();

        let instances = svc.load_computer_instances().unwrap();
        assert!(instances.instances.is_empty());
    }

    #[test]
    fn test_empty_new_instances_file_uses_empty_config() {
        let (svc, tmp) = setup_empty();
        std::fs::write(tmp.path().join("computer_instances.json"), "").unwrap();

        let loaded = svc.load_computer_instances().unwrap();

        assert!(loaded.instances.is_empty());
    }

    #[test]
    fn legacy_mcp_migration_read_is_instance_scoped() {
        let (svc, _tmp) = setup();
        let first_config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "first-server",
            "server_parameters": { "command": "node", "args": [], "env": {} }
        }))
        .unwrap();
        let second_config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "second-server",
            "server_parameters": { "command": "python", "args": [], "env": {} }
        }))
        .unwrap();
        let instances = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![
                ComputerInstance {
                    id: "first".to_string(),
                    name: "First".to_string(),
                    mcp_servers: vec![first_config.into()],
                    ..ComputerInstance::new("", "")
                },
                ComputerInstance {
                    id: "second".to_string(),
                    name: "Second".to_string(),
                    mcp_servers: vec![second_config.into()],
                    ..ComputerInstance::new("", "")
                },
            ],
        };

        save_json_file(svc.legacy_computer_instances_path(), &instances).unwrap();
        let first = svc.load_legacy_mcp_configs_for_migration("first").unwrap();
        let second = svc.load_legacy_mcp_configs_for_migration("second").unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name(), "first-server");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].name(), "second-server");
    }

    #[test]
    fn test_add_get_rename_and_remove_computer_instance() {
        let (svc, _tmp) = setup();
        let instance = ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::new("", "")
        };

        svc.add_computer_instance(instance).unwrap();
        assert_eq!(svc.get_computer_instance("second").unwrap().name, "Second");

        let renamed = svc
            .rename_computer_instance("second", "Renamed".to_string())
            .unwrap();
        assert_eq!(renamed.name, "Renamed");
        assert_eq!(svc.get_computer_instance("second").unwrap().name, "Renamed");

        let removed = svc.remove_computer_instance("second").unwrap();
        assert_eq!(removed.id, "second");
        assert!(svc.get_computer_instance("second").is_err());
    }

    #[test]
    fn test_add_duplicate_computer_instance_returns_error() {
        let (svc, _tmp) = setup();
        let instance = ComputerInstance {
            id: "dup".to_string(),
            name: "Duplicate".to_string(),
            ..ComputerInstance::new("", "")
        };

        svc.add_computer_instance(instance.clone()).unwrap();
        let error = svc.add_computer_instance(instance).unwrap_err();

        assert!(error.to_string().contains("Already exists"));
    }

    #[test]
    fn test_remove_legacy_default_id_computer_instance_succeeds() {
        let (svc, _tmp) = setup();

        let removed = svc.remove_computer_instance(TEST_INSTANCE_ID).unwrap();

        assert_eq!(removed.id, TEST_INSTANCE_ID);
        assert!(svc.get_computer_instance(TEST_INSTANCE_ID).is_err());
    }

    #[test]
    fn legacy_migration_read_reports_corrupted_instances_file() {
        let (svc, tmp) = setup();
        std::fs::write(tmp.path().join("computer_instances.json"), "not json").unwrap();
        let result = svc.load_legacy_mcp_configs_for_migration(TEST_INSTANCE_ID);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_empty_json_file_returns_empty_config() {
        let (svc, tmp) = setup_empty();
        std::fs::write(tmp.path().join("computer_instances.json"), "").unwrap();
        let instances = svc.load_computer_instances().unwrap();
        assert!(instances.instances.is_empty());
    }

    // --- Input Definitions ---

    #[test]
    fn test_load_empty_inputs() {
        let (svc, _tmp) = setup();
        let inputs = svc.load_inputs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_save_and_load_inputs_roundtrip() {
        let (svc, _tmp) = setup();
        let input: InputDefinition = serde_json::from_value(serde_json::json!({
            "type": "PromptString",
            "id": "api-key",
            "label": "API Key",
            "password": true
        }))
        .unwrap();

        svc.save_inputs_for_instance(TEST_INSTANCE_ID, &[input])
            .unwrap();
        let loaded = svc.load_inputs_for_instance(TEST_INSTANCE_ID).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_manual_target_id_is_stable_and_independent_of_header_order() {
        let mut headers_a = HashMap::new();
        headers_a.insert("X-TF-Namespace".to_string(), "ns".to_string());
        headers_a.insert("X-TF-Region".to_string(), "cn".to_string());

        let mut headers_b = HashMap::new();
        headers_b.insert("X-TF-Region".to_string(), "cn".to_string());
        headers_b.insert("X-TF-Namespace".to_string(), "ns".to_string());

        let target_a = ManualSmcpTarget {
            id: String::new(),
            name: "prod".to_string(),
            url: "https://smcp.example.com".to_string(),
            namespace: "/smcp".to_string(),
            office_id: "office-1".to_string(),
            headers: headers_a,
        };
        let target_b = ManualSmcpTarget {
            headers: headers_b,
            ..target_a.clone()
        };

        let id_a = normalize_manual_smcp_target(target_a).id;
        let id_b = normalize_manual_smcp_target(target_b).id;

        assert_eq!(id_a, id_b);
        assert!(id_a.starts_with("manual-"));
        assert_eq!(id_a.len(), "manual-".len() + 32);
    }

    #[test]
    fn test_loads_legacy_manual_target_computer_name_without_using_it() {
        let (svc, tmp) = setup();
        let path = tmp.path().join("connection_targets.json");
        std::fs::write(
            path,
            serde_json::json!({
                "schema_version": 1,
                "manual_smcp_targets": [{
                    "id": "legacy-target",
                    "name": "Legacy",
                    "url": "https://smcp.example.com",
                    "namespace": "/smcp",
                    "office_id": "office-1",
                    "computer_name": "old-wire-name",
                    "headers": { "X-TF-Namespace": "ns" }
                }]
            })
            .to_string(),
        )
        .unwrap();

        let targets = svc
            .load_legacy_connection_targets()
            .unwrap()
            .manual_smcp_targets;

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "legacy-target");
        assert_eq!(targets[0].office_id, "office-1");
    }
}
