use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use socketioxide::extract::{AckSender, SocketRef};
use socketioxide::SocketIo;
use tfrobot_client_lib::commands::connection::{connect_smcp_core, ConnectionProfile};
use tfrobot_client_lib::commands::mcp::add_mcp_server_core;
use tfrobot_client_lib::commands::settings::update_settings_core;
use tfrobot_client_lib::services::settings::{AppSettings, SettingsService};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};
use tower::service_fn;
use tower::Layer;

#[allow(dead_code)]
mod common;

const CLIENT_GET_TOOLS: &str = "client:get_tools";
const SERVER_JOIN_OFFICE: &str = "server:join_office";
const SERVER_LEAVE_OFFICE: &str = "server:leave_office";
const SERVER_UPDATE_SKILLS: &str = "server:update_skills";
const CLIENT_GET_SKILLS: &str = "client:get_skills";
const CLIENT_GET_SKILL: &str = "client:get_skill";

#[derive(Default)]
struct SocketStats {
    active: AtomicUsize,
    connected: AtomicUsize,
    disconnected: AtomicUsize,
    leave_events: AtomicUsize,
    skill_update_events: AtomicUsize,
}

impl SocketStats {
    fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    fn connected(&self) -> usize {
        self.connected.load(Ordering::SeqCst)
    }

    fn disconnected(&self) -> usize {
        self.disconnected.load(Ordering::SeqCst)
    }

    fn leave_events(&self) -> usize {
        self.leave_events.load(Ordering::SeqCst)
    }

    fn skill_update_events(&self) -> usize {
        self.skill_update_events.load(Ordering::SeqCst)
    }
}

async fn start_smcp_socket_server() -> (String, Arc<SocketStats>) {
    let stats = Arc::new(SocketStats::default());
    let (socket_layer, io) = SocketIo::new_layer();

    let connect_stats = stats.clone();
    io.ns("/smcp", move |socket: SocketRef| {
        connect_stats.active.fetch_add(1, Ordering::SeqCst);
        connect_stats.connected.fetch_add(1, Ordering::SeqCst);

        socket.on(SERVER_JOIN_OFFICE, |ack: AckSender| {
            let _ = ack.send(&(true, Option::<String>::None));
        });

        let leave_stats = connect_stats.clone();
        socket.on(SERVER_LEAVE_OFFICE, move || {
            leave_stats.leave_events.fetch_add(1, Ordering::SeqCst);
        });

        let update_stats = connect_stats.clone();
        socket.on(SERVER_UPDATE_SKILLS, move || {
            update_stats
                .skill_update_events
                .fetch_add(1, Ordering::SeqCst);
        });

        let disconnect_stats = connect_stats.clone();
        socket.on_disconnect(move || {
            disconnect_stats.active.fetch_sub(1, Ordering::SeqCst);
            disconnect_stats.disconnected.fetch_add(1, Ordering::SeqCst);
        });
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));

    tokio::spawn(async move {
        let fallback = service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
            Ok::<_, Infallible>(
                hyper::Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(Full::<Bytes>::from("not found"))
                    .unwrap(),
            )
        });
        let service = socket_layer.layer(fallback);

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let service = service.clone();
            tokio::spawn(async move {
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    (url, stats)
}

async fn start_rejecting_join_smcp_socket_server() -> (String, Arc<SocketStats>) {
    let stats = Arc::new(SocketStats::default());
    let (socket_layer, io) = SocketIo::new_layer();

    let connect_stats = stats.clone();
    io.ns("/smcp", move |socket: SocketRef| {
        connect_stats.active.fetch_add(1, Ordering::SeqCst);
        connect_stats.connected.fetch_add(1, Ordering::SeqCst);

        socket.on(SERVER_JOIN_OFFICE, |ack: AckSender| {
            let _ = ack.send(&(false, Some("join rejected")));
        });

        let disconnect_stats = connect_stats.clone();
        socket.on_disconnect(move || {
            disconnect_stats.active.fetch_sub(1, Ordering::SeqCst);
            disconnect_stats.disconnected.fetch_add(1, Ordering::SeqCst);
        });
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));

    tokio::spawn(async move {
        let fallback = service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
            Ok::<_, Infallible>(
                hyper::Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(Full::<Bytes>::from("not found"))
                    .unwrap(),
            )
        });
        let service = socket_layer.layer(fallback);

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let service = service.clone();
            tokio::spawn(async move {
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    (url, stats)
}

async fn start_skill_query_smcp_socket_server(
    result_tx: oneshot::Sender<(serde_json::Value, serde_json::Value)>,
) -> String {
    let (socket_layer, io) = SocketIo::new_layer();
    let result_tx = Arc::new(std::sync::Mutex::new(Some(result_tx)));

    io.ns("/smcp", move |socket: SocketRef| {
        let tx = result_tx.clone();
        socket.on(
            SERVER_JOIN_OFFICE,
            move |socket: SocketRef, ack: AckSender| {
                let _ = ack.send(&(true, Option::<String>::None));
                let tx = tx.clone();
                tokio::spawn(async move {
                    let get_skills_req = serde_json::json!({
                        "agent": "robot",
                        "req_id": "skills-req",
                        "computer": "tfrobot-client"
                    });
                    let skills: serde_json::Value = socket
                        .emit_with_ack(CLIENT_GET_SKILLS, &get_skills_req)
                        .expect("emit get skills")
                        .await
                        .expect("get skills ack");

                    let skill_name = skills["skills"][0]["name"]
                        .as_str()
                        .expect("skill name")
                        .to_string();
                    let get_skill_req = serde_json::json!({
                        "agent": "robot",
                        "req_id": "skill-req",
                        "computer": "tfrobot-client",
                        "name": skill_name
                    });
                    let detail: serde_json::Value = socket
                        .emit_with_ack(CLIENT_GET_SKILL, &get_skill_req)
                        .expect("emit get skill")
                        .await
                        .expect("get skill ack");

                    if let Some(tx) = tx.lock().expect("tx lock").take() {
                        let _ = tx.send((skills, detail));
                    }
                });
            },
        );

        socket.on(SERVER_LEAVE_OFFICE, || {});
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));

    tokio::spawn(async move {
        let fallback = service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
            Ok::<_, Infallible>(
                hyper::Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(Full::<Bytes>::from("not found"))
                    .unwrap(),
            )
        });
        let service = socket_layer.layer(fallback);

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let service = service.clone();
            tokio::spawn(async move {
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    url
}

async fn start_tool_query_smcp_socket_server(
    result_tx: oneshot::Sender<serde_json::Value>,
) -> (String, oneshot::Sender<()>) {
    let (socket_layer, io) = SocketIo::new_layer();
    let result_tx = Arc::new(std::sync::Mutex::new(Some(result_tx)));
    let (trigger_tx, trigger_rx) = oneshot::channel();
    let trigger_rx = Arc::new(std::sync::Mutex::new(Some(trigger_rx)));

    io.ns("/smcp", move |socket: SocketRef| {
        let tx = result_tx.clone();
        let trigger = trigger_rx.clone();
        socket.on(
            SERVER_JOIN_OFFICE,
            move |socket: SocketRef, ack: AckSender| {
                let _ = ack.send(&(true, Option::<String>::None));
                let tx = tx.clone();
                let trigger = trigger.clone();
                tokio::spawn(async move {
                    let trigger_rx = trigger.lock().expect("trigger lock").take();
                    if let Some(trigger_rx) = trigger_rx {
                        let _ = trigger_rx.await;
                    }
                    let get_tools_req = serde_json::json!({
                        "agent": "robot",
                        "req_id": "tools-req",
                        "computer": "tfrobot-client"
                    });
                    let tools: serde_json::Value = socket
                        .emit_with_ack(CLIENT_GET_TOOLS, &get_tools_req)
                        .expect("emit get tools")
                        .await
                        .expect("get tools ack");

                    if let Some(tx) = tx.lock().expect("tx lock").take() {
                        let _ = tx.send(tools);
                    }
                });
            },
        );

        socket.on(SERVER_LEAVE_OFFICE, || {});
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));

    tokio::spawn(async move {
        let fallback = service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
            Ok::<_, Infallible>(
                hyper::Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(Full::<Bytes>::from("not found"))
                    .unwrap(),
            )
        });
        let service = socket_layer.layer(fallback);

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let service = service.clone();
            tokio::spawn(async move {
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    (url, trigger_tx)
}

async fn wait_for(timeout_message: &str, predicate: impl Fn() -> bool) {
    for _ in 0..40 {
        if predicate() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("{timeout_message}");
}

fn profile(name: &str, url: String) -> ConnectionProfile {
    ConnectionProfile {
        name: name.to_string(),
        url,
        namespace: "/smcp".to_string(),
        office_id: format!("{name}-office"),
        computer_name: format!("{name}-computer"),
        api_key_ref: None,
        headers: Default::default(),
        auto_connect: true,
        auto_reconnect: true,
    }
}

#[tokio::test]
async fn disconnect_smcp_closes_socket_without_rebuilding_runtime() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skill_home = tmp.path().join("skills-home");
    let demo_skill = skill_home.join("demo-skill");
    std::fs::create_dir_all(&demo_skill).expect("create skill dir");
    std::fs::write(
        demo_skill.join("SKILL.md"),
        "---\nname: demo-skill\ndescription: Demo skill\n---\n# Demo Skill\n",
    )
    .expect("write skill");
    let settings_service = SettingsService::new(tmp.path().to_path_buf());
    settings_service
        .save(&AppSettings {
            skills_root_dir: skill_home.to_string_lossy().to_string(),
            ..AppSettings::default()
        })
        .expect("save settings");
    let state = common::create_test_app_state(tmp.path());
    let (server_url, stats) = start_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[profile("lifecycle", server_url)])
        .expect("save profile");

    connect_smcp_core(&state, "lifecycle".to_string())
        .await
        .expect("connect profile");
    let before = state.runtime.computer();
    assert!(before
        .get_skills()
        .await
        .iter()
        .any(|skill| skill.name == "demo-skill"));

    wait_for("server never observed the SMCP socket connection", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    state.runtime.disconnect().await;

    wait_for("server never observed the leave_office event", || {
        stats.leave_events() == 1
    })
    .await;
    wait_for("SMCP cleanup did not close the socket", || {
        stats.active() == 0 && stats.disconnected() == 1
    })
    .await;

    assert!(state.runtime.computer().is_mcp_manager_initialized().await);
    assert!(Arc::ptr_eq(&before, &state.runtime.computer()));
    assert!(state
        .runtime
        .computer()
        .get_skills()
        .await
        .iter()
        .any(|skill| skill.name == "demo-skill"));
}

#[tokio::test]
async fn failed_skill_root_reconfigure_keeps_existing_smcp_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (server_url, stats) = start_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[profile("existing", server_url)])
        .expect("save profile");
    connect_smcp_core(&state, "existing".to_string())
        .await
        .expect("connect profile");
    wait_for("server never observed active connection", || {
        stats.active() == 1
    })
    .await;

    let invalid_skill_home = tmp.path().join("not-a-dir");
    std::fs::write(&invalid_skill_home, "file blocks skill home directory")
        .expect("write blocking file");
    let err = update_settings_core(
        &state,
        AppSettings {
            skills_root_dir: invalid_skill_home.to_string_lossy().to_string(),
            ..AppSettings::default()
        },
    )
    .await
    .expect_err("invalid skill root should fail reconfigure");

    assert!(err.contains("Not a directory") || err.contains("not a directory"));
    let status = state.runtime.connection_status().await;
    assert!(status.connected);
    assert_eq!(status.profile_name.as_deref(), Some("existing"));
    assert_eq!(stats.active(), 1);
    assert_eq!(stats.disconnected(), 0);

    state.runtime.disconnect().await;
}

#[tokio::test]
async fn successful_skill_root_sync_keeps_existing_smcp_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (server_url, stats) = start_smcp_socket_server().await;
    let new_skill_home = tmp.path().join("new-skills");
    let new_user_skill = new_skill_home.join("new-skill");
    std::fs::create_dir_all(&new_user_skill).expect("create new skill");
    std::fs::write(
        new_user_skill.join("SKILL.md"),
        "---\nname: new-skill\ndescription: New skill\n---\n# New Skill\n",
    )
    .expect("write new skill");

    state
        .config
        .save_profiles(&[profile("existing", server_url)])
        .expect("save profile");
    connect_smcp_core(&state, "existing".to_string())
        .await
        .expect("connect profile");
    wait_for("server never observed active connection", || {
        stats.active() == 1
    })
    .await;

    update_settings_core(
        &state,
        AppSettings {
            skills_root_dir: new_skill_home.to_string_lossy().to_string(),
            ..AppSettings::default()
        },
    )
    .await
    .expect("valid skill root should sync runtime skills");

    let status = state.runtime.connection_status().await;
    assert!(status.connected);
    assert_eq!(status.profile_name.as_deref(), Some("existing"));
    assert_eq!(stats.active(), 1);
    assert_eq!(stats.disconnected(), 0);
    assert_eq!(stats.leave_events(), 0);
    assert_eq!(state.runtime.local_skill_root(), new_skill_home);
    assert!(state
        .runtime
        .computer()
        .get_skills()
        .await
        .iter()
        .any(|skill| skill.name == "new-skill"));

    state.runtime.shutdown().await;
}

#[tokio::test]
async fn connecting_new_profile_closes_previous_smcp_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (first_url, first_stats) = start_smcp_socket_server().await;
    let (second_url, second_stats) = start_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[profile("first", first_url), profile("second", second_url)])
        .expect("save profiles");

    connect_smcp_core(&state, "first".to_string())
        .await
        .expect("connect first profile");
    let first_computer = state.runtime.computer();
    wait_for("first server never observed active connection", || {
        first_stats.active() == 1
    })
    .await;

    connect_smcp_core(&state, "second".to_string())
        .await
        .expect("connect second profile");
    assert!(
        Arc::ptr_eq(&first_computer, &state.runtime.computer()),
        "Switching SMCP profiles should not rebuild the Computer runtime"
    );

    wait_for("first server never observed leave_office", || {
        first_stats.leave_events() == 1
    })
    .await;
    wait_for("first SMCP socket was not closed on profile switch", || {
        first_stats.active() == 0 && first_stats.disconnected() == 1
    })
    .await;
    wait_for("second server never observed active connection", || {
        second_stats.active() == 1
    })
    .await;

    let status = state.runtime.connection_status().await;
    assert!(status.connected);
    assert_eq!(status.profile_name.as_deref(), Some("second"));
    assert_eq!(status.computer_name.as_deref(), Some("second-computer"));

    state.runtime.disconnect().await;
}

#[tokio::test]
async fn rejected_join_closes_established_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (server_url, stats) = start_rejecting_join_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[profile("reject", server_url)])
        .expect("save profile");

    let err = connect_smcp_core(&state, "reject".to_string())
        .await
        .expect_err("join rejection should fail connect");
    assert!(err.contains("Failed to join office"));

    wait_for("rejected join socket was not disconnected", || {
        stats.active() == 0 && stats.disconnected() == 1
    })
    .await;
    assert!(!state.runtime.connection_status().await.connected);
}

#[tokio::test]
async fn rejected_profile_switch_keeps_existing_smcp_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (existing_url, existing_stats) = start_smcp_socket_server().await;
    let (reject_url, reject_stats) = start_rejecting_join_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[
            profile("existing", existing_url),
            profile("reject", reject_url),
        ])
        .expect("save profiles");

    connect_smcp_core(&state, "existing".to_string())
        .await
        .expect("connect existing profile");
    wait_for("existing server never observed active connection", || {
        existing_stats.active() == 1
    })
    .await;

    let err = connect_smcp_core(&state, "reject".to_string())
        .await
        .expect_err("rejected profile should fail switch");
    assert!(err.contains("Failed to join office"));

    wait_for("rejected profile socket was not disconnected", || {
        reject_stats.active() == 0 && reject_stats.disconnected() == 1
    })
    .await;

    let status = state.runtime.connection_status().await;
    assert!(status.connected);
    assert_eq!(status.profile_name.as_deref(), Some("existing"));
    assert_eq!(existing_stats.active(), 1);
    assert_eq!(existing_stats.disconnected(), 0);

    state.runtime.disconnect().await;
}

#[tokio::test]
async fn failed_profile_lookup_keeps_existing_smcp_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (server_url, stats) = start_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[profile("existing", server_url)])
        .expect("save profile");

    connect_smcp_core(&state, "existing".to_string())
        .await
        .expect("connect existing profile");

    let err = connect_smcp_core(&state, "missing-profile".to_string())
        .await
        .expect_err("missing profile should fail");
    assert_eq!(err, "Profile not found: missing-profile");

    let status = state.runtime.connection_status().await;
    assert!(status.connected);
    assert_eq!(status.profile_name.as_deref(), Some("existing"));
    assert_eq!(stats.active(), 1);
    assert_eq!(stats.disconnected(), 0);

    state.runtime.disconnect().await;
}

#[tokio::test]
async fn robot_can_fetch_local_skill_list_and_detail_over_shared_runtime() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skill_home = tmp.path().join("skills-home");
    let demo_skill = skill_home.join("demo-skill");
    std::fs::create_dir_all(&demo_skill).expect("create skill dir");
    std::fs::write(
        demo_skill.join("SKILL.md"),
        "---\nname: demo-skill\ndescription: Demo skill for robot\n---\n# Demo Skill\n\nUse this skill.",
    )
    .expect("write skill");

    let settings_service = SettingsService::new(tmp.path().to_path_buf());
    settings_service
        .save(&AppSettings {
            skills_root_dir: skill_home.to_string_lossy().to_string(),
            ..AppSettings::default()
        })
        .expect("save settings");
    let state = common::create_test_app_state(tmp.path());

    let (tx, rx) = oneshot::channel();
    let server_url = start_skill_query_smcp_socket_server(tx).await;
    state
        .config
        .save_profiles(&[profile("skill", server_url)])
        .expect("save profile");

    connect_smcp_core(&state, "skill".to_string())
        .await
        .expect("connect profile");

    let (skills, detail) = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("robot skill query timed out")
        .expect("robot skill query result");

    assert_eq!(skills["req_id"], "skills-req");
    assert_eq!(skills["skills"][0]["name"], "demo-skill");
    assert_eq!(skills["skills"][0]["source"], "user");
    assert_eq!(skills["skills"][0]["description"], "Demo skill for robot");

    assert_eq!(detail["req_id"], "skill-req");
    assert_eq!(detail["name"], "demo-skill");
    assert_eq!(detail["rel_path"], "SKILL.md");
    assert_eq!(detail["mime_type"], "text/markdown");
    assert!(detail["body"]
        .as_str()
        .expect("skill body")
        .contains("# Demo Skill"));

    state.runtime.disconnect().await;
}

#[tokio::test]
async fn robot_can_fetch_mcp_tools_over_shared_runtime() {
    let node_available = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        node_available,
        "Node.js is required for the echo MCP server integration test"
    );

    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let config = common::echo_server_config("robot-tool-test");
    add_mcp_server_core(&state, config.clone())
        .await
        .expect("add MCP config");
    let (tx, rx) = oneshot::channel();
    let (server_url, trigger_tool_query) = start_tool_query_smcp_socket_server(tx).await;
    state
        .config
        .save_profiles(&[profile("tools", server_url)])
        .expect("save profile");

    let computer = state.runtime.computer();
    computer
        .start_mcp_client("robot-tool-test")
        .await
        .expect("start echo MCP server");

    let mut echo_tool_ready = false;
    for _ in 0..40 {
        if computer
            .get_available_tools()
            .await
            .map(|tools| tools.iter().any(|tool| tool.name == "echo"))
            .unwrap_or(false)
        {
            echo_tool_ready = true;
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert!(echo_tool_ready, "echo MCP server did not expose tools");

    connect_smcp_core(&state, "tools".to_string())
        .await
        .expect("connect profile");
    assert!(
        Arc::ptr_eq(&computer, &state.runtime.computer()),
        "SMCP connect should reuse the current Computer runtime"
    );

    let _ = trigger_tool_query.send(());

    let tools = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("robot tool query timed out")
        .expect("robot tool query result");

    assert_eq!(tools["req_id"], "tools-req");
    let tool_names: Vec<_> = tools["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(
        tool_names.contains(&"echo"),
        "robot-visible tool list should include echo, got {tool_names:?}"
    );

    state.runtime.disconnect().await;
}

#[tokio::test]
async fn skill_sync_summary_refreshes_after_local_skill_changes_without_reconnect() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skill_home = tmp.path().join("skills-home");
    let initial_skill = skill_home.join("initial-skill");
    std::fs::create_dir_all(&initial_skill).expect("create initial skill dir");
    std::fs::write(
        initial_skill.join("SKILL.md"),
        "---\nname: initial-skill\ndescription: Initial skill\n---\n# Initial Skill\n",
    )
    .expect("write initial skill");

    let settings_service = SettingsService::new(tmp.path().to_path_buf());
    let settings = AppSettings {
        skills_root_dir: skill_home.to_string_lossy().to_string(),
        ..AppSettings::default()
    };
    settings_service.save(&settings).expect("save settings");
    let state = common::create_test_app_state(tmp.path());

    state.runtime.boot().await.expect("boot runtime");
    let first = state
        .runtime
        .refresh_skill_sync_summary(&settings)
        .await
        .expect("first summary");
    assert_eq!(first.local_synced, 1);

    let added_skill = skill_home.join("added-skill");
    std::fs::create_dir_all(&added_skill).expect("create added skill dir");
    std::fs::write(
        added_skill.join("SKILL.md"),
        "---\nname: added-skill\ndescription: Added skill\n---\n# Added Skill\n",
    )
    .expect("write added skill");

    let refreshed = state
        .runtime
        .refresh_skill_sync_summary(&settings)
        .await
        .expect("refreshed summary");
    assert_eq!(refreshed.local_synced, 2);
    assert!(refreshed.skipped.is_empty());

    state.runtime.shutdown().await;
}

#[tokio::test]
async fn skill_sync_notifies_connected_smcp_server_after_local_skill_changes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skill_home = tmp.path().join("skills-home");
    let initial_skill = skill_home.join("initial-skill");
    std::fs::create_dir_all(&initial_skill).expect("create initial skill dir");
    std::fs::write(
        initial_skill.join("SKILL.md"),
        "---\nname: initial-skill\ndescription: Initial skill\n---\n# Initial Skill\n",
    )
    .expect("write initial skill");

    let settings_service = SettingsService::new(tmp.path().to_path_buf());
    let settings = AppSettings {
        skills_root_dir: skill_home.to_string_lossy().to_string(),
        ..AppSettings::default()
    };
    settings_service.save(&settings).expect("save settings");
    let state = common::create_test_app_state(tmp.path());
    let (server_url, stats) = start_smcp_socket_server().await;

    state
        .config
        .save_profiles(&[profile("skill-sync", server_url)])
        .expect("save profile");
    connect_smcp_core(&state, "skill-sync".to_string())
        .await
        .expect("connect profile");
    wait_for("server never observed active connection", || {
        stats.active() == 1
    })
    .await;
    let update_events_before_sync = stats.skill_update_events();

    let added_skill = skill_home.join("added-skill");
    std::fs::create_dir_all(&added_skill).expect("create added skill dir");
    std::fs::write(
        added_skill.join("SKILL.md"),
        "---\nname: added-skill\ndescription: Added skill\n---\n# Added Skill\n",
    )
    .expect("write added skill");

    let refreshed = state
        .runtime
        .refresh_skill_sync_summary(&settings)
        .await
        .expect("refresh summary");
    assert_eq!(refreshed.local_synced, 2);
    wait_for("server never observed skill update notification", || {
        stats.skill_update_events() > update_events_before_sync
    })
    .await;

    state.runtime.disconnect().await;
}
