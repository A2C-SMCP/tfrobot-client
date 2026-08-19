#[allow(dead_code)]
mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tfrobot_client_lib::commands::inputs::{list_inputs_core, InputDefinition};
use tfrobot_client_lib::services::computer::{
    ComputerInstance, ComputerInstancesConfig, ManagedMcpServer,
};
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::config_migration::{migrate_legacy_config, MigrationOutcome};
use tfrobot_client_lib::services::connection_targets::{
    manual_target_keychain_id, ConnectionTargetsConfig, ManualSmcpTarget,
};
use tfrobot_client_lib::services::input_value_store::InputValueStore;
use tfrobot_client_lib::services::keychain::{self, InMemorySecretStore, SecretStore};
use tfrobot_client_lib::services::observability::ObservabilityService;
use tfrobot_client_lib::services::settings::{
    AppSettings, ManagerSessionSettings, SettingsService,
};
use tfrobot_client_lib::AppState;

fn legacy_global_input_key(namespace: &str, input_id: &str) -> String {
    let digest = Sha256::digest(input_id.as_bytes());
    format!("{namespace}:{}", hex::encode(&digest[..16]))
}

#[derive(Default)]
struct RecordingSecretStore {
    inner: InMemorySecretStore,
    reads: Mutex<Vec<String>>,
}

impl RecordingSecretStore {
    fn read_keys(&self) -> Vec<String> {
        self.reads.lock().unwrap().clone()
    }
}

impl SecretStore for RecordingSecretStore {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), keychain::KeychainError> {
        self.inner.set_secret(key, secret)
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, keychain::KeychainError> {
        self.reads.lock().unwrap().push(key.to_string());
        self.inner.get_secret(key)
    }

    fn delete_secret(&self, key: &str) -> Result<(), keychain::KeychainError> {
        self.inner.delete_secret(key)
    }
}

#[tokio::test]
async fn startup_atomically_migrates_legacy_registry_into_owned_destinations() {
    let directory = TempDir::new().unwrap();
    let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
    let settings = SettingsService::new(directory.path().to_path_buf());
    let expected_skill_home = directory.path().join("custom-skills");
    let mut legacy_instance = ComputerInstance::new("computer-a", "Computer A");
    legacy_instance.local_skills_root = Some(expected_skill_home.clone());
    legacy_instance.inputs = vec![InputDefinition::PromptString {
        id: "api-token".to_string(),
        label: Some("API token".to_string()),
        description: None,
        default: None,
        password: Some(true),
    }];
    legacy_instance.input_values.insert(
        "api-token".to_string(),
        serde_json::json!("password-default"),
    );
    let mut legacy_registry = serde_json::to_value(ComputerInstancesConfig {
        schema_version: 1,
        instances: vec![legacy_instance],
    })
    .unwrap();
    legacy_registry["instances"][0]["mcp_servers"] =
        serde_json::to_value(vec![ManagedMcpServer::user(common::echo_server_config(
            "legacy-echo",
        ))])
        .unwrap();
    std::fs::write(
        config.legacy_computer_instances_path(),
        serde_json::to_vec_pretty(&legacy_registry).unwrap(),
    )
    .unwrap();
    std::fs::write(
        config.legacy_connection_targets_path(),
        serde_json::to_vec_pretty(&ConnectionTargetsConfig {
            schema_version: 1,
            manual_smcp_targets: vec![ManualSmcpTarget {
                id: "office-a".to_string(),
                name: "Office A".to_string(),
                url: "wss://example.test/smcp".to_string(),
                namespace: "/smcp".to_string(),
                office_id: "office-a".to_string(),
                headers: HashMap::from([("x-routing".to_string(), "primary".to_string())]),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let legacy_settings = AppSettings {
        manager_session: Some(ManagerSessionSettings {
            base_url: "https://api-staging.turingfocus.cn".to_string(),
            user_id: 7,
            account_id: 8,
            account_name: "Robot Account".to_string(),
        }),
        ..AppSettings::default()
    };
    let mut legacy_settings_json = serde_json::to_value(&legacy_settings).unwrap();
    legacy_settings_json["manager_session"] =
        serde_json::to_value(legacy_settings.manager_session.as_ref().unwrap()).unwrap();
    std::fs::write(
        settings.legacy_settings_path(),
        serde_json::to_vec_pretty(&legacy_settings_json).unwrap(),
    )
    .unwrap();

    let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
    secrets
        .set_secret(
            &manual_target_keychain_id("office-a"),
            "manual-target-api-key",
        )
        .unwrap();
    let state = AppState::try_new_with_secret_store(
        config,
        ObservabilityService::new(directory.path()).unwrap(),
        settings,
        secrets.clone(),
    )
    .unwrap();

    let discovered = state.config.load_computer_instances().unwrap();
    assert_eq!(discovered.instances.len(), 1);
    assert_eq!(discovered.instances[0].id, "computer-a");
    assert_eq!(
        discovered.instances[0].local_skills_root.as_deref(),
        Some(expected_skill_home.as_path())
    );
    assert!(state
        .config
        .computer_profile_path("computer-a")
        .unwrap()
        .exists());
    assert!(state
        .config
        .sdk_context_path("computer-a")
        .unwrap()
        .exists());
    assert!(state.config.migration_state_path().exists());
    assert!(!state.config.legacy_computer_instances_path().exists());
    assert!(!state.config.legacy_connection_targets_path().exists());

    assert!(state
        .sdk_config
        .load_project_input_definitions("computer-a")
        .unwrap()
        .is_empty());
    assert_eq!(
        InputValueStore::for_computer(state.config.as_ref(), "computer-a")
            .get("api-token")
            .unwrap(),
        None
    );

    let targets = state.config.load_global_manual_targets().unwrap();
    assert_eq!(targets.manual_smcp_targets.len(), 1);
    assert_eq!(
        targets.manual_smcp_targets[0]
            .routing_headers
            .get("x-routing")
            .map(String::as_str),
        Some("primary")
    );
    assert_eq!(
        secrets
            .get_secret(&manual_target_keychain_id("office-a"))
            .unwrap()
            .as_deref(),
        Some("manual-target-api-key")
    );
    let persisted_manual_targets =
        std::fs::read_to_string(state.config.global_manual_targets_path()).unwrap();
    assert!(persisted_manual_targets.contains("x-routing"));
    assert!(persisted_manual_targets.contains("primary"));
    assert!(!persisted_manual_targets.contains("manual-target-api-key"));
    let manager = state
        .settings_service
        .load_global_manager_session()
        .unwrap();
    assert!(manager.session.is_none());

    let sdk_document = state.sdk_config.export("computer-a").unwrap();
    assert!(sdk_document.mcp.unwrap()["servers"]["legacy-echo"].is_object());

    let hydrated_runtime = state.computer_registry.runtime("computer-a").await;
    assert!(hydrated_runtime.is_some());
    assert_eq!(
        migrate_legacy_config(
            state.config.as_ref(),
            state.sdk_config.as_ref(),
            state.settings_service.as_ref(),
            secrets.as_ref(),
        )
        .unwrap(),
        MigrationOutcome::AlreadyCompleted
    );
}

#[tokio::test]
async fn startup_does_not_import_legacy_global_inputs_or_read_unscoped_keychain_namespaces() {
    let directory = TempDir::new().unwrap();
    let config = ConfigService::new(directory.path().to_path_buf()).unwrap();
    config
        .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
        .unwrap();

    let legacy_global_inputs_path = directory.path().join("client_computers/global/inputs.json");
    std::fs::create_dir_all(legacy_global_inputs_path.parent().unwrap()).unwrap();
    let legacy_global_inputs = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": 1,
        "inputs": [{
            "type": "PromptString",
            "id": "shared-token",
            "label": "Legacy shared token",
            "default": null,
            "password": true
        }]
    }))
    .unwrap();
    std::fs::write(&legacy_global_inputs_path, &legacy_global_inputs).unwrap();

    let recording_secrets = Arc::new(RecordingSecretStore::default());
    let secrets: Arc<dyn SecretStore> = recording_secrets.clone();
    let legacy_value_key = legacy_global_input_key("input-value", "shared-token");
    let legacy_secret_key = legacy_global_input_key("input-secret", "shared-token");
    secrets
        .set_secret(
            &legacy_value_key,
            &serde_json::to_string(&serde_json::json!("legacy-value")).unwrap(),
        )
        .unwrap();
    secrets
        .set_secret(&legacy_secret_key, "legacy-secret")
        .unwrap();

    let state = AppState::try_new_with_secret_store(
        config,
        ObservabilityService::new(directory.path()).unwrap(),
        SettingsService::new(directory.path().to_path_buf()),
        secrets.clone(),
    )
    .unwrap();

    assert!(list_inputs_core(&state, "computer-a").unwrap().is_empty());
    assert!(state
        .sdk_config
        .load_project_input_definitions("computer-a")
        .unwrap()
        .is_empty());
    assert_eq!(
        InputValueStore::for_computer(state.config.as_ref(), "computer-a")
            .get("shared-token")
            .unwrap(),
        None
    );
    assert_eq!(
        keychain::get_input_secret(secrets.as_ref(), "computer-a", "shared-token").unwrap(),
        None
    );
    let read_keys = recording_secrets.read_keys();
    assert!(
        !read_keys.contains(&legacy_value_key),
        "startup must not read the legacy unscoped value key"
    );
    assert!(
        !read_keys.contains(&legacy_secret_key),
        "startup must not read the legacy unscoped secret key"
    );

    assert!(state
        .computer_registry
        .runtime("computer-a")
        .await
        .is_some());

    assert_eq!(
        secrets.get_secret(&legacy_value_key).unwrap().as_deref(),
        Some("\"legacy-value\"")
    );
    assert_eq!(
        secrets.get_secret(&legacy_secret_key).unwrap().as_deref(),
        Some("legacy-secret")
    );
    assert_eq!(
        std::fs::read(&legacy_global_inputs_path).unwrap(),
        legacy_global_inputs
    );
}
