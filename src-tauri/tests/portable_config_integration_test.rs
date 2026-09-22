mod common;

use common::{create_test_app_state, echo_server_config};
use tfrobot_client_lib::commands::computer::{
    create_computer_instance_core, list_computer_instances_core, CreateComputerInstanceRequest,
};
use tfrobot_client_lib::commands::inputs::{set_input_value_core, InputDefinition};
use tfrobot_client_lib::commands::portable_config::{
    commit_computer_package_import_core, export_computer_package_core,
    preview_computer_package_import_core,
};
use tfrobot_client_lib::commands::sdk_config::upsert_computer_mcp_config_with_inputs_core;
use tfrobot_client_lib::services::input_value_store::InputValueStore;
use tfrobot_client_lib::services::portable_config::{
    parse_package, PackageGroup, PORTABLE_PACKAGE_FORMAT_VERSION,
};

fn package_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("computer-package.json")
}

#[tokio::test]
async fn export_produces_a_versioned_package_and_preview_reports_name_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: Some("portable source".to_string()),
        },
    )
    .await
    .unwrap();

    let path = package_path(dir.path());
    export_computer_package_core(
        &state,
        &created.id,
        path.to_str().unwrap(),
        Some(PackageGroup::ALL.to_vec()),
    )
    .await
    .unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let package = parse_package(&bytes).unwrap();
    assert_eq!(package.format_version, PORTABLE_PACKAGE_FORMAT_VERSION);
    assert_eq!(package.manifest.source_computer_name, "One");
    assert_eq!(package.groups.len(), PackageGroup::ALL.len());
    assert!(package.profile.is_some());

    let preview = preview_computer_package_import_core(&state, path.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(preview.original_name, "One");
    assert!(preview.name_conflict);
    assert_eq!(preview.final_name, "One (2)");
    assert!(preview.version_compatible);
}

#[tokio::test]
async fn import_commit_creates_a_new_computer_and_rejects_conflicting_names() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: Some("portable source".to_string()),
        },
    )
    .await
    .unwrap();

    let path = package_path(dir.path());
    export_computer_package_core(
        &state,
        &created.id,
        path.to_str().unwrap(),
        Some(PackageGroup::ALL.to_vec()),
    )
    .await
    .unwrap();

    // Importing with the existing name is rejected.
    let conflict = commit_computer_package_import_core(&state, path.to_str().unwrap(), "One")
        .await
        .unwrap_err();
    assert!(conflict.contains("conflicts"));

    let imported = commit_computer_package_import_core(&state, path.to_str().unwrap(), "Two")
        .await
        .unwrap();
    assert_eq!(imported.name, "Two");
    assert_ne!(imported.id, created.id);
    assert_eq!(imported.description.as_deref(), Some("portable source"));

    let instances = list_computer_instances_core(&state).await.unwrap();
    let names = instances
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"One"));
    assert!(names.contains(&"Two"));
}

#[tokio::test]
async fn round_trip_preserves_sdk_config_and_non_sensitive_input_values() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: Some("round-trip source".to_string()),
        },
    )
    .await
    .unwrap();

    upsert_computer_mcp_config_with_inputs_core(
        &state,
        &created.id,
        echo_server_config("echo"),
        vec![InputDefinition::PromptString {
            id: "region".to_string(),
            label: Some("Region".to_string()),
            description: None,
            default: None,
            password: Some(false),
        }],
        Vec::new(),
    )
    .await
    .unwrap();
    set_input_value_core(
        &state,
        &created.id,
        "region".to_string(),
        serde_json::json!("cn"),
    )
    .await
    .unwrap();

    let path = package_path(dir.path());
    export_computer_package_core(
        &state,
        &created.id,
        path.to_str().unwrap(),
        Some(PackageGroup::ALL.to_vec()),
    )
    .await
    .unwrap();

    let package = parse_package(&std::fs::read(&path).unwrap()).unwrap();
    assert!(package.sdk_config.is_some());
    let sdk_config = package.sdk_config.as_ref().unwrap();
    assert!(
        sdk_config
            .mcp
            .as_ref()
            .is_some_and(|mcp| mcp.get("servers").is_some())
            || sdk_config
                .mcp_local
                .as_ref()
                .is_some_and(|mcp| mcp.get("servers").is_some())
    );
    assert_eq!(
        package.input_values.as_ref().unwrap().get("region"),
        Some(&serde_json::json!("cn"))
    );

    let imported = commit_computer_package_import_core(&state, path.to_str().unwrap(), "Two")
        .await
        .unwrap();
    assert_eq!(imported.mcp_server_count, 1);
    let imported_value = InputValueStore::for_computer(state.config.as_ref(), &imported.id)
        .get("region")
        .unwrap();
    assert_eq!(imported_value, Some(serde_json::json!("cn")));
}

#[tokio::test]
async fn deselected_sdk_group_is_not_exported_but_profile_remains() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());
    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    let path = package_path(dir.path());
    export_computer_package_core(
        &state,
        &created.id,
        path.to_str().unwrap(),
        Some(vec![PackageGroup::BasicProfile]),
    )
    .await
    .unwrap();

    let package = parse_package(&std::fs::read(&path).unwrap()).unwrap();
    assert!(package.profile.is_some());
    assert!(package.sdk_config.is_none());
    assert!(package.input_values.is_none());
    assert!(package.skills.is_none());
    assert_eq!(package.groups, vec![PackageGroup::BasicProfile]);
}

#[tokio::test]
async fn future_format_version_is_blocked_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());
    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: None,
        },
    )
    .await
    .unwrap();

    let path = package_path(dir.path());
    export_computer_package_core(
        &state,
        &created.id,
        path.to_str().unwrap(),
        Some(PackageGroup::ALL.to_vec()),
    )
    .await
    .unwrap();

    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["formatVersion"] = serde_json::json!(99);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let error = preview_computer_package_import_core(&state, path.to_str().unwrap())
        .await
        .unwrap_err();
    assert!(error.contains("unsupported package format version"));

    let instances = list_computer_instances_core(&state).await.unwrap();
    assert_eq!(instances.len(), 1);
}

#[tokio::test]
async fn repeated_import_of_the_same_package_creates_independent_computers() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        CreateComputerInstanceRequest {
            name: "One".to_string(),
            description: Some("source".to_string()),
        },
    )
    .await
    .unwrap();

    let path = package_path(dir.path());
    export_computer_package_core(
        &state,
        &created.id,
        path.to_str().unwrap(),
        Some(PackageGroup::ALL.to_vec()),
    )
    .await
    .unwrap();

    let first = commit_computer_package_import_core(&state, path.to_str().unwrap(), "Copy A")
        .await
        .unwrap();
    let second = commit_computer_package_import_core(&state, path.to_str().unwrap(), "Copy B")
        .await
        .unwrap();

    assert_ne!(first.id, second.id);
    assert_ne!(first.id, created.id);
    assert_ne!(second.id, created.id);
    assert_eq!(first.description.as_deref(), Some("source"));
    assert_eq!(second.description.as_deref(), Some("source"));

    let instances = list_computer_instances_core(&state).await.unwrap();
    let names = instances
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"One"));
    assert!(names.contains(&"Copy A"));
    assert!(names.contains(&"Copy B"));
    assert_eq!(instances.len(), 3);
}
