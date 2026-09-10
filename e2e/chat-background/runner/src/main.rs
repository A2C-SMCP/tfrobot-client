use serde_json::{json, Value};
use std::io::{self, BufRead};
use tauri::Manager;

// Fixture credentials and HTTP relay, never the user's Manager/Keychain session.
#[tauri::command]
fn chat_open_session() -> Value {
    json!({"leaseId":"fixture","employeeId":42,"robotName":"Fixture Robot",
        "httpBaseUrl":"http://127.0.0.1:18766/api/", "socketNamespaceUrl":"http://127.0.0.1:18766/chat", "socketPath":"/socket.io"})
}
#[tauri::command]
fn chat_get_recent_robot() -> u32 {
    42
}
#[tauri::command]
fn chat_remember_robot() {}
#[tauri::command]
fn chat_close_session() {}
#[tauri::command]
fn chat_get_session_token() -> Value {
    json!({"token":"fixture","expiresAt":4102444800000u64})
}
#[tauri::command]
async fn chat_http_request(url: String, method: String) -> Result<Value, String> {
    if !url.starts_with("http://127.0.0.1:18766/api/") || method != "GET" {
        return Err("Fixture permits local GET only".into());
    }
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| e.to_string())?;
    Ok(json!({"status":status,"body":body,"contentType":"application/json"}))
}
fn main() {
    // Consume the production window configuration, including the policy under test.
    let config: tauri::utils::config::Config =
        serde_json::from_str(include_str!("../../../../src-tauri/tauri.conf.json")).unwrap();
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            chat_open_session,
            chat_get_recent_robot,
            chat_remember_robot,
            chat_close_session,
            chat_get_session_token,
            chat_http_request
        ])
        .setup(move |app| {
            tauri::WebviewWindowBuilder::from_config(app, &config.app.windows[0])?.build()?;
            println!("{}", json!({"kind":"native_ready"}));
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // Blocking stdin is event driven; no timer or repeated WebView IPC.
                for line in io::stdin().lock().lines() {
                    let line = line.unwrap();
                    let h = handle.clone();
                    handle
                        .run_on_main_thread(move || {
                            let window = h.get_webview_window("main").unwrap();
                            let result = match line.as_str() {
                                "hide" => window.hide(),
                                "minimize" => window.minimize(),
                                "show" => window.unminimize().and_then(|_| window.show()),
                                "quit" => {
                                    h.exit(0);
                                    Ok(())
                                }
                                name => window.eval(format!(
                                    "window.backgroundSnapshot({})",
                                    serde_json::to_string(name).unwrap()
                                )),
                            };
                            println!(
                                "{}",
                                json!({"kind":"native_action","action":line,"ok":result.is_ok()})
                            );
                        })
                        .unwrap();
                }
                handle.exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("acceptance runner");
}
