mod common;

use common::{create_test_app_state, echo_server_config};
use tfrobot_client_lib::commands::computer::{
    create_computer_instance_core, list_computer_instances_core, CreateComputerInstanceRequest,
};
use tfrobot_client_lib::commands::inputs::{set_input_value_core, InputDefinition};
use tfrobot_client_lib::commands::portable_config::{
    export_computer_package_core, inspect_computer_package_core,
};
use tfrobot_client_lib::commands::sdk_config::upsert_computer_mcp_config_with_inputs_core;
use tfrobot_client_lib::services::input_value_store::InputValueStore;
use tfrobot_client_lib::services::portable_config::{
    parse_package, PackageGroup, PORTABLE_PACKAGE_FORMAT_VERSION,
};

fn package_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("computer-package.json")
}

fn create_request(
    name: &str,
    description: Option<&str>,
    import_path: Option<&std::path::Path>,
) -> CreateComputerInstanceRequest {
    CreateComputerInstanceRequest {
        name: name.to_string(),
        description: description.map(str::to_string),
        import_path: import_path.map(|path| path.to_string_lossy().into_owned()),
    }
}

async fn export_source(state: &tfrobot_client_lib::AppState, id: &str, path: &std::path::Path) {
    export_computer_package_core(
        state,
        id,
        path.to_str().unwrap(),
        Some(PackageGroup::ALL.to_vec()),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn export_produces_a_versioned_package_and_inspection_round_trips_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created =
        create_computer_instance_core(&state, create_request("One", Some("portable source"), None))
            .await
            .unwrap();

    let path = package_path(dir.path());
    export_source(&state, &created.id, &path).await;

    let package = parse_package(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(package.format_version, PORTABLE_PACKAGE_FORMAT_VERSION);
    assert_eq!(package.manifest.source_computer_name, "One");
    assert_eq!(package.groups.len(), PackageGroup::ALL.len());
    assert!(package.profile.is_some());

    let inspection = inspect_computer_package_core(&state, path.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(inspection.original_name, "One");
    assert_eq!(inspection.description.as_deref(), Some("portable source"));
    assert_eq!(inspection.groups, PackageGroup::ALL.to_vec());
}

#[tokio::test]
async fn create_with_import_applies_profile_and_allows_duplicate_names() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created =
        create_computer_instance_core(&state, create_request("One", Some("portable source"), None))
            .await
            .unwrap();

    let path = package_path(dir.path());
    export_source(&state, &created.id, &path).await;

    // Duplicate names are allowed: the id is what identifies a Computer.
    let imported = create_computer_instance_core(
        &state,
        create_request("One", Some("overridden description"), Some(&path)),
    )
    .await
    .unwrap();

    assert_ne!(imported.id, created.id);
    assert_eq!(imported.name, "One");
    assert_eq!(
        imported.description.as_deref(),
        Some("overridden description")
    );

    let instances = list_computer_instances_core(&state).await.unwrap();
    assert_eq!(instances.len(), 2);
}

#[tokio::test]
async fn round_trip_preserves_sdk_config_and_non_sensitive_input_values() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());

    let created = create_computer_instance_core(
        &state,
        create_request("One", Some("round-trip source"), None),
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
    export_source(&state, &created.id, &path).await;

    let package = parse_package(&std::fs::read(&path).unwrap()).unwrap();
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

    let imported = create_computer_instance_core(&state, create_request("Two", None, Some(&path)))
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
    let created = create_computer_instance_core(&state, create_request("One", None, None))
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
    let created = create_computer_instance_core(&state, create_request("One", None, None))
        .await
        .unwrap();

    let path = package_path(dir.path());
    export_source(&state, &created.id, &path).await;

    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["formatVersion"] = serde_json::json!(99);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let inspect_error = inspect_computer_package_core(&state, path.to_str().unwrap())
        .await
        .unwrap_err();
    assert!(inspect_error.contains("unsupported package format version"));

    let create_error =
        create_computer_instance_core(&state, create_request("Blocked", None, Some(&path)))
            .await
            .unwrap_err();
    assert!(create_error.contains("unsupported package format version"));

    let instances = list_computer_instances_core(&state).await.unwrap();
    assert_eq!(instances.len(), 1);
}

#[tokio::test]
async fn create_with_import_of_a_structurally_invalid_sdk_config_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_app_state(dir.path());
    let created = create_computer_instance_core(&state, create_request("One", None, None))
        .await
        .unwrap();

    let path = package_path(dir.path());
    export_source(&state, &created.id, &path).await;

    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["sdkConfig"]["mcp"] = serde_json::json!({ "servers": "not-an-object" });
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let error = create_computer_instance_core(&state, create_request("Blocked", None, Some(&path)))
        .await
        .unwrap_err();
    assert!(error.contains("invalid SDK configuration"));

    let instances = list_computer_instances_core(&state).await.unwrap();
    assert_eq!(instances.len(), 1);
}
