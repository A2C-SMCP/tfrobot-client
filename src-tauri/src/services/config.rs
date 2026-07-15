use crate::commands::inputs::InputDefinition;
use crate::services::client_computers::{
    ClientComputersPathError, ClientComputersPaths, GlobalConfigFile,
};
use crate::services::computer::{
    ComputerInstance, ComputerInstancesConfig, ComputerProfile, GlobalInputDefinition,
    GlobalInputsConfig, ManagedMcpServer, COMPUTER_PROFILE_SCHEMA_VERSION,
    GLOBAL_INPUTS_SCHEMA_VERSION,
};
use crate::services::connection_targets::{
    ConnectionTargetsConfig, GlobalManualTargetsConfig, ManualSmcpTarget,
    MANUAL_TARGETS_SCHEMA_VERSION,
};
use crate::services::storage::{write_json_atomically, AtomicJsonWriteError};
#[cfg(test)]
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use reqwest::header::{HeaderName, HeaderValue};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Service for persisting all configuration data to disk
pub struct ConfigService {
    config_dir: PathBuf,
    computer_instances_file: PathBuf,
    connection_targets_file: PathBuf,
    client_computers_paths: ClientComputersPaths,
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

    pub fn load_global_inputs(&self) -> Result<GlobalInputsConfig, ConfigError> {
        let path = self.global_config_path(GlobalConfigFile::Inputs);
        let config: GlobalInputsConfig = load_new_artifact_or_default(&path)?;
        validate_global_inputs_config(&config)?;
        Ok(config)
    }

    pub fn save_global_inputs(&self, config: &GlobalInputsConfig) -> Result<(), ConfigError> {
        validate_global_inputs_config(config)?;
        save_json_file(&self.global_config_path(GlobalConfigFile::Inputs), config)
    }

    pub fn load_global_manual_targets(&self) -> Result<GlobalManualTargetsConfig, ConfigError> {
        let path = self.global_config_path(GlobalConfigFile::ManualTargets);
        let config: GlobalManualTargetsConfig = load_new_artifact_or_default(&path)?;
        validate_global_manual_targets_config(&config)?;
        Ok(config)
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
        let instances = self.load_computer_instances()?;
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
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.inputs.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    pub fn save_inputs_for_instance(
        &self,
        instance_id: &str,
        inputs: &[InputDefinition],
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(instance_id, |instance| {
            instance.inputs = inputs.to_vec();
        })
    }

    // --- Input Values ---

    pub fn load_input_values_for_instance(
        &self,
        instance_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .map(|instance| instance.input_values.clone())
            .ok_or_else(|| ConfigError::NotFound(instance_id.to_string()))
    }

    pub fn save_input_values_for_instance(
        &self,
        instance_id: &str,
        values: &HashMap<String, serde_json::Value>,
    ) -> Result<ComputerInstance, ConfigError> {
        self.update_computer_instance(instance_id, |instance| {
            instance.input_values = values.clone();
        })
    }

    // --- Computer Instances ---

    pub fn load_computer_instances(&self) -> Result<ComputerInstancesConfig, ConfigError> {
        let mut config: ComputerInstancesConfig = load_json_file(&self.computer_instances_file)?;
        config.normalize();
        Ok(config)
    }

    pub fn save_computer_instances(
        &self,
        instances: &ComputerInstancesConfig,
    ) -> Result<(), ConfigError> {
        let mut instances = instances.clone();
        instances.normalize();
        save_json_file(&self.computer_instances_file, &instances)
    }

    pub fn get_computer_instance(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let instances = self.load_computer_instances()?;
        instances
            .instances
            .into_iter()
            .find(|instance| instance.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))
    }

    pub fn add_computer_instance(&self, instance: ComputerInstance) -> Result<(), ConfigError> {
        let mut instances = self.load_computer_instances()?;
        if instances
            .instances
            .iter()
            .any(|existing| existing.id == instance.id)
        {
            return Err(ConfigError::AlreadyExists(instance.id));
        }
        instances.instances.push(instance);
        self.save_computer_instances(&instances)
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
        let mut instances = self.load_computer_instances()?;
        let instance = instances
            .instances
            .iter_mut()
            .find(|instance| instance.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        update(instance);
        let updated = instance.clone();
        self.save_computer_instances(&instances)?;
        Ok(updated)
    }

    pub fn remove_computer_instance(&self, id: &str) -> Result<ComputerInstance, ConfigError> {
        let mut instances = self.load_computer_instances()?;
        let index = instances
            .instances
            .iter()
            .position(|instance| instance.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        let removed = instances.instances.remove(index);
        self.save_computer_instances(&instances)?;
        Ok(removed)
    }

    // --- Connection Targets ---

    pub fn load_connection_targets(&self) -> Result<ConnectionTargetsConfig, ConfigError> {
        load_json_file(&self.connection_targets_file)
    }

    pub fn save_connection_targets(
        &self,
        targets: &ConnectionTargetsConfig,
    ) -> Result<(), ConfigError> {
        save_json_file(&self.connection_targets_file, targets)
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

fn validate_global_inputs_config(config: &GlobalInputsConfig) -> Result<(), ConfigError> {
    validate_schema_version(
        "global inputs",
        config.schema_version,
        GLOBAL_INPUTS_SCHEMA_VERSION,
    )?;
    validate_unique_artifact_ids("global input", config.inputs.iter().map(|input| input.id()))?;

    for input in &config.inputs {
        if let GlobalInputDefinition::PromptString {
            id,
            default: Some(default),
            password: Some(true),
            ..
        } = input
        {
            if !default.is_empty() {
                return Err(ConfigError::SecretPlaintextInGlobalInput {
                    input_id: id.clone(),
                });
            }
        }
    }
    Ok(())
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

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

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

    #[error("global input '{input_id}' contains a password default; store it in keychain")]
    SecretPlaintextInGlobalInput { input_id: String },

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
        let inputs = GlobalInputsConfig {
            schema_version: GLOBAL_INPUTS_SCHEMA_VERSION + 1,
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
            svc.save_global_inputs(&inputs).unwrap_err(),
            ConfigError::UnsupportedSchemaVersion {
                artifact: "global inputs",
                ..
            }
        ));
    }

    #[test]
    fn global_inputs_and_manual_targets_are_stored_separately() {
        let (svc, tmp) = setup_empty();
        let inputs = GlobalInputsConfig {
            schema_version: GLOBAL_INPUTS_SCHEMA_VERSION,
            inputs: vec![GlobalInputDefinition::PromptString {
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

        svc.save_global_inputs(&inputs).unwrap();
        svc.save_global_manual_targets(&targets).unwrap();

        let loaded_inputs = svc.load_global_inputs().unwrap();
        let loaded_targets = svc.load_global_manual_targets().unwrap();
        assert_eq!(loaded_inputs.inputs[0].id(), "region");
        assert_eq!(loaded_targets.manual_smcp_targets[0].id, "target-a");
        assert!(tmp
            .path()
            .join("client_computers/global/inputs.json")
            .exists());
        assert!(tmp
            .path()
            .join("client_computers/global/manual_targets.json")
            .exists());
        assert!(
            !std::fs::read_to_string(tmp.path().join("client_computers/global/inputs.json"))
                .unwrap()
                .contains("input_values")
        );
    }

    #[test]
    fn global_inputs_reject_password_default_plaintext() {
        let (svc, _tmp) = setup_empty();
        let inputs = GlobalInputsConfig {
            schema_version: GLOBAL_INPUTS_SCHEMA_VERSION,
            inputs: vec![GlobalInputDefinition::PromptString {
                id: "api-key".to_string(),
                label: "API key".to_string(),
                description: None,
                default: Some("plaintext-secret".to_string()),
                password: Some(true),
            }],
        };

        assert!(matches!(
            svc.save_global_inputs(&inputs).unwrap_err(),
            ConfigError::SecretPlaintextInGlobalInput { input_id } if input_id == "api-key"
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
        let prompt = |id: &str| GlobalInputDefinition::PromptString {
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

        let invalid_inputs = GlobalInputsConfig {
            schema_version: GLOBAL_INPUTS_SCHEMA_VERSION,
            inputs: vec![prompt(" ")],
        };
        assert!(matches!(
            svc.save_global_inputs(&invalid_inputs).unwrap_err(),
            ConfigError::InvalidArtifactEntityId {
                artifact: "global input",
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
        let inputs_path = svc.global_config_path(GlobalConfigFile::Inputs);
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
            svc.load_global_inputs().unwrap_err(),
            ConfigError::DuplicateArtifactEntityId {
                artifact: "global input",
                id
            } if id == "duplicate"
        ));

        std::fs::write(
            svc.global_config_path(GlobalConfigFile::ManualTargets),
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
    fn missing_global_artifacts_return_current_version_defaults() {
        let (svc, _tmp) = setup_empty();

        let inputs = svc.load_global_inputs().unwrap();
        let targets = svc.load_global_manual_targets().unwrap();

        assert_eq!(inputs.schema_version, GLOBAL_INPUTS_SCHEMA_VERSION);
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

        let inputs_path = svc.global_config_path(GlobalConfigFile::Inputs);
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
            svc.load_global_inputs().unwrap_err(),
            ConfigError::Json(_)
        ));

        let targets_path = svc.global_config_path(GlobalConfigFile::ManualTargets);
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
        for artifact in [GlobalConfigFile::Inputs, GlobalConfigFile::ManualTargets] {
            let path = svc.global_config_path(artifact);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }

        assert!(matches!(
            svc.load_global_inputs().unwrap_err(),
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
        let inputs_path = svc.global_config_path(GlobalConfigFile::Inputs);
        std::fs::create_dir_all(inputs_path.parent().unwrap()).unwrap();
        std::fs::write(inputs_path, r#"{"schema_version": 2, "inputs": []}"#).unwrap();
        std::fs::write(
            svc.global_config_path(GlobalConfigFile::ManualTargets),
            r#"{"schema_version": 2, "manual_smcp_targets": []}"#,
        )
        .unwrap();

        assert!(matches!(
            svc.load_global_inputs().unwrap_err(),
            ConfigError::UnsupportedSchemaVersion {
                artifact: "global inputs",
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
        svc.add_computer_instance(ComputerInstance::new("legacy", "Legacy"))
            .unwrap();
        svc.save_computer_profile(&ComputerProfile::new("new-profile", "New"))
            .unwrap();

        let legacy = svc.load_computer_instances().unwrap();
        assert_eq!(legacy.instances.len(), 1);
        assert_eq!(legacy.instances[0].id, "legacy");
        assert!(tmp.path().join("computer_instances.json").exists());
    }

    // --- Legacy MCP migration ---

    #[test]
    fn legacy_mcp_migration_read_is_empty_for_new_profiles() {
        let (svc, _tmp) = setup();
        let configs = svc
            .load_legacy_mcp_configs_for_migration(TEST_INSTANCE_ID)
            .unwrap();
        assert!(configs.is_empty());
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

        svc.save_computer_instances(&instances).unwrap();
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

    // --- Input Values ---

    #[test]
    fn test_load_empty_input_values() {
        let (svc, _tmp) = setup();
        let values = svc
            .load_input_values_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert!(values.is_empty());
    }

    #[test]
    fn test_save_and_load_input_values_roundtrip() {
        let (svc, _tmp) = setup();
        let mut values = HashMap::new();
        values.insert("key1".to_string(), serde_json::json!("value1"));
        values.insert("key2".to_string(), serde_json::json!(42));

        svc.save_input_values_for_instance(TEST_INSTANCE_ID, &values)
            .unwrap();
        let loaded = svc
            .load_input_values_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["key1"], serde_json::json!("value1"));
        assert_eq!(loaded["key2"], serde_json::json!(42));
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

        let targets = svc.list_manual_smcp_targets().unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "legacy-target");
        assert_eq!(targets[0].office_id, "office-1");
    }
}
