//! Real WebView -> production store -> Tauri commands -> SDK -> Git acceptance.
//! The caller supplies an isolated temporary data directory and credential-free fixtures.
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::State;
use tfrobot_client_lib::commands::{marketplace, skills};
use tfrobot_client_lib::services::{
    computer::ComputerInstance, config::ConfigService, keychain::InMemorySecretStore,
    observability::ObservabilityService, settings::SettingsService,
};
use tfrobot_client_lib::AppState;

struct Fixture {
    remote_url: String,
    local_path: String,
}
#[tauri::command]
fn acceptance_fixture(fixture: State<'_, Fixture>) -> Value {
    json!({"remoteUrl": fixture.remote_url, "localPath": fixture.local_path})
}
#[tauri::command]
fn acceptance_result(app: tauri::AppHandle, passed: bool, observations: Vec<String>) {
    println!(
        "ACCEPTANCE:{}",
        json!({"passed": passed, "observations": observations})
    );
    app.exit(if passed { 0 } else { 1 });
}
fn main() {
    let root =
        PathBuf::from(std::env::var_os("MARKETPLACE_IPC_DATA").expect("isolated data required"));
    let state = AppState::new_with_secret_store(
        ConfigService::new(root.clone()).unwrap(),
        ObservabilityService::new(&root).unwrap(),
        SettingsService::new(root),
        InMemorySecretStore::shared(),
    );
    let instance = ComputerInstance::new("marketplace-acceptance", "Acceptance");
    state
        .config
        .add_computer_instance(instance.clone())
        .unwrap();
    tauri::async_runtime::block_on(state.computer_registry.upsert_runtime(instance)).unwrap();
    tauri::Builder::default()
        .manage(state)
        .manage(Fixture {
            remote_url: std::env::var("MARKETPLACE_IPC_REMOTE").unwrap(),
            local_path: std::env::var("MARKETPLACE_IPC_LOCAL").unwrap(),
        })
        .invoke_handler(tauri::generate_handler![
            acceptance_fixture,
            acceptance_result,
            marketplace::add_marketplace,
            marketplace::update_marketplace,
            marketplace::get_marketplace_governance,
            skills::list_skills
        ])
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Issue 88 Marketplace IPC acceptance")
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!(
            "../e2e/marketplace-ipc/tauri.conf.json"
        ))
        .expect("native Marketplace IPC acceptance");
}
