//! Native restart harness: production Chat UI, IPC transport and SettingsService.
//! Only Manager identity/session acquisition is a local fixture. Never reads user credentials.
use serde_json::{json, Value};
use std::io::{self, BufRead};
use tauri::{Manager, State};
use tfrobot_client_lib::services::{
    manager_context::ManagerContextKey, manager_environment::ManagerEnvironment,
    settings::SettingsService,
};
fn context() -> ManagerContextKey {
    ManagerContextKey {
        environment: ManagerEnvironment::Staging,
        account_id: "fixture".into(),
        organization_id: "fixture".into(),
    }
}
fn emit(event: Value) {
    println!("ACCEPTANCE:{event}");
}
#[tauri::command]
fn acceptance_event(event: Value) {
    emit(event);
}
#[tauri::command]
fn chat_open_session(employee_id: u64) -> Value {
    emit(json!({"kind":"opened","employeeId":employee_id}));
    json!({"leaseId":employee_id.to_string(),"employeeId":employee_id,"robotName":format!("Robot {employee_id}"),
    "httpBaseUrl":"http://127.0.0.1:18767/", "socketNamespaceUrl":"http://127.0.0.1:18767/chat", "socketPath":"/socket.io"})
}
#[tauri::command]
fn chat_get_recent_robot(state: State<'_, SettingsService>) -> Option<u64> {
    state.load_recent_chat_employee(&context()).unwrap()
}
#[tauri::command]
fn chat_remember_robot(state: State<'_, SettingsService>, lease_id: String) {
    state
        .save_recent_chat_employee(&context(), lease_id.parse().unwrap())
        .unwrap();
}
#[tauri::command]
fn chat_get_recent_conversation(
    state: State<'_, SettingsService>,
    lease_id: String,
) -> Option<String> {
    state
        .load_recent_chat_conversation(&context(), lease_id.parse().unwrap())
        .unwrap()
}
#[tauri::command]
fn chat_remember_conversation(
    state: State<'_, SettingsService>,
    lease_id: String,
    conversation_id: String,
) {
    state
        .save_recent_chat_conversation(&context(), lease_id.parse().unwrap(), &conversation_id)
        .unwrap();
    emit(json!({"kind":"saved","employeeId":lease_id,"conversationId":conversation_id}));
}
#[tauri::command]
fn chat_close_session() {}
#[tauri::command]
fn chat_get_session_token() -> Value {
    json!({"token":"fixture","expiresAt":4102444800000u64})
}
#[tauri::command]
async fn chat_http_request(url: String, method: String) -> Result<Value, String> {
    if !url.starts_with("http://127.0.0.1:18767/") || method != "GET" {
        return Err("Local GET only".into());
    }
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| e.to_string())?;
    Ok(json!({"status":status,"body":body,"contentType":"application/json"}))
}
fn main() {
    let root = std::env::var_os("CHAT_RESTORE_DATA").expect("isolated data directory required");
    tauri::Builder::default()
        .manage(SettingsService::new(root.into()))
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            acceptance_event,
            chat_open_session,
            chat_get_recent_robot,
            chat_remember_robot,
            chat_get_recent_conversation,
            chat_remember_conversation,
            chat_close_session,
            chat_get_session_token,
            chat_http_request
        ])
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Issue 81 restart acceptance")
            .inner_size(1200., 800.)
            .build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for line in io::stdin().lock().lines() {
                    let line = line.unwrap();
                    if line == "quit" {
                        handle.exit(0);
                        break;
                    }
                    let h = handle.clone();
                    handle
                        .run_on_main_thread(move || {
                            h.get_webview_window("main").unwrap().eval(line).unwrap();
                        })
                        .unwrap();
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!(
            "../e2e/chat-restoration/tauri.conf.json"
        ))
        .expect("native restart acceptance");
}
