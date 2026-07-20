#[allow(dead_code)]
mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use tempfile::TempDir;
use tfrobot_client_lib::commands::inputs::InputDefinition;
use tfrobot_client_lib::services::computer::{
    ComputerInstance, ComputerInstancesConfig, ManagedMcpServer,
};
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::config_migration::{migrate_legacy_config, MigrationOutcome};
use tfrobot_client_lib::services::connection_targets::{
    manual_target_keychain_id, ConnectionTargetsConfig, ManualSmcpTarget,
};
use tfrobot_client_lib::services::keychain::{
    self, InMemorySecretStore, KeychainError, SecretStore,
};
use tfrobot_client_lib::services::logger::LogService;
use tfrobot_client_lib::services::sdk_config::SdkConfigService;
use tfrobot_client_lib::services::settings::{
    AppSettings, ManagerSessionSettings, SettingsService,
};
use tfrobot_client_lib::AppState;

struct FailingVerificationSecretStore {
    inner: InMemorySecretStore,
    reads: AtomicUsize,
    fail_verification_reads: AtomicBool,
}

impl Default for FailingVerificationSecretStore {
    fn default() -> Self {
        Self {
            inner: InMemorySecretStore::default(),
            reads: AtomicUsize::new(0),
            fail_verification_reads: AtomicBool::new(true),
        }
    }
}

impl FailingVerificationSecretStore {
    fn allow_reads(&self) {
        self.fail_verification_reads.store(false, Ordering::SeqCst);
    }
}

impl SecretStore for FailingVerificationSecretStore {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        self.inner.set_secret(key, secret)
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        if self.fail_verification_reads.load(Ordering::SeqCst)
            && self.reads.fetch_add(1, Ordering::SeqCst) > 0
        {
            return Err(KeychainError::Store(
                "injected migration verification failure".to_string(),
            ));
        }
        self.inner.get_secret(key)
    }

    fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
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
        label: "API token".to_string(),
        description: None,
        default: Some("password-default".to_string()),
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
            base_url: "https://manager.example.test".to_string(),
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
        LogService::new(directory.path()).unwrap(),
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

    let global_inputs = state.config.load_global_inputs().unwrap();
    assert_eq!(global_inputs.inputs.len(), 1);
    let serialized_input = serde_json::to_value(&global_inputs.inputs[0]).unwrap();
    assert_eq!(serialized_input["id"], "api-token");
    assert!(serialized_input.get("default").is_none());
    assert_eq!(
        keychain::get_input_value(secrets.as_ref(), "api-token").unwrap(),
        Some(serde_json::json!("password-default"))
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
    assert_eq!(manager.session.unwrap().account_id, 8);

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

#[test]
fn migration_verification_failure_restores_profile_global_keychain_and_sdk_destinations() {
    let directory = TempDir::new().unwrap();
    let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
    let settings = SettingsService::new(directory.path().to_path_buf());
    let sdk_config = SdkConfigService::new(config.clone());
    let before_sdk = ProjectConfigDoc {
        settings: Some(
            serde_json::json!({ "existing": "preserve-me" })
                .as_object()
                .unwrap()
                .clone(),
        ),
        mcp: Some(
            serde_json::json!({
                "servers": {
                    "existing": {
                        "type": "stdio",
                        "server_parameters": { "command": "existing" }
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ),
        ..ProjectConfigDoc::default()
    };
    sdk_config.save("computer-a", &before_sdk).unwrap();

    let mut legacy_instance = ComputerInstance::new("computer-a", "Computer A");
    legacy_instance.inputs = vec![InputDefinition::PromptString {
        id: "api-token".to_string(),
        label: "API token".to_string(),
        description: None,
        default: None,
        password: Some(true),
    }];
    legacy_instance
        .input_values
        .insert("api-token".to_string(), serde_json::json!("new-secret"));
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

    let secrets = FailingVerificationSecretStore::default();
    let error =
        migrate_legacy_config(config.as_ref(), &sdk_config, &settings, &secrets).unwrap_err();
    secrets.allow_reads();

    assert!(
        error
            .to_string()
            .contains("injected migration verification failure"),
        "unexpected migration error: {error}"
    );
    assert!(config.legacy_computer_instances_path().exists());
    assert!(!config.migration_state_path().exists());
    assert!(!config.computer_profile_path("computer-a").unwrap().exists());
    assert!(!config.global_inputs_path().exists());
    assert!(!config.global_manual_targets_path().exists());
    assert!(!settings.global_manager_session_path().exists());
    assert!(!config.sdk_context_path("computer-a").unwrap().exists());
    assert_eq!(
        keychain::get_input_value(&secrets, "api-token").unwrap(),
        None
    );
    assert_eq!(
        a2c_smcp::smcp_computer::settings::config::load_project_config_doc(
            &sdk_config.project_anchor("computer-a"),
        )
        .unwrap(),
        before_sdk
    );
}
