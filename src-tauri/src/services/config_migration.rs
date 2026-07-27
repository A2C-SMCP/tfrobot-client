use crate::services::computer::{ComputerProfile, SdkContextConfig, SDK_CONTEXT_SCHEMA_VERSION};
use crate::services::config::{normalize_manual_smcp_target, ConfigError, ConfigService};
use crate::services::connection_targets::{
    manual_target_keychain_id, GlobalManualSmcpTarget, GlobalManualTargetsConfig,
    MANUAL_TARGETS_SCHEMA_VERSION,
};
use crate::services::keychain::{KeychainError, SecretStore};
use crate::services::sdk_config::{normalize_mcp_input_references, SdkConfigService};
use crate::services::settings::{
    ManagerSessionConfig, ManagerSessionConfigError, PersistedManagerSession, SettingsService,
    MANAGER_SESSION_SCHEMA_VERSION,
};
use crate::services::storage::{write_json_atomically, AtomicJsonWriteError};
use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const MIGRATION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    NotNeeded,
    AlreadyCompleted,
    Completed,
}

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    ManagerSession(#[from] ManagerSessionConfigError),
    #[error(transparent)]
    Keychain(#[from] KeychainError),
    #[error(transparent)]
    AtomicWrite(#[from] AtomicJsonWriteError),
    #[error("migration IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("legacy migration conflict: {0}")]
    Conflict(String),
    #[error("SDK config migration failed for Computer '{instance_id}': {reason}")]
    Sdk { instance_id: String, reason: String },
    #[error("migration verification failed: {0}")]
    Verification(String),
    #[error("migration failed: {primary}; rollback also failed: {rollback}")]
    Rollback { primary: String, rollback: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationState {
    schema_version: u32,
    completed_at: String,
    #[serde(default)]
    cleanup_completed: bool,
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> Result<Self, std::io::Error> {
        let content = if path.exists() {
            Some(fs::read(&path)?)
        } else {
            None
        };
        Ok(Self { path, content })
    }

    fn restore(&self) -> Result<(), std::io::Error> {
        match &self.content {
            Some(content) => {
                if let Some(parent) = self.path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&self.path, content)
            }
            None => match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        }
    }
}

#[derive(Debug)]
struct SdkMutation {
    instance_id: String,
    before: ProjectConfigDoc,
    after: ProjectConfigDoc,
}

struct MigrationPlan {
    profiles: Vec<(
        ComputerProfile,
        SdkContextConfig,
        Vec<crate::commands::inputs::InputDefinition>,
    )>,
    manual_targets: GlobalManualTargetsConfig,
    manager_session: ManagerSessionConfig,
    sdk_configs: Vec<SdkMutation>,
}

#[derive(Debug, Default)]
struct MigrationProgress {
    sdk_mutations_attempted: usize,
}

pub fn migrate_legacy_config(
    config: &ConfigService,
    sdk_config: &SdkConfigService,
    settings: &SettingsService,
    secret_store: &dyn SecretStore,
) -> Result<MigrationOutcome, MigrationError> {
    if config.migration_state_path().exists() {
        let marker_content = fs::read(config.migration_state_path())?;
        let marker: MigrationState = serde_json::from_slice(&marker_content).map_err(|error| {
            MigrationError::Conflict(format!("migration marker is corrupted: {error}"))
        })?;
        if marker.schema_version != MIGRATION_VERSION {
            return Err(MigrationError::Conflict(format!(
                "migration marker schema {} is unsupported (expected {})",
                marker.schema_version, MIGRATION_VERSION
            )));
        }
        if !marker.cleanup_completed {
            finalize_legacy_cleanup(config, settings)?;
            write_migration_marker(config, true)?;
        }
        return Ok(MigrationOutcome::AlreadyCompleted);
    }

    let legacy_settings = settings.load_legacy_for_migration()?;
    let has_legacy_source = config.legacy_computer_instances_path().exists()
        || config.legacy_connection_targets_path().exists()
        || legacy_settings.manager_session.is_some();
    if !has_legacy_source {
        return Ok(MigrationOutcome::NotNeeded);
    }

    let plan = build_plan(
        config,
        sdk_config,
        settings,
        secret_store,
        legacy_settings.manager_session.as_ref(),
    )?;
    let file_snapshots = capture_destination_snapshots(config, settings, &plan)?;

    let mut progress = MigrationProgress::default();
    let migration_result = apply_plan(
        config,
        sdk_config,
        settings,
        secret_store,
        &plan,
        &mut progress,
    )
    .and_then(|()| verify_plan(config, sdk_config, settings, secret_store, &plan))
    .and_then(|()| write_migration_marker(config, false));
    if let Err(error) = migration_result {
        let rollback = rollback_plan(
            config,
            sdk_config,
            secret_store,
            &plan,
            &progress,
            &file_snapshots,
        );
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(MigrationError::Rollback {
                primary: error.to_string(),
                rollback: rollback_error,
            }),
        };
    }
    finalize_legacy_cleanup(config, settings)?;
    write_migration_marker(config, true)?;

    Ok(MigrationOutcome::Completed)
}

fn build_plan(
    config: &ConfigService,
    sdk_config: &SdkConfigService,
    settings: &SettingsService,
    secret_store: &dyn SecretStore,
    legacy_manager_session: Option<&crate::services::settings::ManagerSessionSettings>,
) -> Result<MigrationPlan, MigrationError> {
    let legacy_instances = config.load_legacy_computer_instances()?;
    let mut seen_instance_ids = HashSet::new();
    let existing_discovery = config.discover_computer_instances()?;
    if !existing_discovery.errors.is_empty() {
        return Err(MigrationError::Conflict(format!(
            "cannot migrate while {} destination Computer profile(s) are invalid",
            existing_discovery.errors.len()
        )));
    }
    let existing_profiles: HashMap<_, _> = existing_discovery
        .config
        .instances
        .iter()
        .map(|instance| {
            (
                instance.id.clone(),
                (ComputerProfile::from(instance), instance.inputs.clone()),
            )
        })
        .collect();

    let mut profiles = Vec::new();
    let mut sdk_configs = Vec::new();

    for instance in &legacy_instances.instances {
        if instance.id.trim().is_empty() || !seen_instance_ids.insert(instance.id.clone()) {
            return Err(MigrationError::Conflict(format!(
                "legacy Computer id '{}' is empty or duplicated",
                instance.id
            )));
        }
        config.computer_profile_path(&instance.id)?;
        let profile = ComputerProfile::from(instance);
        let context = SdkContextConfig {
            schema_version: SDK_CONTEXT_SCHEMA_VERSION,
            skill_home_override: instance.local_skills_root.clone(),
        };
        let inputs = if let Some((existing, inputs)) = existing_profiles.get(&instance.id) {
            if existing != &profile {
                return Err(MigrationError::Conflict(format!(
                    "destination profile '{}' differs from the legacy profile",
                    instance.id
                )));
            }
            if config.load_sdk_context(&instance.id)? != context {
                return Err(MigrationError::Conflict(format!(
                    "destination SDK context '{}' differs from the legacy context",
                    instance.id
                )));
            }
            inputs.clone()
        } else {
            Vec::new()
        };
        profiles.push((profile, context, inputs));

        let before = sdk_config
            .load_raw_project_config(&instance.id)
            .map_err(|error| MigrationError::Sdk {
                instance_id: instance.id.clone(),
                reason: error.to_string(),
            })?;
        let after = merge_legacy_user_mcp(&instance.id, before.clone(), &instance.mcp_servers)?;
        if !sdk_config.validate(&after).is_valid() {
            return Err(MigrationError::Conflict(format!(
                "legacy SDK-owned config for '{}' is not valid",
                instance.id
            )));
        }
        sdk_configs.push(SdkMutation {
            instance_id: instance.id.clone(),
            before,
            after,
        });
    }

    let mut targets: BTreeMap<String, GlobalManualSmcpTarget> = config
        .load_global_manual_targets()?
        .manual_smcp_targets
        .into_iter()
        .map(|target| (target.id.clone(), target))
        .collect();
    for target in config.load_legacy_connection_targets()?.manual_smcp_targets {
        let target = normalize_manual_smcp_target(target);
        let migrated = GlobalManualSmcpTarget::from(&target);
        match targets.get(&migrated.id) {
            Some(existing) if existing != &migrated => {
                return Err(MigrationError::Conflict(format!(
                    "manual target '{}' differs between legacy and global storage",
                    migrated.id
                )));
            }
            _ => {
                targets.insert(migrated.id.clone(), migrated);
            }
        }
    }
    let manual_targets = GlobalManualTargetsConfig {
        schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
        manual_smcp_targets: targets.into_values().collect(),
    };

    let mut manager_session = settings.load_global_manager_session()?;
    if let Some(legacy) = legacy_manager_session {
        let migrated = PersistedManagerSession::from(legacy);
        match manager_session.session.as_ref() {
            Some(existing) if existing != &migrated => {
                return Err(MigrationError::Conflict(
                    "Manager session differs between legacy and global storage".to_string(),
                ));
            }
            _ => {
                manager_session = ManagerSessionConfig {
                    schema_version: MANAGER_SESSION_SCHEMA_VERSION,
                    session: Some(migrated),
                }
            }
        }
    }

    // Manual target credentials already use stable target IDs. Reading them during
    // preflight verifies keychain access without copying secrets into JSON.
    for target in &manual_targets.manual_smcp_targets {
        let _ = secret_store.get_secret(&manual_target_keychain_id(&target.id))?;
    }

    Ok(MigrationPlan {
        profiles,
        manual_targets,
        manager_session,
        sdk_configs,
    })
}

fn merge_legacy_user_mcp(
    instance_id: &str,
    mut document: ProjectConfigDoc,
    servers: &[crate::services::computer::ManagedMcpServer],
) -> Result<ProjectConfigDoc, MigrationError> {
    let mcp = document.mcp.get_or_insert_with(Map::new);
    let servers_value = mcp
        .entry("servers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let server_map = servers_value.as_object_mut().ok_or_else(|| {
        MigrationError::Conflict(format!(
            "SDK MCP config for '{instance_id}' has a non-object servers field"
        ))
    })?;

    for server in servers {
        if server.is_plugin_owned() {
            continue;
        }
        let name = server.name().to_string();
        let normalized = normalize_mcp_input_references(server.config.clone())
            .map_err(MigrationError::Conflict)?;
        let mut value = serde_json::to_value(normalized)
            .map_err(|error| MigrationError::Conflict(error.to_string()))?;
        let body = value.as_object_mut().ok_or_else(|| {
            MigrationError::Conflict(format!("legacy MCP server '{name}' is not a JSON object"))
        })?;
        body.remove("name");
        match server_map.get(&name) {
            Some(existing) if existing != &value => {
                return Err(MigrationError::Conflict(format!(
                    "SDK MCP server '{name}' for '{instance_id}' differs from existing config"
                )));
            }
            _ => {
                server_map.insert(name, value);
            }
        }
    }
    Ok(document)
}

fn capture_destination_snapshots(
    config: &ConfigService,
    settings: &SettingsService,
    plan: &MigrationPlan,
) -> Result<Vec<FileSnapshot>, MigrationError> {
    let mut paths = vec![
        config.migration_state_path(),
        config.global_manual_targets_path(),
        settings.global_manager_session_path(),
    ];
    for (profile, _, _) in &plan.profiles {
        paths.push(config.computer_profile_path(&profile.id)?);
        paths.push(config.sdk_context_path(&profile.id)?);
        paths.push(config.computer_inputs_path(&profile.id)?);
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(FileSnapshot::capture)
        .collect::<Result<Vec<_>, _>>()
        .map_err(MigrationError::from)
}

fn apply_plan(
    config: &ConfigService,
    sdk_config: &SdkConfigService,
    settings: &SettingsService,
    _secret_store: &dyn SecretStore,
    plan: &MigrationPlan,
    progress: &mut MigrationProgress,
) -> Result<(), MigrationError> {
    for (profile, context, inputs) in &plan.profiles {
        config.save_computer_directory(profile, context, inputs)?;
    }
    config.save_global_manual_targets(&plan.manual_targets)?;
    settings.save_global_manager_session(&plan.manager_session)?;
    for sdk in &plan.sdk_configs {
        progress.sdk_mutations_attempted += 1;
        sdk_config
            .save(&sdk.instance_id, &sdk.after)
            .map_err(|error| MigrationError::Sdk {
                instance_id: sdk.instance_id.clone(),
                reason: error.to_string(),
            })?;
    }
    Ok(())
}

fn verify_plan(
    config: &ConfigService,
    sdk_config: &SdkConfigService,
    settings: &SettingsService,
    secret_store: &dyn SecretStore,
    plan: &MigrationPlan,
) -> Result<(), MigrationError> {
    if config.load_global_manual_targets()? != plan.manual_targets {
        return Err(MigrationError::Verification(
            "manual targets do not match the migration plan".to_string(),
        ));
    }
    if settings.load_global_manager_session()? != plan.manager_session {
        return Err(MigrationError::Verification(
            "Manager session does not match the migration plan".to_string(),
        ));
    }
    for (profile, context, inputs) in &plan.profiles {
        if config.load_computer_profile(&profile.id)? != *profile
            || config.load_sdk_context(&profile.id)? != *context
            || config.load_inputs_for_instance(&profile.id)? != *inputs
        {
            return Err(MigrationError::Verification(format!(
                "Computer '{}' destination data does not match the migration plan",
                profile.id
            )));
        }
    }
    for target in &plan.manual_targets.manual_smcp_targets {
        let _ = secret_store.get_secret(&manual_target_keychain_id(&target.id))?;
    }
    for sdk in &plan.sdk_configs {
        if sdk_config
            .load_raw_project_config(&sdk.instance_id)
            .map_err(|error| MigrationError::Sdk {
                instance_id: sdk.instance_id.clone(),
                reason: error.to_string(),
            })?
            != sdk.after
        {
            return Err(MigrationError::Verification(format!(
                "SDK config for '{}' does not match the migration plan",
                sdk.instance_id
            )));
        }
    }
    Ok(())
}

fn rollback_plan(
    config: &ConfigService,
    sdk_config: &SdkConfigService,
    _secret_store: &dyn SecretStore,
    plan: &MigrationPlan,
    progress: &MigrationProgress,
    snapshots: &[FileSnapshot],
) -> Result<(), String> {
    let mut errors = Vec::new();
    let computer_snapshot_paths = plan
        .profiles
        .iter()
        .flat_map(|(profile, _, _)| {
            [
                config.computer_profile_path(&profile.id),
                config.sdk_context_path(&profile.id),
                config.computer_inputs_path(&profile.id),
            ]
        })
        .filter_map(Result::ok)
        .collect::<HashSet<_>>();
    let mut computer_snapshots_restored = true;
    for sdk in plan
        .sdk_configs
        .iter()
        .take(progress.sdk_mutations_attempted)
        .rev()
    {
        if let Err(error) = sdk_config.restore_raw_project_config(&sdk.instance_id, &sdk.before) {
            errors.push(format!("restore SDK config '{}': {error}", sdk.instance_id));
        }
    }
    for snapshot in snapshots.iter().rev() {
        if let Err(error) = snapshot.restore() {
            if computer_snapshot_paths.contains(&snapshot.path) {
                computer_snapshots_restored = false;
            }
            errors.push(format!("restore '{}': {error}", snapshot.path.display()));
        }
    }
    if computer_snapshots_restored {
        if let Err(error) = config.discard_computer_directory_transactions() {
            errors.push(format!("discard Computer directory transactions: {error}"));
        }
    }
    for (profile, _, _) in &plan.profiles {
        let profile_path = match config.computer_profile_path(&profile.id) {
            Ok(path) => path,
            Err(error) => {
                errors.push(format!(
                    "resolve created Computer directory '{}': {error}",
                    profile.id
                ));
                continue;
            }
        };
        let was_created = snapshots
            .iter()
            .find(|snapshot| snapshot.path == profile_path)
            .is_some_and(|snapshot| snapshot.content.is_none());
        if was_created {
            match config.computer_instance_root(&profile.id) {
                Ok(path) => match fs::remove_dir_all(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => errors.push(format!(
                        "remove created Computer directory '{}': {error}",
                        path.display()
                    )),
                },
                Err(error) => errors.push(format!(
                    "resolve created Computer directory '{}': {error}",
                    profile.id
                )),
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn write_migration_marker(
    config: &ConfigService,
    cleanup_completed: bool,
) -> Result<(), MigrationError> {
    write_json_atomically(
        &config.migration_state_path(),
        &MigrationState {
            schema_version: MIGRATION_VERSION,
            completed_at: chrono::Utc::now().to_rfc3339(),
            cleanup_completed,
        },
    )?;
    Ok(())
}

fn finalize_legacy_cleanup(
    config: &ConfigService,
    settings: &SettingsService,
) -> Result<(), MigrationError> {
    archive_legacy_file(config.legacy_computer_instances_path())?;
    archive_legacy_file(config.legacy_connection_targets_path())?;
    let mut legacy_settings = settings.load_legacy_for_migration()?;
    if legacy_settings.manager_session.take().is_some() {
        settings.save(&legacy_settings)?;
    }
    Ok(())
}

fn archive_legacy_file(path: &Path) -> Result<(), MigrationError> {
    if !path.exists() {
        return Ok(());
    }
    let archive = path.with_extension(format!(
        "{}.migrated-v{MIGRATION_VERSION}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("legacy")
    ));
    if archive.exists() {
        return Err(MigrationError::Conflict(format!(
            "cannot archive legacy config {} because {} already exists",
            path.display(),
            archive.display()
        )));
    }
    fs::rename(path, &archive)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::inputs::InputDefinition;
    use crate::services::computer::{ComputerInstance, ComputerInstancesConfig};
    use crate::services::config::DirectoryRenameTestAction;
    use crate::services::keychain::{self, InMemorySecretStore};
    use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
    use tempfile::tempdir;

    #[derive(Default)]
    struct FailingVerificationReadSecretStore {
        reads: std::sync::atomic::AtomicUsize,
    }

    impl SecretStore for FailingVerificationReadSecretStore {
        fn set_secret(&self, _key: &str, _secret: &str) -> Result<(), KeychainError> {
            Ok(())
        }

        fn get_secret(&self, _key: &str) -> Result<Option<String>, KeychainError> {
            if self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
                return Err(KeychainError::Store(
                    "injected verification read failure".to_string(),
                ));
            }
            Ok(None)
        }

        fn delete_secret(&self, _key: &str) -> Result<(), KeychainError> {
            Ok(())
        }
    }

    fn install_verification_probe_target(config: &ConfigService) {
        config
            .save_global_manual_targets(&GlobalManualTargetsConfig {
                schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
                manual_smcp_targets: vec![GlobalManualSmcpTarget {
                    id: "verification-probe".to_string(),
                    name: "Verification probe".to_string(),
                    url: "wss://example.test/smcp".to_string(),
                    namespace: "/smcp".to_string(),
                    office_id: "verification-probe".to_string(),
                    routing_headers: HashMap::new(),
                }],
            })
            .unwrap();
    }

    #[test]
    fn legacy_inputs_and_values_are_not_migrated() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let legacy = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![ComputerInstance {
                inputs: vec![InputDefinition::PromptString {
                    id: "token".to_string(),
                    label: "Token".to_string(),
                    description: None,
                    default: Some("legacy-default".to_string()),
                    password: Some(true),
                }],
                input_values: HashMap::from([(
                    "token".to_string(),
                    Value::String("legacy-value".to_string()),
                )]),
                ..ComputerInstance::new("one", "One")
            }],
        };
        write_json_atomically(config.legacy_computer_instances_path(), &legacy).unwrap();
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = InMemorySecretStore::default();

        assert_eq!(
            migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap(),
            MigrationOutcome::Completed
        );

        assert!(config.load_inputs_for_instance("one").unwrap().is_empty());
        assert_eq!(
            keychain::get_input_value(&secrets, "one", "token").unwrap(),
            None
        );
    }

    #[test]
    fn incomplete_cleanup_marker_resumes_legacy_archival() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        write_json_atomically(
            config.legacy_computer_instances_path(),
            &ComputerInstancesConfig::default(),
        )
        .unwrap();
        write_json_atomically(
            &config.migration_state_path(),
            &MigrationState {
                schema_version: MIGRATION_VERSION,
                completed_at: chrono::Utc::now().to_rfc3339(),
                cleanup_completed: false,
            },
        )
        .unwrap();
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = InMemorySecretStore::default();

        assert_eq!(
            migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap(),
            MigrationOutcome::AlreadyCompleted
        );

        assert!(!config.legacy_computer_instances_path().exists());
        assert!(config
            .legacy_computer_instances_path()
            .with_extension("json.migrated-v1")
            .exists());
        let marker: MigrationState =
            serde_json::from_slice(&fs::read(config.migration_state_path()).unwrap()).unwrap();
        assert!(marker.cleanup_completed);
    }

    #[test]
    fn migration_rollback_retains_directory_transaction_when_snapshot_restore_fails() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let instance = ComputerInstance::new("one", "Original");
        config.add_computer_instance(instance.clone()).unwrap();
        write_json_atomically(
            config.legacy_computer_instances_path(),
            &ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![instance],
            },
        )
        .unwrap();
        config.inject_directory_rename_actions([
            DirectoryRenameTestAction::Proceed,
            DirectoryRenameTestAction::FailAfterCreatingDestinationFile,
        ]);
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = InMemorySecretStore::default();

        let error = migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap_err();

        assert!(matches!(error, MigrationError::Rollback { .. }));
        let instance_root = config.computer_instance_root("one").unwrap();
        assert!(instance_root.is_file());
        let transactions_root = config.client_computers_root().join(".transactions");
        let transaction_roots = fs::read_dir(&transactions_root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(transaction_roots.len(), 1);
        let retained_profile: ComputerProfile = serde_json::from_slice(
            &fs::read(
                transaction_roots[0]
                    .join("old")
                    .join(crate::services::client_computers::COMPUTER_PROFILE_FILE_NAME),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(retained_profile.name, "Original");

        fs::remove_file(&instance_root).unwrap();
        let discovered = config.discover_computer_instances().unwrap();
        assert_eq!(discovered.config.instances[0].name, "Original");
        assert!(!transactions_root.exists());
    }

    #[test]
    fn existing_sdk_context_conflict_fails_before_overwriting_destination() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let legacy_instance = ComputerInstance {
            local_skills_root: Some(PathBuf::from("/legacy/skills")),
            ..ComputerInstance::new("one", "One")
        };
        write_json_atomically(
            config.legacy_computer_instances_path(),
            &ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![legacy_instance.clone()],
            },
        )
        .unwrap();
        config
            .save_computer_profile(&ComputerProfile::from(&legacy_instance))
            .unwrap();
        let existing_context = SdkContextConfig {
            schema_version: SDK_CONTEXT_SCHEMA_VERSION,
            skill_home_override: Some(PathBuf::from("/new/skills")),
        };
        config.save_sdk_context("one", &existing_context).unwrap();
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = InMemorySecretStore::default();

        let error = migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap_err();

        assert!(error.to_string().contains("destination SDK context"));
        assert_eq!(config.load_sdk_context("one").unwrap(), existing_context);
        assert!(!config.migration_state_path().exists());
    }

    #[test]
    fn sdk_restore_failure_is_reported_and_keeps_legacy_source_for_recovery() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let legacy_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "legacy-server",
            "server_parameters": {"command": "node", "args": [], "env": {}}
        }))
        .unwrap();
        let legacy_instance = ComputerInstance {
            mcp_servers: vec![legacy_server.into()],
            input_values: HashMap::from([(
                "token".to_string(),
                Value::String("secret".to_string()),
            )]),
            ..ComputerInstance::new("one", "One")
        };
        write_json_atomically(
            config.legacy_computer_instances_path(),
            &ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![legacy_instance],
            },
        )
        .unwrap();
        let existing_input = InputDefinition::PromptString {
            id: "current-token".to_string(),
            label: "Current token".to_string(),
            description: None,
            default: None,
            password: Some(true),
        };
        config
            .add_computer_instance(ComputerInstance {
                inputs: vec![existing_input.clone()],
                ..ComputerInstance::new("one", "One")
            })
            .unwrap();
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        sdk.save(
            "one",
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "existing": {
                                "type": "stdio",
                                "server_parameters": {"command": "existing"}
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..ProjectConfigDoc::default()
            },
        )
        .unwrap();
        sdk.inject_raw_restore_failure();
        install_verification_probe_target(&config);
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = FailingVerificationReadSecretStore::default();

        let error = migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap_err();

        assert!(matches!(error, MigrationError::Rollback { .. }));
        assert!(error
            .to_string()
            .contains("injected raw SDK restore failure"));
        assert_eq!(
            config.load_inputs_for_instance("one").unwrap(),
            vec![existing_input]
        );
        assert!(config.legacy_computer_instances_path().exists());
        assert!(!config.migration_state_path().exists());
    }

    #[test]
    fn sdk_restore_install_failure_preserves_existing_sdk_config() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let legacy_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "legacy-server",
            "server_parameters": {"command": "node", "args": [], "env": {}}
        }))
        .unwrap();
        let legacy_instance = ComputerInstance {
            mcp_servers: vec![legacy_server.into()],
            input_values: HashMap::from([(
                "token".to_string(),
                Value::String("secret".to_string()),
            )]),
            ..ComputerInstance::new("one", "One")
        };
        write_json_atomically(
            config.legacy_computer_instances_path(),
            &ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![legacy_instance],
            },
        )
        .unwrap();
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        let before = ProjectConfigDoc {
            settings: Some(
                serde_json::json!({"custom": "preserve-me"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            mcp: Some(
                serde_json::json!({
                    "servers": {
                        "existing": {
                            "type": "stdio",
                            "server_parameters": {"command": "existing"}
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..ProjectConfigDoc::default()
        };
        sdk.save("one", &before).unwrap();
        sdk.inject_raw_restore_failure_after_backup();
        install_verification_probe_target(&config);
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = FailingVerificationReadSecretStore::default();

        let error = migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap_err();

        assert!(matches!(error, MigrationError::Rollback { .. }));
        let recovered = sdk.load_raw_project_config("one").unwrap();
        assert_eq!(recovered.settings, before.settings);
        assert_eq!(
            recovered.mcp.as_ref().unwrap()["servers"]["existing"],
            before.mcp.as_ref().unwrap()["servers"]["existing"]
        );
        assert!(config.legacy_computer_instances_path().exists());
        assert!(!config.migration_state_path().exists());
    }

    #[test]
    fn migration_preserves_raw_sdk_secrets_and_local_layers() {
        let directory = tempdir().unwrap();
        let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
        let legacy_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "legacy-server",
            "server_parameters": {"command": "node", "args": [], "env": {}}
        }))
        .unwrap();
        let legacy_instance = ComputerInstance {
            mcp_servers: vec![legacy_server.into()],
            ..ComputerInstance::new("one", "One")
        };
        write_json_atomically(
            config.legacy_computer_instances_path(),
            &ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![legacy_instance],
            },
        )
        .unwrap();
        let config = std::sync::Arc::new(config);
        let sdk = SdkConfigService::new(config.clone());
        let raw_before = ProjectConfigDoc {
            mcp: Some(
                serde_json::json!({
                    "servers": {
                        "existing": {
                            "type": "stdio",
                            "server_parameters": {
                                "command": "existing",
                                "env": {"TOKEN": "literal-secret"}
                            }
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            mcp_local: Some(
                serde_json::json!({
                    "servers": {
                        "local-only": {
                            "type": "stdio",
                            "server_parameters": {"command": "local"}
                        }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..ProjectConfigDoc::default()
        };
        sdk.save("one", &raw_before).unwrap();
        let settings = SettingsService::new(directory.path().to_path_buf());
        let secrets = InMemorySecretStore::default();

        migrate_legacy_config(&config, &sdk, &settings, &secrets).unwrap();

        let raw_after = sdk.load_raw_project_config("one").unwrap();
        assert_eq!(raw_after.mcp_local, raw_before.mcp_local);
        assert_eq!(
            raw_after.mcp.as_ref().unwrap()["servers"]["existing"]["server_parameters"]["env"]
                ["TOKEN"],
            "literal-secret"
        );
        assert!(raw_after.mcp.unwrap()["servers"]["legacy-server"].is_object());
    }
}
