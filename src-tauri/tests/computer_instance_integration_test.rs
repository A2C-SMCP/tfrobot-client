#[allow(dead_code)]
mod common;

use common::create_test_app_state;
use tempfile::TempDir;
use tfrobot_client_lib::commands::computer::{
    create_computer_instance_core, delete_computer_instance_core, duplicate_computer_instance_core,
    get_computer_instance_status_core, list_computer_instances_core, rename_computer_instance_core,
    start_computer_instance_core, stop_computer_instance_core, CreateComputerInstanceRequest,
    DuplicateComputerInstanceRequest, RenameComputerInstanceRequest,
};
use tfrobot_client_lib::commands::inputs::InputDefinition;
use tfrobot_client_lib::commands::mcp;
use tfrobot_client_lib::services::computer::{ComputerInstance, DEFAULT_COMPUTER_INSTANCE_ID};

#[tokio::test]
async fn command_core_creates_renames_lists_and_deletes_instance() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "  Second Computer  ".to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(created.name, "Second Computer");
    assert!(!created.is_default);
    assert!(!created.running);

    let renamed = rename_computer_instance_core(
        &state,
        RenameComputerInstanceRequest {
            id: created.id.clone(),
            name: "Renamed Computer".to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.name, "Renamed Computer");

    let list = list_computer_instances_core(&state).await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list.iter().any(|instance| instance.is_default));
    assert!(list
        .iter()
        .any(|instance| instance.id == created.id && instance.name == "Renamed Computer"));

    delete_computer_instance_core(&state, created.id.clone())
        .await
        .unwrap();
    let list = list_computer_instances_core(&state).await.unwrap();
    assert!(list.is_empty());
    assert!(state.computer_registry.runtime(&created.id).await.is_none());
}

#[tokio::test]
async fn created_instances_use_uuid_based_ids() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());

    let first = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "First".to_string(),
        },
    )
    .await
    .unwrap();
    let second = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Second".to_string(),
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
        },
    )
    .await
    .unwrap();
    state
        .config
        .update_computer_instance(&source.id, |instance| {
            instance.input_values.insert(
                "token".to_string(),
                serde_json::Value::String("persisted-input".to_string()),
            );
        })
        .unwrap();
    start_computer_instance_core(&state, source.id.clone())
        .await
        .unwrap();

    let duplicate = duplicate_computer_instance_core(
        &state,
        DuplicateComputerInstanceRequest {
            source_id: source.id.clone(),
            name: "Duplicate".to_string(),
        },
    )
    .await
    .unwrap();

    let duplicate_config = state.config.get_computer_instance(&duplicate.id).unwrap();
    assert_eq!(duplicate.name, "Duplicate");
    assert_ne!(duplicate.id, source.id);
    assert_uuid_instance_id(&duplicate.id);
    assert_eq!(
        duplicate_config.input_values.get("token"),
        Some(&serde_json::Value::String("persisted-input".to_string()))
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
async fn start_stop_and_delete_running_instance_are_instance_scoped() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    let one = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
        },
    )
    .await
    .unwrap();
    let two = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Two".to_string(),
        },
    )
    .await
    .unwrap();

    start_computer_instance_core(&state, one.id.clone())
        .await
        .unwrap();
    start_computer_instance_core(&state, two.id.clone())
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
    assert!(state.computer_registry.default_runtime().await.is_some());
}

#[tokio::test]
async fn list_marks_only_configured_default_instance() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    state
        .config
        .add_computer_instance(ComputerInstance::default_instance())
        .unwrap();
    let mut instances = state.config.load_computer_instances().unwrap();
    instances.default_instance_id = DEFAULT_COMPUTER_INSTANCE_ID.to_string();
    state.config.save_computer_instances(&instances).unwrap();
    let second = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Second".to_string(),
        },
    )
    .await
    .unwrap();

    let list = list_computer_instances_core(&state).await.unwrap();

    let default = list
        .iter()
        .find(|instance| instance.id == DEFAULT_COMPUTER_INSTANCE_ID)
        .unwrap();
    let second = list
        .iter()
        .find(|instance| instance.id == second.id)
        .unwrap();
    assert!(default.is_default);
    assert!(!second.is_default);
}

#[tokio::test]
async fn default_instance_status_is_preserved_across_status_and_lifecycle_commands() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    state
        .config
        .add_computer_instance(ComputerInstance::default_instance())
        .unwrap();
    let mut instances = state.config.load_computer_instances().unwrap();
    instances.default_instance_id = DEFAULT_COMPUTER_INSTANCE_ID.to_string();
    state.config.save_computer_instances(&instances).unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(DEFAULT_COMPUTER_INSTANCE_ID)
                .unwrap(),
        )
        .await;

    let status =
        get_computer_instance_status_core(&state, DEFAULT_COMPUTER_INSTANCE_ID.to_string())
            .await
            .unwrap();
    let started = start_computer_instance_core(&state, DEFAULT_COMPUTER_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let stopped = stop_computer_instance_core(&state, DEFAULT_COMPUTER_INSTANCE_ID.to_string())
        .await
        .unwrap();

    assert!(status.is_default);
    assert!(started.is_default);
    assert!(stopped.is_default);
}

#[tokio::test]
async fn mcp_configs_and_input_values_are_isolated_per_instance() {
    let dir = TempDir::new().unwrap();
    let state = create_test_app_state(dir.path());
    state
        .config
        .add_computer_instance(ComputerInstance::default_instance())
        .unwrap();
    let second = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "Second".to_string(),
        },
    )
    .await
    .unwrap();

    mcp::add_mcp_server_core(
        &state,
        DEFAULT_COMPUTER_INSTANCE_ID,
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
            DEFAULT_COMPUTER_INSTANCE_ID,
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
            DEFAULT_COMPUTER_INSTANCE_ID,
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

    let default_servers = mcp::get_mcp_servers_core(&state, DEFAULT_COMPUTER_INSTANCE_ID)
        .await
        .unwrap();
    let second_servers = mcp::get_mcp_servers_core(&state, &second.id).await.unwrap();
    let default_inputs = state
        .config
        .load_inputs_for_instance(DEFAULT_COMPUTER_INSTANCE_ID)
        .unwrap();
    let second_inputs = state.config.load_inputs_for_instance(&second.id).unwrap();
    let default_values = state
        .config
        .load_input_values_for_instance(DEFAULT_COMPUTER_INSTANCE_ID)
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
