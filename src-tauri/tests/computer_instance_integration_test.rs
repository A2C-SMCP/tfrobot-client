#[allow(dead_code)]
mod common;

use a2c_smcp::smcp_computer::settings::config::{ConfigEdit, ConfigEntity, EditIntent};
use common::{create_test_app_state, mcp};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tfrobot_client_lib::commands::computer::{
    create_computer_instance_core, delete_computer_instance_core, duplicate_computer_instance_core,
    get_computer_instance_status_core, list_computer_instances_core, rename_computer_instance_core,
    start_computer_instance_core, stop_computer_instance_core, CreateComputerInstanceRequest,
    DuplicateComputerInstanceRequest, DuplicateSkillHomeMode, RenameComputerInstanceRequest,
};
use tfrobot_client_lib::commands::connection::{
    connect_connection_target_core, connect_connection_target_for_policy_core, disconnect_smcp_core,
};
use tfrobot_client_lib::commands::inputs::{self, InputDefinition};
use tfrobot_client_lib::services::computer::{
    ComputerConnectionTarget, ComputerInstance, ManagerRobotBindingState, RobotBindingMetadata,
};
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::connection_targets::ManualSmcpTarget;
use tfrobot_client_lib::services::keychain::{KeychainError, SecretStore};
use tfrobot_client_lib::services::observability::ObservabilityService;
use tfrobot_client_lib::services::settings::SettingsService;
use tfrobot_client_lib::AppState;

const LEGACY_INSTANCE_ID: &str = "default";

#[derive(Default)]
struct ToggleReadFailureSecretStore {
    secrets: Mutex<HashMap<String, String>>,
    fail_reads: AtomicBool,
    read_count: AtomicUsize,
}

impl ToggleReadFailureSecretStore {
    fn fail_reads(&self) {
        self.fail_reads.store(true, Ordering::SeqCst);
    }

    fn read_count(&self) -> usize {
        self.read_count.load(Ordering::SeqCst)
    }
}

impl SecretStore for ToggleReadFailureSecretStore {
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        self.secrets
            .lock()
            .map_err(|error| KeychainError::Store(error.to_string()))?
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
        self.read_count.fetch_add(1, Ordering::SeqCst);
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err(KeychainError::Store(
                "injected Keychain read failure".to_string(),
            ));
        }
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

fn create_state_with_toggle_secret_store(
    path: &std::path::Path,
) -> (AppState, Arc<ToggleReadFailureSecretStore>) {
    let secrets = Arc::new(ToggleReadFailureSecretStore::default());
    let state = AppState::new_with_secret_store(
        ConfigService::new(path.to_path_buf()).unwrap(),
        ObservabilityService::new(path).unwrap(),
        SettingsService::new(path.to_path_buf()),
        secrets.clone(),
    );
    (state, secrets)
}

async fn create_computer_with_input(state: &AppState, name: &str) -> String {
    let created = create_computer_instance_core(
        state,
        CreateComputerInstanceRequest {
            name: name.to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    inputs::add_or_update_input_core(
        state,
        &created.id,
        InputDefinition::PromptString {
            id: "token".to_string(),
            label: "Token".to_string(),
            description: None,
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    created.id
}

#[tokio::test]
async fn command_core_creates_renames_lists_and_deletes_instance() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "  Second Computer  ".to_string(),
            description: Some("  Test description  ".to_string()),
        },
    )
    .await
    .unwrap();
    assert_eq!(created.name, "Second Computer");
    assert_eq!(created.description.as_deref(), Some("Test description"));
    assert!(!created.running);

    let renamed = rename_computer_instance_core(
        &state,
        RenameComputerInstanceRequest {
            id: created.id.clone(),
            name: "Renamed Computer".to_string(),
            description: Some("Renamed description".to_string()),
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.name, "Renamed Computer");
    assert_eq!(renamed.description.as_deref(), Some("Renamed description"));

    let list = list_computer_instances_core(&state).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list
        .iter()
        .any(|instance| instance.id == created.id && instance.name == "Renamed Computer"));
    inputs::add_or_update_input_core(
        &state,
        &created.id,
        InputDefinition::PromptString {
            id: "delete-token".to_string(),
            label: "Delete token".to_string(),
            description: None,
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        &created.id,
        "delete-token".to_string(),
        serde_json::json!("deleted-secret"),
    )
    .await
    .unwrap();
    tfrobot_client_lib::services::keychain::set_input_secret(
        state.secret_store.as_ref(),
        "other-computer",
        "delete-token",
        "preserved-secret",
    )
    .unwrap();

    let instance_storage_root = state.config.computer_instance_storage_root(&created.id);
    std::fs::create_dir_all(instance_storage_root.join("skill_home")).unwrap();
    std::fs::write(
        instance_storage_root.join("skill_home").join("skill.md"),
        "skill",
    )
    .unwrap();
    std::fs::create_dir_all(instance_storage_root.join("blob")).unwrap();
    std::fs::write(instance_storage_root.join("blob").join("blob.bin"), "blob").unwrap();
    assert!(instance_storage_root.exists());

    delete_computer_instance_core(&state, created.id.clone())
        .await
        .unwrap();
    let list = list_computer_instances_core(&state).await.unwrap();
    assert!(list.is_empty());
    assert!(state.computer_registry.runtime(&created.id).await.is_none());
    assert!(!instance_storage_root.exists());
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_secret(
            state.secret_store.as_ref(),
            &created.id,
            "delete-token",
        )
        .unwrap(),
        None
    );
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_secret(
            state.secret_store.as_ref(),
            "other-computer",
            "delete-token",
        )
        .unwrap()
        .as_deref(),
        Some("preserved-secret")
    );
}

#[tokio::test]
async fn created_instances_use_uuid_based_ids() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());

    let first = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "First".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    let second = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Second".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    assert_ne!(first.id, second.id);
    assert_uuid_instance_id(&first.id);
    assert_uuid_instance_id(&second.id);
}

#[tokio::test]
async fn create_does_not_require_keychain_reads() {
    let dir = TempDir::new().unwrap();
    let (state, secrets) = create_state_with_toggle_secret_store(dir.path());
    create_computer_with_input(&state, "Existing").await;
    let read_count_before = secrets.read_count();
    secrets.fail_reads();

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Created Without Keychain Reads".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    let persisted = state.config.load_computer_instances().unwrap();
    assert_eq!(persisted.instances.len(), 2);
    assert!(persisted
        .instances
        .iter()
        .any(|instance| instance.id == created.id));
    assert!(state.computer_registry.runtime(&created.id).await.is_some());
    assert_eq!(secrets.read_count(), read_count_before);
}

#[tokio::test]
async fn rename_does_not_require_keychain_reads() {
    let dir = TempDir::new().unwrap();
    let (state, secrets) = create_state_with_toggle_secret_store(dir.path());
    let id = create_computer_with_input(&state, "Original").await;
    let read_count_before = secrets.read_count();
    secrets.fail_reads();

    let renamed = rename_computer_instance_core(
        &state,
        RenameComputerInstanceRequest {
            id: id.clone(),
            name: "Renamed Without Keychain Reads".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(renamed.name, "Renamed Without Keychain Reads");
    assert_eq!(
        state.config.get_computer_instance(&id).unwrap().name,
        "Renamed Without Keychain Reads"
    );
    assert_eq!(
        state
            .computer_registry
            .runtime(&id)
            .await
            .unwrap()
            .instance
            .name,
        "Renamed Without Keychain Reads"
    );
    assert_eq!(secrets.read_count(), read_count_before);
}

#[tokio::test]
async fn duplicate_does_not_require_keychain_reads() {
    let dir = TempDir::new().unwrap();
    let (state, secrets) = create_state_with_toggle_secret_store(dir.path());
    let source_id = create_computer_with_input(&state, "Source").await;
    let read_count_before = secrets.read_count();
    secrets.fail_reads();

    let duplicated = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source_id.clone(),
            name: "Duplicated Without Keychain Reads".to_string(),
            description: None,
            copy_robot_binding: false,
            connection_target_id: None,
            skill_home_mode: DuplicateSkillHomeMode::Empty,
        },
    )
    .await
    .unwrap();

    let persisted = state.config.load_computer_instances().unwrap();
    assert_eq!(persisted.instances.len(), 2);
    assert!(persisted
        .instances
        .iter()
        .any(|instance| instance.id == source_id));
    assert!(persisted
        .instances
        .iter()
        .any(|instance| instance.id == duplicated.id));
    assert!(state
        .computer_registry
        .runtime(&duplicated.id)
        .await
        .is_some());
    assert!(state
        .config
        .computer_instance_storage_root(&duplicated.id)
        .exists());
    assert_eq!(secrets.read_count(), read_count_before);
}

#[tokio::test]
async fn computer_lifecycle_commands_wait_for_the_transaction_lock() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(create_test_app_state(dir.path()));
    let guard = state.computer_lifecycle_lock.lock().await;
    let operation_state = state.clone();
    let mut operation = tokio::spawn(async move {
        create_computer_instance_core(
            operation_state.as_ref(),
            CreateComputerInstanceRequest {
                name: "Serialized".to_string(),
                description: None,
            },
        )
        .await
    });

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut operation)
            .await
            .is_err()
    );
    drop(guard);

    let created = operation.await.unwrap().unwrap();

    let guard = state.computer_lifecycle_lock.lock().await;
    let operation_state = state.clone();
    let mut input_operation = tokio::spawn(async move {
        inputs::add_or_update_input_core(
            operation_state.as_ref(),
            &created.id,
            InputDefinition::PromptString {
                id: "serialized-input".to_string(),
                label: "Serialized input".to_string(),
                description: None,
                default: None,
                password: None,
            },
        )
        .await
    });

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut input_operation)
            .await
            .is_err()
    );
    drop(guard);

    input_operation.await.unwrap().unwrap();
}

#[tokio::test]
async fn mcp_mutations_wait_for_the_computer_lifecycle_transaction_lock() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(create_test_app_state(dir.path()));
    let created = create_computer_instance_core(
        state.as_ref(),
        CreateComputerInstanceRequest {
            name: "Serialized MCP".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    let guard = state.computer_lifecycle_lock.lock().await;
    let operation_state = state.clone();
    let instance_id = created.id.clone();
    let mut operation = tokio::spawn(async move {
        mcp::add_mcp_server_core(
            operation_state.as_ref(),
            &instance_id,
            common::echo_server_config("serialized-mcp"),
        )
        .await
    });

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut operation)
            .await
            .is_err(),
        "MCP mutation escaped the Computer lifecycle transaction"
    );
    drop(guard);

    operation.await.unwrap().unwrap();
}

#[tokio::test]
async fn connection_mutations_wait_for_the_computer_lifecycle_transaction_lock() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(create_test_app_state(dir.path()));
    let guard = state.computer_lifecycle_lock.lock().await;
    let operation_state = state.clone();
    let mut operation = tokio::spawn(async move {
        connect_connection_target_core(operation_state.as_ref(), "missing", "missing-target").await
    });

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut operation)
            .await
            .is_err(),
        "connection mutation escaped the Computer lifecycle transaction"
    );
    drop(guard);

    assert!(operation.await.unwrap().is_err());

    let guard = state.computer_lifecycle_lock.lock().await;
    let operation_state = state.clone();
    let mut operation =
        tokio::spawn(
            async move { disconnect_smcp_core(operation_state.as_ref(), "missing").await },
        );

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut operation)
            .await
            .is_err(),
        "disconnect mutation escaped the Computer lifecycle transaction"
    );
    drop(guard);

    assert!(operation.await.unwrap().is_err());
}

#[tokio::test]
async fn policy_connect_rejects_a_target_replaced_before_transaction_prepare() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Policy Race".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    for (id, office_id) in [("target-a", "office-a"), ("target-b", "office-b")] {
        state
            .config
            .save_manual_smcp_target(ManualSmcpTarget {
                id: id.to_string(),
                name: id.to_string(),
                url: "https://smcp.example.com".to_string(),
                namespace: "/smcp".to_string(),
                office_id: office_id.to_string(),
                headers: HashMap::new(),
            })
            .unwrap();
    }
    let stale_target = ComputerConnectionTarget::manual_smcp("target-a");
    state
        .config
        .update_computer_instance(&created.id, |instance| {
            instance.connection_policy.target =
                Some(ComputerConnectionTarget::manual_smcp("target-b"));
        })
        .unwrap();

    let error = connect_connection_target_for_policy_core(&state, &created.id, &stale_target)
        .await
        .unwrap_err();

    assert!(error.contains("changed before the connection could start"));
    assert!(matches!(
        state
            .config
            .get_computer_instance(&created.id)
            .unwrap()
            .connection_policy
            .target,
        Some(ComputerConnectionTarget::ManualSmcp { ref id }) if id == "target-b"
    ));
}

#[tokio::test]
async fn duplicate_copies_configuration_without_runtime_state() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let source = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Source".to_string(),
            description: Some("Source description".to_string()),
        },
    )
    .await
    .unwrap();
    let source_sdk_snapshot = state
        .sdk_config
        .update(
            &source.id,
            &[ConfigEdit::new(
                ConfigEntity::McpServer("source-sdk-server".to_string()),
                EditIntent::Upsert(serde_json::json!({
                    "type": "stdio",
                    "server_parameters": {"command": "node"}
                })),
            )],
        )
        .unwrap();
    state
        .config
        .update_computer_instance(&source.id, |instance| {
            instance.robot_binding = Some(RobotBindingMetadata {
                context_key: None,
                state: ManagerRobotBindingState::NeedsRebind,
                employee_id: 42,
                robot_id: Some("robot-42".to_string()),
                last_resolved_robot_account_id: Some("4200".to_string()),
                namespace: Some("test".to_string()),
                robot_name: Some("Robot 42".to_string()),
            });
        })
        .unwrap();
    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "target-a".to_string(),
            name: "Target A".to_string(),
            url: "https://smcp.example.com".to_string(),
            namespace: "/smcp".to_string(),
            office_id: "office-a".to_string(),
            headers: std::collections::HashMap::from([(
                "X-TF-Route".to_string(),
                "route-a".to_string(),
            )]),
        })
        .unwrap();
    inputs::add_or_update_input_core(
        &state,
        &source.id,
        InputDefinition::PromptString {
            id: "source-token".to_string(),
            label: "Source token".to_string(),
            description: None,
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        &source.id,
        "source-token".to_string(),
        serde_json::json!("source-secret"),
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, source.id.clone())
        .await
        .unwrap();

    let duplicate = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source.id.clone(),
            name: "Duplicate".to_string(),
            description: Some("Duplicate description".to_string()),
            copy_robot_binding: true,
            connection_target_id: Some(target.id.clone()),
            skill_home_mode: DuplicateSkillHomeMode::Empty,
        },
    )
    .await
    .unwrap();

    let duplicate_config = state.config.get_computer_instance(&duplicate.id).unwrap();
    let duplicate_sdk_snapshot = state.sdk_config.load(&duplicate.id);
    assert_eq!(duplicate.name, "Duplicate");
    assert_eq!(
        duplicate.description.as_deref(),
        Some("Duplicate description")
    );
    assert_ne!(duplicate.id, source.id);
    assert_uuid_instance_id(&duplicate.id);
    assert!(duplicate_config.input_values.is_empty());
    assert_eq!(duplicate_config.inputs.len(), 1);
    assert_eq!(duplicate_config.inputs[0].id(), "source-token");
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_secret(
            state.secret_store.as_ref(),
            &source.id,
            "source-token",
        )
        .unwrap()
        .as_deref(),
        Some("source-secret")
    );
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_secret(
            state.secret_store.as_ref(),
            &duplicate.id,
            "source-token",
        )
        .unwrap(),
        None
    );
    assert_eq!(
        duplicate_config
            .robot_binding
            .as_ref()
            .and_then(|binding| binding.robot_name.as_deref()),
        Some("Robot 42")
    );
    assert!(matches!(
        duplicate_config.connection_policy.target.as_ref(),
        Some(ComputerConnectionTarget::ManualSmcp { id }) if id == &target.id
    ));
    assert!(!duplicate.running);
    assert!(!duplicate.connected);
    assert!(!source_sdk_snapshot.revision.0.is_empty());
    assert!(!duplicate_sdk_snapshot.revision.0.is_empty());
    assert_eq!(
        duplicate_sdk_snapshot.revision,
        state.sdk_config.load(&duplicate.id).revision
    );
    assert_eq!(
        state.sdk_config.export(&source.id).unwrap(),
        state.sdk_config.export(&duplicate.id).unwrap()
    );
    assert_eq!(
        duplicate_sdk_snapshot.provenance,
        source_sdk_snapshot.provenance
    );
    assert_eq!(
        duplicate_sdk_snapshot
            .mcp
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>(),
        vec!["source-sdk-server"]
    );
    state
        .sdk_config
        .update(
            &duplicate.id,
            &[ConfigEdit::new(
                ConfigEntity::McpServer("duplicate-only".to_string()),
                EditIntent::Upsert(serde_json::json!({
                    "type": "stdio",
                    "server_parameters": {"command": "node"}
                })),
            )],
        )
        .unwrap();
    assert_eq!(
        state
            .sdk_config
            .load(&source.id)
            .mcp
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>(),
        vec!["source-sdk-server"]
    );
    assert!(
        state
            .computer_registry
            .runtime(&source.id)
            .await
            .unwrap()
            .is_running()
            .await
    );
}

#[tokio::test]
async fn duplicate_uses_own_skill_home_and_can_copy_source_contents() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let source = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Source".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    let source_skill_root = state.config.default_local_skills_root(&source.id);
    let source_user_skills = source_skill_root.join("user");
    std::fs::create_dir_all(source_user_skills.join("nested")).unwrap();
    std::fs::write(source_user_skills.join("skill.md"), "source skill").unwrap();
    std::fs::write(source_user_skills.join("nested").join("data.txt"), "nested").unwrap();
    std::fs::write(
        source_skill_root.join("installed_plugins.json"),
        r#"{"plugins":{"audit@acme":{"installPath":"/source/plugin"}}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(source_skill_root.join("marketplace/acme")).unwrap();
    std::fs::write(
        source_skill_root.join("marketplace/acme/plugin.json"),
        "source governance content",
    )
    .unwrap();

    let empty_duplicate = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source.id.clone(),
            name: "Empty Duplicate".to_string(),
            description: None,
            copy_robot_binding: false,
            connection_target_id: None,
            skill_home_mode: DuplicateSkillHomeMode::Empty,
        },
    )
    .await
    .unwrap();
    let empty_config = state
        .config
        .get_computer_instance(&empty_duplicate.id)
        .unwrap();
    let empty_skill_root = state.config.default_local_skills_root(&empty_duplicate.id);
    assert!(empty_config.local_skills_root.is_none());
    assert!(empty_skill_root.exists());
    assert!(!empty_skill_root.join("user/skill.md").exists());

    let copy_duplicate = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source.id.clone(),
            name: "Copy Duplicate".to_string(),
            description: None,
            copy_robot_binding: false,
            connection_target_id: None,
            skill_home_mode: DuplicateSkillHomeMode::Copy,
        },
    )
    .await
    .unwrap();
    let copy_config = state
        .config
        .get_computer_instance(&copy_duplicate.id)
        .unwrap();
    let copy_skill_root = state.config.default_local_skills_root(&copy_duplicate.id);
    assert!(copy_config.local_skills_root.is_none());
    assert_ne!(copy_skill_root, source_skill_root);
    assert_eq!(
        std::fs::read_to_string(copy_skill_root.join("user/skill.md")).unwrap(),
        "source skill"
    );
    assert_eq!(
        std::fs::read_to_string(copy_skill_root.join("user/nested/data.txt")).unwrap(),
        "nested"
    );
    assert!(!copy_skill_root.join("installed_plugins.json").exists());
    assert!(!copy_skill_root.join("marketplace").exists());
}

#[tokio::test]
async fn duplicate_copy_failure_cleans_destination_and_returns_error() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let source = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Source".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    state
        .sdk_config
        .update(
            &source.id,
            &[ConfigEdit::new(
                ConfigEntity::McpServer("rollback-source".to_string()),
                EditIntent::Upsert(serde_json::json!({
                    "type": "stdio",
                    "server_parameters": {"command": "node"}
                })),
            )],
        )
        .unwrap();
    let source_skill_root = state.config.default_local_skills_root(&source.id);
    std::fs::create_dir_all(&source_skill_root).unwrap();
    std::fs::write(source_skill_root.join("user"), "not a directory").unwrap();

    let error = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source.id.clone(),
            name: "Copy Duplicate".to_string(),
            description: None,
            copy_robot_binding: false,
            connection_target_id: None,
            skill_home_mode: DuplicateSkillHomeMode::Copy,
        },
    )
    .await
    .unwrap_err();

    assert!(error.contains("Failed to copy skills"));
    assert!(error.contains("source is not a directory"));
    assert_eq!(
        state
            .config
            .load_computer_instances()
            .unwrap()
            .instances
            .len(),
        1
    );
    let skill_home_base = state.config.computer_skill_home_base();
    let remaining_entries = if skill_home_base.exists() {
        std::fs::read_dir(skill_home_base).unwrap().count()
    } else {
        0
    };
    assert_eq!(remaining_entries, 1);
    assert!(state
        .sdk_config
        .project_anchor(&source.id)
        .join(".tfrobot/mcp.json")
        .is_file());
}

#[tokio::test]
async fn duplicate_copy_is_safe_when_custom_root_contains_target_storage() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let source = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Source".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    let skill_home_base = state.config.computer_skill_home_base();
    std::fs::create_dir_all(skill_home_base.join("user")).unwrap();
    std::fs::write(skill_home_base.join("user/root-skill.md"), "root").unwrap();
    std::fs::write(
        skill_home_base.join("installed_plugins.json"),
        r#"{"plugins":{"audit@acme":{"installPath":"/source/plugin"}}}"#,
    )
    .unwrap();
    state
        .config
        .update_computer_instance(&source.id, |instance| {
            instance.local_skills_root = Some(skill_home_base.clone());
        })
        .unwrap();

    let duplicate = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source.id.clone(),
            name: "Copy Duplicate".to_string(),
            description: None,
            copy_robot_binding: false,
            connection_target_id: None,
            skill_home_mode: DuplicateSkillHomeMode::Copy,
        },
    )
    .await
    .unwrap();

    let duplicate_skill_root = state.config.default_local_skills_root(&duplicate.id);
    assert_eq!(
        std::fs::read_to_string(duplicate_skill_root.join("user/root-skill.md")).unwrap(),
        "root"
    );
    assert!(!duplicate_skill_root.join("installed_plugins.json").exists());
}

#[tokio::test]
async fn start_stop_and_delete_running_instance_are_instance_scoped() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let one = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    let two = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Two".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    start_computer_instance_core(None, &state, one.id.clone())
        .await
        .unwrap();
    start_computer_instance_core(None, &state, two.id.clone())
        .await
        .unwrap();

    stop_computer_instance_core(&state, one.id.clone())
        .await
        .unwrap();
    assert!(
        !state
            .computer_registry
            .runtime(&one.id)
            .await
            .unwrap()
            .is_running()
            .await
    );
    assert!(
        state
            .computer_registry
            .runtime(&two.id)
            .await
            .unwrap()
            .is_running()
            .await
    );

    delete_computer_instance_core(&state, two.id.clone())
        .await
        .unwrap();
    assert!(state.computer_registry.runtime(&two.id).await.is_none());
}

#[tokio::test]
async fn delete_closes_activity_admission_before_shutting_down_runtime() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(create_test_app_state(dir.path()));
    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Busy".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, created.id.clone())
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(&created.id)
        .await
        .expect("runtime should exist");
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let activity_runtime = runtime.clone();
    let activity_task = tokio::spawn(async move {
        activity_runtime
            .hold_activity_for_test(started_tx, release_rx)
            .await
    });
    started_rx.await.expect("activity should start");

    let delete_state = state.clone();
    let instance_id = created.id.clone();
    let delete_task =
        tokio::spawn(
            async move { delete_computer_instance_core(&delete_state, instance_id).await },
        );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !runtime.is_retired_for_test() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("delete did not close runtime activity admission");

    assert!(
        runtime.is_running().await,
        "busy runtime shut down before activity drained"
    );
    assert!(!runtime.can_begin_activity_for_test());
    assert!(!delete_task.is_finished());

    release_tx.send(()).expect("release activity");
    activity_task.await.unwrap().unwrap();
    delete_task.await.unwrap().unwrap();

    assert!(state.computer_registry.runtime(&created.id).await.is_none());
    assert!(state.config.get_computer_instance(&created.id).is_err());
}

#[tokio::test]
async fn failed_delete_preserves_the_authoritative_runtime_incarnation() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Rollback".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, created.id.clone())
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(&created.id)
        .await
        .expect("runtime should exist");
    let previous_incarnation = runtime.runtime_snapshot().await.incarnation;
    let storage_root = state.config.computer_instance_storage_root(&created.id);
    std::fs::create_dir_all(&storage_root).unwrap();
    std::fs::write(
        storage_root.parent().unwrap().join(".trash"),
        b"block trash directory creation",
    )
    .unwrap();

    let error = delete_computer_instance_core(&state, created.id.clone())
        .await
        .expect_err("quarantine should fail");

    assert!(error.contains("Failed to create Computer storage trash"));
    let restored = state
        .computer_registry
        .runtime(&created.id)
        .await
        .expect("failed deletion should restore a runtime");
    assert_eq!(
        restored.runtime_snapshot().await.incarnation,
        previous_incarnation,
        "reversible deletion failure must not replace the connected runtime"
    );
    assert!(!restored.is_retired_for_test());
    assert!(restored.is_running().await);
    assert!(state.config.get_computer_instance(&created.id).is_ok());
}

#[tokio::test]
async fn status_reads_reconcile_runtime_inputs_from_computer_storage() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Computer".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    state
        .config
        .save_inputs_for_instance(
            &created.id,
            &[InputDefinition::PromptString {
                id: "api-key".to_string(),
                label: "API Key".to_string(),
                description: None,
                default: Some("default-key".to_string()),
                password: Some(false),
            }],
        )
        .unwrap();

    list_computer_instances_core(&state).await.unwrap();
    get_computer_instance_status_core(&state, created.id.clone())
        .await
        .unwrap();

    let runtime = state.computer_registry.runtime(&created.id).await.unwrap();
    assert!(runtime.inputs.read().await.contains_key("api-key"));
}

#[tokio::test]
async fn legacy_default_id_instance_is_a_normal_instance() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(LEGACY_INSTANCE_ID, "Legacy Computer"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(LEGACY_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();

    let list = list_computer_instances_core(&state).await.unwrap();
    assert!(list
        .iter()
        .any(|instance| instance.id == LEGACY_INSTANCE_ID && instance.name == "Legacy Computer"));

    let status = get_computer_instance_status_core(&state, LEGACY_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let started = start_computer_instance_core(None, &state, LEGACY_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let stopped = stop_computer_instance_core(&state, LEGACY_INSTANCE_ID.to_string())
        .await
        .unwrap();

    assert_eq!(status.id, LEGACY_INSTANCE_ID);
    assert!(!status.running);
    assert!(started.running);
    assert!(!stopped.running);
}

#[tokio::test]
async fn mcp_configs_inputs_and_values_are_isolated_per_computer() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(LEGACY_INSTANCE_ID, "Legacy Computer"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(LEGACY_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    let second = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Second".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    mcp::add_mcp_server_core(
        &state,
        LEGACY_INSTANCE_ID,
        common::echo_server_config("default-only"),
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        &second.id,
        common::echo_server_config("second-only"),
    )
    .await
    .unwrap();
    inputs::add_or_update_input_core(
        &state,
        LEGACY_INSTANCE_ID,
        InputDefinition::PromptString {
            id: "token".to_string(),
            label: "Shared token".to_string(),
            description: None,
            default: None,
            password: None,
        },
    )
    .await
    .unwrap();
    inputs::add_or_update_input_core(
        &state,
        &second.id,
        InputDefinition::PromptString {
            id: "token".to_string(),
            label: "Second token".to_string(),
            description: None,
            default: None,
            password: None,
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        &second.id,
        "token".to_string(),
        serde_json::Value::String("shared-secret".to_string()),
    )
    .await
    .unwrap();

    let default_servers = mcp::get_mcp_servers_core(&state, LEGACY_INSTANCE_ID)
        .await
        .unwrap();
    let second_servers = mcp::get_mcp_servers_core(&state, &second.id).await.unwrap();
    let default_inputs = state
        .config
        .load_inputs_for_instance(LEGACY_INSTANCE_ID)
        .unwrap();
    let second_inputs = state.config.load_inputs_for_instance(&second.id).unwrap();
    let default_value = tfrobot_client_lib::services::keychain::get_input_value(
        state.secret_store.as_ref(),
        LEGACY_INSTANCE_ID,
        "token",
    )
    .unwrap();
    let second_value = tfrobot_client_lib::services::keychain::get_input_value(
        state.secret_store.as_ref(),
        &second.id,
        "token",
    )
    .unwrap();

    assert_eq!(default_servers[0].name, "default-only");
    assert_eq!(second_servers[0].name, "second-only");
    assert_eq!(default_inputs[0].id(), "token");
    assert_eq!(second_inputs[0].id(), "token");
    match &default_inputs[0] {
        InputDefinition::PromptString { label, .. } => assert_eq!(label, "Shared token"),
        _ => panic!("expected default PromptString input"),
    }
    match &second_inputs[0] {
        InputDefinition::PromptString { label, .. } => assert_eq!(label, "Second token"),
        _ => panic!("expected second PromptString input"),
    }
    assert_eq!(default_value, None);
    assert_eq!(
        second_value,
        Some(serde_json::Value::String("shared-secret".to_string()))
    );
}

#[tokio::test]
async fn blank_names_are_rejected_and_default_named_instance_is_not_special() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());

    let error = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "   ".to_string(),
            description: None,
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("cannot be empty"));

    let error = delete_computer_instance_core(&state, "default".to_string())
        .await
        .unwrap_err();
    assert!(error.contains("not found"));
}

fn assert_uuid_instance_id(id: &str) {
    let uuid = id
        .strip_prefix("computer-")
        .expect("instance id should use the computer- prefix");
    let parsed = uuid::Uuid::parse_str(uuid).expect("instance id suffix should be a valid UUID");
    assert_eq!(parsed.get_version_num(), 4);
}
