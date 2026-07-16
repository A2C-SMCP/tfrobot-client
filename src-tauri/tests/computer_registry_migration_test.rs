#[allow(dead_code)]
mod common;

use std::collections::HashMap;
use std::sync::Arc;

use tempfile::TempDir;
use tfrobot_client_lib::commands::inputs::InputDefinition;
use tfrobot_client_lib::services::computer::{
    ComputerInstance, ComputerInstancesConfig, ManagedMcpServer,
};
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::config_migration::{migrate_legacy_config, MigrationOutcome};
use tfrobot_client_lib::services::connection_targets::{ConnectionTargetsConfig, ManualSmcpTarget};
use tfrobot_client_lib::services::keychain::{self, InMemorySecretStore, SecretStore};
use tfrobot_client_lib::services::logger::LogService;
use tfrobot_client_lib::services::settings::{
    AppSettings, ManagerSessionSettings, SettingsService,
};
use tfrobot_client_lib::AppState;

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
