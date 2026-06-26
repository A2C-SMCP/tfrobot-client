#[allow(dead_code)]
mod common;

use common::create_test_app_state;
use tempfile::TempDir;
use tfrobot_client_lib::commands::computer::{
    create_computer_instance_core, delete_computer_instance_core, duplicate_computer_instance_core,
    get_computer_instance_status_core, list_computer_instances_core, rename_computer_instance_core,
    start_computer_instance_core, stop_computer_instance_core, CreateComputerInstanceRequest,
    DuplicateComputerInstanceRequest, DuplicateSkillHomeMode, RenameComputerInstanceRequest,
};
use tfrobot_client_lib::commands::inputs::InputDefinition;
use tfrobot_client_lib::commands::mcp;
use tfrobot_client_lib::services::computer::{ComputerInstance, RobotBindingMetadata};
use tfrobot_client_lib::services::connection_targets::ManualSmcpTarget;

const LEGACY_INSTANCE_ID: &str = "default";

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

    let instance_storage_root = state.config.computer_instance_storage_root(&created.id);
    std::fs::create_dir_all(instance_storage_root.join("skills")).unwrap();
    std::fs::write(
        instance_storage_root.join("skills").join("skill.md"),
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
    state
        .config
        .update_computer_instance(&source.id, |instance| {
            instance.robot_binding = Some(RobotBindingMetadata {
                employee_id: 42,
                robot_id: Some("robot-42".to_string()),
                robot_account_id: Some(4200),
                namespace: Some("test".to_string()),
                robot_name: Some("Robot 42".to_string()),
            });
            instance.input_values.insert(
                "token".to_string(),
                serde_json::Value::String("persisted-input".to_string()),
            );
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
            computer_name: "computer-a".to_string(),
            headers: std::collections::HashMap::from([(
                "X-TF-Route".to_string(),
                "route-a".to_string(),
            )]),
        })
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
    assert_eq!(duplicate.name, "Duplicate");
    assert_eq!(
        duplicate.description.as_deref(),
        Some("Duplicate description")
    );
    assert_ne!(duplicate.id, source.id);
    assert_uuid_instance_id(&duplicate.id);
    assert_eq!(
        duplicate_config.input_values.get("token"),
        Some(&serde_json::Value::String("persisted-input".to_string()))
    );
    assert_eq!(
        duplicate_config
            .robot_binding
            .as_ref()
            .and_then(|binding| binding.robot_name.as_deref()),
        Some("Robot 42")
    );
    assert_eq!(
        duplicate_config
            .connection_policy
            .target
            .as_ref()
            .map(|target| target.id.as_str()),
        Some(target.id.as_str())
    );
    assert!(!duplicate.running);
    assert!(!duplicate.connected);
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
    std::fs::create_dir_all(source_skill_root.join("nested")).unwrap();
    std::fs::write(source_skill_root.join("skill.md"), "source skill").unwrap();
    std::fs::write(source_skill_root.join("nested").join("data.txt"), "nested").unwrap();

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
    assert!(!empty_skill_root.join("skill.md").exists());

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
        std::fs::read_to_string(copy_skill_root.join("skill.md")).unwrap(),
        "source skill"
    );
    assert_eq!(
        std::fs::read_to_string(copy_skill_root.join("nested").join("data.txt")).unwrap(),
        "nested"
    );
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
    let source_skill_file = dir.path().join("source-skill-file");
    std::fs::write(&source_skill_file, "not a directory").unwrap();
    state
        .config
        .update_computer_instance(&source.id, |instance| {
            instance.local_skills_root = Some(source_skill_file.clone());
        })
        .unwrap();

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
    assert_eq!(remaining_entries, 0);
}

#[tokio::test]
async fn duplicate_rejects_copy_when_destination_would_be_inside_source() {
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
    std::fs::create_dir_all(&skill_home_base).unwrap();
    std::fs::write(skill_home_base.join("root-skill.md"), "root").unwrap();
    state
        .config
        .update_computer_instance(&source.id, |instance| {
            instance.local_skills_root = Some(skill_home_base.clone());
        })
        .unwrap();

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

    assert!(error.contains("destination is inside source directory"));
    assert_eq!(
        state
            .config
            .load_computer_instances()
            .unwrap()
            .instances
            .len(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(skill_home_base.join("root-skill.md")).unwrap(),
        "root"
    );
    assert_eq!(std::fs::read_dir(&skill_home_base).unwrap().count(), 1);
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
async fn status_reads_do_not_sync_runtime_inputs() {
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
                password: Some(true),
            }],
        )
        .unwrap();

    list_computer_instances_core(&state).await.unwrap();
    get_computer_instance_status_core(&state, created.id.clone())
        .await
        .unwrap();

    let runtime = state.computer_registry.runtime(&created.id).await.unwrap();
    assert!(!runtime.inputs.read().await.contains_key("api-key"));
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
        .await;

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
async fn mcp_configs_and_input_values_are_isolated_per_instance() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(LEGACY_INSTANCE_ID, "Legacy Computer"))
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
    state
        .config
        .save_inputs_for_instance(
            LEGACY_INSTANCE_ID,
            &[InputDefinition::PromptString {
                id: "token".to_string(),
                label: "Default token".to_string(),
                description: None,
                default: None,
                password: None,
            }],
        )
        .unwrap();
    state
        .config
        .save_inputs_for_instance(
            &second.id,
            &[InputDefinition::PromptString {
                id: "token".to_string(),
                label: "Second token".to_string(),
                description: None,
                default: None,
                password: None,
            }],
        )
        .unwrap();
    state
        .config
        .save_input_values_for_instance(
            LEGACY_INSTANCE_ID,
            &std::collections::HashMap::from([(
                "token".to_string(),
                serde_json::Value::String("default-secret".to_string()),
            )]),
        )
        .unwrap();
    state
        .config
        .save_input_values_for_instance(
            &second.id,
            &std::collections::HashMap::from([(
                "token".to_string(),
                serde_json::Value::String("second-secret".to_string()),
            )]),
        )
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
    let default_values = state
        .config
        .load_input_values_for_instance(LEGACY_INSTANCE_ID)
        .unwrap();
    let second_values = state
        .config
        .load_input_values_for_instance(&second.id)
        .unwrap();

    assert_eq!(default_servers[0].name, "default-only");
    assert_eq!(second_servers[0].name, "second-only");
    assert_eq!(default_inputs[0].id(), "token");
    assert_eq!(second_inputs[0].id(), "token");
    match &default_inputs[0] {
        InputDefinition::PromptString { label, .. } => assert_eq!(label, "Default token"),
        _ => panic!("expected default PromptString input"),
    }
    match &second_inputs[0] {
        InputDefinition::PromptString { label, .. } => assert_eq!(label, "Second token"),
        _ => panic!("expected second PromptString input"),
    }
    assert_eq!(
        default_values.get("token"),
        Some(&serde_json::Value::String("default-secret".to_string()))
    );
    assert_eq!(
        second_values.get("token"),
        Some(&serde_json::Value::String("second-secret".to_string()))
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
