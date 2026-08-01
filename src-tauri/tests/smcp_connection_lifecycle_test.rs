use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use a2c_smcp::smcp_computer::mcp_clients::model::BundleId;
use a2c_smcp::{events, A2CSkillRef, AgentCallData, GetSkillReq, GetSkillsReq, ReqId, Role};
use futures_util::FutureExt;
use http_body_util::Full;
use hyper::body::Bytes;
use serde_json::{json, Value};
use smcp_server_core::{DefaultAuthenticationProvider, SmcpServerBuilder};
use socketioxide::extract::{AckSender, SocketRef};
use socketioxide::SocketIo;
use tf_rust_socketio::asynchronous::{Client, ClientBuilder};
use tf_rust_socketio::{Payload, TransportType};
use tfrobot_client_lib::commands::computer::{
    delete_computer_instance_core, rename_computer_instance_core, stop_computer_instance_core,
    RenameComputerInstanceRequest,
};
use tfrobot_client_lib::commands::connection::{
    close_smcp_connection, connect_connection_target_core, delete_manual_smcp_target_core,
    reconnect_with_token, try_install_refreshed_client, ConnectionState, ManagerConnectionParams,
    RefreshOutcome, SwapResult,
};
use tfrobot_client_lib::services::computer::{
    ClientConnectionStatus, ComputerInstance, ComputerInstanceRuntime, ComputerRuntimeState,
    RobotBindingMetadata,
};
use tfrobot_client_lib::services::computer_runtime_events::{
    ComputerRuntimeAffectedCapability, ComputerRuntimeProblemMessage,
    ComputerRuntimeProblemSeverity, ComputerRuntimeProblemSource,
};
use tfrobot_client_lib::services::connection_targets::ManualSmcpTarget;
use tfrobot_client_lib::services::manager_client::ExchangedToken;
use tfrobot_client_lib::AppState;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{sleep, Duration};
use tower::service_fn;
use tower::Layer;

#[allow(dead_code)]
mod common;

const SERVER_JOIN_OFFICE: &str = "server:join_office";
const SERVER_LEAVE_OFFICE: &str = "server:leave_office";
const TEST_INSTANCE_ID: &str = "test-computer";
const TEST_OFFICE_ID: &str = "skills-office";
const TEST_AGENT_NAME: &str = "skills-agent";
const TEST_RELAY_TOKEN: &str = "skills-relay-token";

async fn create_test_runtime(state: &AppState) -> ComputerInstanceRuntime {
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Test Computer"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(TEST_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap()
}

#[derive(Default)]
struct SocketStats {
    active: AtomicUsize,
    connected: AtomicUsize,
    disconnected: AtomicUsize,
    leave_events: AtomicUsize,
    join_events: AtomicUsize,
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

    fn join_events(&self) -> usize {
        self.join_events.load(Ordering::SeqCst)
    }
}

async fn start_smcp_socket_server() -> (String, Arc<SocketStats>) {
    start_smcp_socket_server_with_join_delay(Duration::ZERO).await
}

async fn start_smcp_socket_server_with_join_delay(
    join_delay: Duration,
) -> (String, Arc<SocketStats>) {
    let stats = Arc::new(SocketStats::default());
    let (socket_layer, io) = SocketIo::new_layer();

    let connect_stats = stats.clone();
    io.ns("/smcp", move |socket: SocketRef| {
        connect_stats.active.fetch_add(1, Ordering::SeqCst);
        connect_stats.connected.fetch_add(1, Ordering::SeqCst);

        let join_stats = connect_stats.clone();
        socket.on(SERVER_JOIN_OFFICE, move |ack: AckSender| async move {
            join_stats.join_events.fetch_add(1, Ordering::SeqCst);
            sleep(join_delay).await;
            let _ = ack.send(&(true, Option::<String>::None));
        });

        let leave_stats = connect_stats.clone();
        socket.on(SERVER_LEAVE_OFFICE, move || {
            leave_stats.leave_events.fetch_add(1, Ordering::SeqCst);
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

async fn wait_for(timeout_message: &str, predicate: impl Fn() -> bool) {
    for _ in 0..40 {
        if predicate() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("{timeout_message}");
}

struct RelayServer {
    url: String,
    shutdown_tx: oneshot::Sender<()>,
}

impl RelayServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("local_addr"));
        let layer = SmcpServerBuilder::new()
            .with_auth_provider(Arc::new(DefaultAuthenticationProvider::new(
                Some(TEST_RELAY_TOKEN.to_string()),
                None,
            )))
            .build_layer()
            .expect("build SMCP relay layer");
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let fallback = service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
            Ok::<_, Infallible>(hyper::Response::new(Full::new(Bytes::new())))
        });
        let service = layer.layer.layer(fallback);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        if let Ok((stream, _)) = result {
                            let stream = hyper_util::rt::TokioIo::new(stream);
                            let service = hyper_util::service::TowerToHyperService::new(service.clone());
                            tokio::spawn(async move {
                                let _ = hyper::server::conn::http1::Builder::new()
                                    .serve_connection(stream, service)
                                    .with_upgrades()
                                    .await;
                            });
                        }
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });

        sleep(Duration::from_millis(150)).await;
        Self { url, shutdown_tx }
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

async fn agent_client(server_url: &str) -> Client {
    let client = ClientBuilder::new(server_url.to_string())
        .transport_type(TransportType::Websocket)
        .namespace("smcp")
        .auth(json!({ "token": TEST_RELAY_TOKEN }))
        .connect()
        .await
        .expect("agent connect");
    sleep(Duration::from_millis(200)).await;
    client
}

async fn agent_client_with_skill_update_listener(
    server_url: &str,
) -> (Client, oneshot::Receiver<Value>) {
    let (tx, rx) = oneshot::channel::<Value>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let client = ClientBuilder::new(server_url.to_string())
        .transport_type(TransportType::Websocket)
        .namespace("smcp")
        .auth(json!({ "token": TEST_RELAY_TOKEN }))
        .on(
            events::NOTIFY_UPDATE_SKILLS,
            move |payload: Payload, _client| {
                let tx = tx.clone();
                async move {
                    let value = match payload {
                        Payload::Text(values, _) => {
                            values.into_iter().next().unwrap_or(Value::Null)
                        }
                        _ => Value::Null,
                    };
                    if let Some(tx) = tx.lock().await.take() {
                        let _ = tx.send(value);
                    }
                }
                .boxed()
            },
        )
        .connect()
        .await
        .expect("agent connect");
    sleep(Duration::from_millis(200)).await;
    (client, rx)
}

fn ack_cb(
    tx: oneshot::Sender<Value>,
) -> impl FnMut(Payload, Client) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync {
    let tx = Arc::new(Mutex::new(Some(tx)));
    move |payload: Payload, _client: Client| {
        let tx = tx.clone();
        async move {
            let value = match payload {
                Payload::Text(mut values, _) => values.pop().unwrap_or(Value::Null),
                _ => Value::Null,
            };
            if let Some(tx) = tx.lock().await.take() {
                let _ = tx.send(flat_ack(value));
            }
        }
        .boxed()
    }
}

fn flat_ack(value: Value) -> Value {
    match value {
        Value::Array(mut values) if values.len() == 1 => values.pop().unwrap_or(Value::Null),
        other => other,
    }
}

async fn join_agent(client: &Client, office_id: &str, agent_name: &str) {
    let (tx, rx) = oneshot::channel::<Value>();
    client
        .emit_with_ack(
            SERVER_JOIN_OFFICE,
            json!({
                "role": Role::Agent.to_string(),
                "office_id": office_id,
                "name": agent_name,
            }),
            Duration::from_secs(10),
            ack_cb(tx),
        )
        .await
        .expect("agent join emit");
    let ack = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("agent join ack timeout")
        .expect("agent join ack channel");
    let ok = ack
        .as_array()
        .and_then(|values| values.first())
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(ok, "agent join failed: {ack}");
}

async fn emit_agent_call(client: &Client, event: &str, payload: Value) -> Value {
    let (tx, rx) = oneshot::channel::<Value>();
    client
        .emit_with_ack(event, payload, Duration::from_secs(15), ack_cb(tx))
        .await
        .expect("agent emit_with_ack");
    tokio::time::timeout(Duration::from_secs(15), rx)
        .await
        .expect("agent call ack timeout")
        .expect("agent call ack channel")
}

async fn agent_get_skills(client: &Client, req_id: &str, computer_name: &str) -> Value {
    emit_agent_call(
        client,
        events::CLIENT_GET_SKILLS,
        json!(GetSkillsReq {
            base: AgentCallData {
                agent: TEST_AGENT_NAME.to_string(),
                req_id: ReqId(req_id.to_string()),
            },
            computer: computer_name.to_string(),
        }),
    )
    .await
}

async fn wait_for_skill_update_notification(
    rx: oneshot::Receiver<Value>,
    expected_computer: &str,
) -> Value {
    let notification = tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("notify:update_skills timeout")
        .expect("notify:update_skills channel");
    assert_eq!(
        notification["computer"],
        json!(expected_computer),
        "notify:update_skills should identify the refreshed Computer"
    );
    notification
}

fn skill_names(skills: &Value) -> Vec<&str> {
    skills["skills"]
        .as_array()
        .map(|skills| {
            skills
                .iter()
                .filter_map(|skill| skill["name"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

async fn wait_for_agent_skill(
    client: &Client,
    computer_name: &str,
    skill_name: &str,
    req_id_prefix: &str,
) -> Value {
    let mut last = Value::Null;
    for attempt in 0..20 {
        let skills =
            agent_get_skills(client, &format!("{req_id_prefix}-{attempt}"), computer_name).await;
        assert!(
            skills.get("code").is_none(),
            "client:get_skills returned protocol error: {skills}"
        );
        if skill_names(&skills).contains(&skill_name) {
            return skills;
        }
        last = skills;
        sleep(Duration::from_millis(100)).await;
    }
    panic!("{skill_name} did not appear in Agent skills, last response: {last}");
}

fn write_skill(skill_dir: &Path, name: &str, description: &str, body: &str) {
    std::fs::create_dir_all(skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n"),
    )
    .unwrap();
}

fn write_user_skill(skill_home: &Path, name: &str, description: &str, body: &str) {
    write_skill(&skill_home.join("user").join(name), name, description, body);
}

fn find_skill<'a>(listed: &'a [Value], name: &str) -> &'a Value {
    listed
        .iter()
        .find(|skill| skill["name"] == json!(name))
        .unwrap_or_else(|| panic!("{name} should be visible, got: {listed:?}"))
}

fn skill_ref(name: &str, source: &str, description: &str, path: &Path) -> A2CSkillRef {
    A2CSkillRef {
        name: name.to_string(),
        source: source.to_string(),
        uri: None,
        path: path.to_string_lossy().into_owned(),
        description: description.to_string(),
        license: None,
        compatibility: None,
        allowed_tools: None,
        version: None,
        skill_metadata: None,
    }
}

async fn agent_get_skill_body(
    client: &Client,
    req_id: &str,
    computer_name: &str,
    name: &str,
) -> String {
    let skill = emit_agent_call(
        client,
        events::CLIENT_GET_SKILL,
        json!(GetSkillReq {
            base: AgentCallData {
                agent: TEST_AGENT_NAME.to_string(),
                req_id: ReqId(req_id.to_string()),
            },
            computer: computer_name.to_string(),
            name: name.to_string(),
            rel_path: None,
        }),
    )
    .await;
    assert!(
        skill.get("code").is_none(),
        "client:get_skill returned protocol error: {skill}"
    );
    assert!(skill["blob_handle"].is_null());
    skill["body"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn agent_get_skills_and_get_skill_use_connected_instance_sdk_registry() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    let skill_home = runtime.sdk_skill_home().await;
    write_user_skill(
        &skill_home,
        "agent-helper",
        "Agent visible helper",
        "AGENT-HELPER-BODY",
    );
    let marketplace_skill_dir = skill_home
        .join("marketplace")
        .join("tf-market")
        .join("desktop-tools")
        .join("market-skill");
    write_skill(
        &marketplace_skill_dir,
        "market-skill",
        "Marketplace visible helper",
        "MARKETPLACE-BODY",
    );
    let mcp_skill_dir = skill_home
        .join("mcp")
        .join("tfrobot-tools")
        .join("mcp-skill");
    write_skill(
        &mcp_skill_dir,
        "mcp-skill",
        "MCP visible helper",
        "MCP-BODY",
    );
    runtime.start().await.expect("start runtime");
    runtime
        .register_sdk_skill_ref_for_test(skill_ref(
            "desktop-tools:market-skill",
            "marketplace:tf-market",
            "Marketplace visible helper",
            &marketplace_skill_dir,
        ))
        .await;
    runtime
        .register_sdk_skill_ref_for_test(skill_ref(
            "mcp:tfrobot-tools:mcp-skill",
            "mcp:tfrobot-tools",
            "MCP visible helper",
            &mcp_skill_dir,
        ))
        .await;

    let relay = RelayServer::start().await;
    runtime
        .connect_smcp_socketio(
            relay.url(),
            Some(json!({ "token": TEST_RELAY_TOKEN })),
            HashMap::new(),
            Some("smcp".to_string()),
            TEST_OFFICE_ID,
            "Test Computer",
        )
        .await
        .expect("connect SDK Computer to relay");

    let agent = agent_client(relay.url()).await;
    join_agent(&agent, TEST_OFFICE_ID, TEST_AGENT_NAME).await;

    let skills = agent_get_skills(&agent, "skills-1", "Test Computer").await;
    assert!(
        skills.get("code").is_none(),
        "client:get_skills returned protocol error: {skills}"
    );
    let listed = skills["skills"].as_array().cloned().unwrap_or_default();
    let helper = find_skill(&listed, "agent-helper");
    assert_eq!(helper["source"], json!("user"));
    assert_eq!(helper["description"], json!("Agent visible helper"));

    let market_helper = find_skill(&listed, "desktop-tools:market-skill");
    assert_eq!(market_helper["source"], json!("marketplace:tf-market"));
    assert_eq!(
        market_helper["description"],
        json!("Marketplace visible helper")
    );
    let mcp_helper = find_skill(&listed, "mcp:tfrobot-tools:mcp-skill");
    assert_eq!(mcp_helper["source"], json!("mcp:tfrobot-tools"));
    assert_eq!(mcp_helper["description"], json!("MCP visible helper"));
    assert!(
        listed.iter().all(|skill| {
            let name = skill["name"].as_str().unwrap_or_default();
            let source = skill["source"].as_str().unwrap_or_default();
            !name.contains("tf-market_desktop-tools")
                && !name.contains("mcp_tfrobot-tools")
                && !source.contains("tf-market_desktop-tools")
                && !source.contains("mcp_tfrobot-tools")
        }),
        "Agent skills must keep SDK/SMCP native names and sources: {listed:?}"
    );

    let body = agent_get_skill_body(&agent, "skill-1", "Test Computer", "agent-helper").await;
    assert!(body.contains("AGENT-HELPER-BODY"), "body was: {body}");
    assert!(
        !body.contains("name: agent-helper"),
        "SDK should strip SKILL.md frontmatter, got: {body}"
    );
    let marketplace_body = agent_get_skill_body(
        &agent,
        "skill-marketplace",
        "Test Computer",
        "desktop-tools:market-skill",
    )
    .await;
    assert!(
        marketplace_body.contains("MARKETPLACE-BODY"),
        "body was: {marketplace_body}"
    );
    let mcp_body = agent_get_skill_body(
        &agent,
        "skill-mcp",
        "Test Computer",
        "mcp:tfrobot-tools:mcp-skill",
    )
    .await;
    assert!(mcp_body.contains("MCP-BODY"), "body was: {mcp_body}");

    agent.disconnect().await.unwrap();
    runtime.clear_smcp_connection().await.unwrap();
    runtime.shutdown().await;
    relay.shutdown();
}

#[tokio::test]
async fn agent_get_skills_is_scoped_to_connected_computer_instance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime_one = create_test_runtime(&state).await;
    write_user_skill(
        &runtime_one.sdk_skill_home().await,
        "one-only",
        "Only visible from instance one",
        "ONE-BODY",
    );
    state
        .config
        .add_computer_instance(ComputerInstance::new("second-computer", "Second Computer"))
        .unwrap();
    let runtime_two = state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance("second-computer")
                .unwrap(),
        )
        .await
        .unwrap();
    write_user_skill(
        &runtime_two.sdk_skill_home().await,
        "two-only",
        "Only visible from instance two",
        "TWO-BODY",
    );
    runtime_one.start().await.expect("start first runtime");
    runtime_two.start().await.expect("start second runtime");

    let relay = RelayServer::start().await;
    runtime_one
        .connect_smcp_socketio(
            relay.url(),
            Some(json!({ "token": TEST_RELAY_TOKEN })),
            HashMap::new(),
            Some("smcp".to_string()),
            TEST_OFFICE_ID,
            "Test Computer",
        )
        .await
        .expect("connect first SDK Computer to relay");

    let agent = agent_client(relay.url()).await;
    join_agent(&agent, TEST_OFFICE_ID, TEST_AGENT_NAME).await;
    let skills = agent_get_skills(&agent, "skills-scope", "Test Computer").await;
    assert!(
        skills.get("code").is_none(),
        "client:get_skills returned protocol error: {skills}"
    );
    let names = skill_names(&skills);
    assert!(names.contains(&"one-only"), "skills were: {names:?}");
    assert!(
        !names.contains(&"two-only"),
        "unconnected instance skill leaked into Agent protocol: {names:?}"
    );

    agent.disconnect().await.unwrap();
    runtime_one.clear_smcp_connection().await.unwrap();
    runtime_one.shutdown().await;
    runtime_two.shutdown().await;
    relay.shutdown();
}

#[tokio::test]
async fn agent_get_skills_reflects_connected_instance_skill_registry_refresh() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    let skill_home = runtime.sdk_skill_home().await;
    write_user_skill(
        &skill_home,
        "initial-helper",
        "Initially visible helper",
        "INITIAL-BODY",
    );
    runtime.start().await.expect("start runtime");

    let relay = RelayServer::start().await;
    runtime
        .connect_smcp_socketio(
            relay.url(),
            Some(json!({ "token": TEST_RELAY_TOKEN })),
            HashMap::new(),
            Some("smcp".to_string()),
            TEST_OFFICE_ID,
            "Test Computer",
        )
        .await
        .expect("connect SDK Computer to relay");

    let (agent, skill_update_rx) = agent_client_with_skill_update_listener(relay.url()).await;
    join_agent(&agent, TEST_OFFICE_ID, TEST_AGENT_NAME).await;

    let initial = wait_for_agent_skill(&agent, "Test Computer", "initial-helper", "initial").await;
    assert!(
        !skill_names(&initial).contains(&"runtime-added-helper"),
        "runtime-added-helper should not be visible before refresh: {initial}"
    );

    write_user_skill(
        &skill_home,
        "runtime-added-helper",
        "Visible after connected registry refresh",
        "RUNTIME-ADDED-BODY",
    );
    runtime.mark_sdk_skills_dirty().await;
    wait_for_skill_update_notification(skill_update_rx, "Test Computer").await;

    let refreshed =
        wait_for_agent_skill(&agent, "Test Computer", "runtime-added-helper", "refreshed").await;
    let added = refreshed["skills"]
        .as_array()
        .and_then(|skills| {
            skills
                .iter()
                .find(|skill| skill["name"] == json!("runtime-added-helper"))
        })
        .unwrap_or_else(|| panic!("runtime-added-helper missing after refresh: {refreshed}"));
    assert_eq!(added["source"], json!("user"));
    assert_eq!(
        added["description"],
        json!("Visible after connected registry refresh")
    );

    let skill = emit_agent_call(
        &agent,
        events::CLIENT_GET_SKILL,
        json!(GetSkillReq {
            base: AgentCallData {
                agent: TEST_AGENT_NAME.to_string(),
                req_id: ReqId("runtime-added-skill".to_string()),
            },
            computer: "Test Computer".to_string(),
            name: "runtime-added-helper".to_string(),
            rel_path: None,
        }),
    )
    .await;
    assert!(
        skill.get("code").is_none(),
        "client:get_skill returned protocol error: {skill}"
    );
    let body = skill["body"].as_str().unwrap_or_default();
    assert!(body.contains("RUNTIME-ADDED-BODY"), "body was: {body}");

    agent.disconnect().await.unwrap();
    runtime.clear_smcp_connection().await.unwrap();
    runtime.shutdown().await;
    relay.shutdown();
}

#[tokio::test]
async fn close_smcp_connection_closes_underlying_socket_after_leaving_office() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "lifecycle-office",
            "lifecycle-test-computer",
        )
        .await
        .expect("connect smcp socket");

    wait_for("server never observed the SMCP socket connection", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    let connection = ConnectionState {
        profile_name: "lifecycle-profile".to_string(),
        url: server_url.clone(),
        office_id: "lifecycle-office".to_string(),
        computer_name: "lifecycle-test-computer".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manual_smcp".to_string(),
        target_id: Some("lifecycle-target".to_string()),
        target_name: Some("lifecycle-profile".to_string()),
        employee_id: None,
        generation: 0,
    };
    *runtime.connection_handle_for_test().write_owned().await = Some(connection.clone());
    runtime
        .leave_office_for_test()
        .await
        .expect("leave office while preserving the client connection snapshot");
    assert_eq!(
        runtime.runtime_state().await,
        ComputerRuntimeState::Connected
    );
    assert!(
        !runtime.is_connected().await,
        "transport Connected must not be exposed as a joined business connection"
    );
    assert!(runtime.connection_status().await.is_none());

    close_smcp_connection(&runtime, connection)
        .await
        .expect("close smcp socket");

    assert!(runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_none());
    assert!(!runtime.is_connected().await);
    assert_eq!(runtime.runtime_state().await, ComputerRuntimeState::Started);

    wait_for("server never observed the leave_office event", || {
        stats.leave_events() == 1
    })
    .await;

    wait_for(
        "SMCP cleanup did not close the underlying Socket.IO connection",
        || stats.active() == 0 && stats.disconnected() == 1,
    )
    .await;

    sleep(Duration::from_millis(250)).await;
    assert_eq!(
        stats.active(),
        0,
        "SMCP socket should remain disconnected after cleanup"
    );
    assert_eq!(
        stats.disconnected(),
        1,
        "SMCP cleanup should not trigger a reconnect cycle"
    );
}

#[tokio::test]
async fn failed_delete_quarantine_preserves_joined_runtime_and_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "rollback-office",
            "rollback-computer",
        )
        .await
        .expect("connect smcp socket");
    *runtime.connection_handle_for_test().write_owned().await = Some(ConnectionState {
        profile_name: "rollback-profile".to_string(),
        url: server_url,
        office_id: "rollback-office".to_string(),
        computer_name: "rollback-computer".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manual_smcp".to_string(),
        target_id: Some("rollback-target".to_string()),
        target_name: Some("rollback-profile".to_string()),
        employee_id: None,
        generation: 1,
    });
    wait_for("server never observed rollback socket", || {
        stats.active() == 1
    })
    .await;
    assert!(runtime.is_connected().await);
    runtime
        .set_refresh_task(tokio::spawn(std::future::pending()))
        .await;
    assert!(runtime.has_refresh_task_for_test().await);
    let incarnation = runtime.runtime_snapshot().await.incarnation;

    let storage_root = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID);
    std::fs::create_dir_all(&storage_root).expect("create instance storage");
    let trash_blocker = storage_root.parent().unwrap().join(".trash");
    std::fs::write(&trash_blocker, b"block trash directory creation")
        .expect("create trash blocker");

    let error = delete_computer_instance_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .expect_err("quarantine should fail");
    assert!(error.contains("Failed to create Computer storage trash"));

    let preserved = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("failed deletion should preserve runtime");
    assert_eq!(preserved.runtime_snapshot().await.incarnation, incarnation);
    assert_eq!(
        preserved.runtime_state().await,
        ComputerRuntimeState::JoinedOffice
    );
    assert!(preserved.is_connected().await);
    assert!(preserved
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_some());
    assert!(
        preserved.has_refresh_task_for_test().await,
        "reversible deletion failure aborted the Manager refresh task"
    );
    assert_eq!(stats.active(), 1, "failed deletion disconnected the socket");

    std::fs::remove_file(trash_blocker).expect("remove trash blocker");
    delete_computer_instance_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .expect("cleanup delete");
}

#[tokio::test]
async fn delete_succeeds_while_socket_reference_is_shared() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    common::mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        common::echo_server_config("commit-echo"),
    )
    .await
    .expect("register MCP server");
    runtime
        .start_mcp_server(&BundleId::try_from("commit-echo").unwrap())
        .await
        .expect("start MCP server");
    assert!(
        runtime
            .mcp_server_statuses()
            .await
            .iter()
            .any(|(_bundle_id, name, running, _)| name == "commit-echo" && *running),
        "commit MCP server never started"
    );
    let (server_url, stats) = start_smcp_socket_server().await;
    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "commit-office",
            "commit-computer",
        )
        .await
        .expect("connect smcp socket");
    *runtime.connection_handle_for_test().write_owned().await = Some(ConnectionState {
        profile_name: "commit-profile".to_string(),
        url: server_url,
        office_id: "commit-office".to_string(),
        computer_name: "commit-computer".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manager_robot".to_string(),
        target_id: Some("manager:1".to_string()),
        target_name: Some("Commit Robot".to_string()),
        employee_id: Some(1),
        generation: 9,
    });
    runtime
        .set_refresh_task(tokio::spawn(std::future::pending()))
        .await;
    let held_socket = runtime
        .clone_sdk_socketio_client_for_test()
        .await
        .expect("hold an SDK socket reference across high-level teardown");
    let storage_root = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID);
    std::fs::create_dir_all(&storage_root).expect("create instance storage");
    std::fs::write(storage_root.join("marker.txt"), b"preserve me").expect("write storage marker");

    delete_computer_instance_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .expect("shared references must not block SDK-owned teardown");
    assert!(state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .is_none());
    assert!(state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .is_err());
    assert!(!storage_root.exists());
    wait_for(
        "shared-reference delete left the SMCP socket active",
        || stats.active() == 0,
    )
    .await;
    assert_eq!(stats.leave_events(), 1);
    drop(held_socket);
}

#[tokio::test]
async fn deletion_exhausts_teardown_after_commit_cleanup_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    common::mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        common::echo_server_config("cleanup-echo"),
    )
    .await
    .expect("register cleanup MCP server");
    runtime
        .start_mcp_server(&BundleId::try_from("cleanup-echo").unwrap())
        .await
        .expect("start cleanup MCP server");
    assert!(
        runtime
            .mcp_server_statuses()
            .await
            .iter()
            .any(|(_bundle_id, name, running, _)| name == "cleanup-echo" && *running),
        "cleanup MCP server never started"
    );
    let (server_url, stats) = start_smcp_socket_server().await;
    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "cleanup-office",
            "cleanup-computer",
        )
        .await
        .expect("connect smcp socket");
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "cleanup-profile".to_string(),
            url: server_url,
            office_id: "cleanup-office".to_string(),
            computer_name: "cleanup-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:1".to_string()),
            target_name: Some("Cleanup Robot".to_string()),
            employee_id: Some(1),
            generation: 10,
        })
        .await
        .expect("install business connection");
    runtime
        .set_refresh_task(tokio::spawn(std::future::pending()))
        .await;
    let storage_root = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID);
    std::fs::create_dir_all(&storage_root).expect("create instance storage");
    std::fs::write(storage_root.join("marker.txt"), b"delete me").expect("write marker");
    runtime.fail_prepare_shutdown_once_for_test();

    delete_computer_instance_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .expect("post-commit cleanup failure must still finish deletion");

    assert!(state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .is_none());
    assert!(state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .is_err());
    assert!(!storage_root.exists());
    assert!(!runtime.has_refresh_task_for_test().await);
    assert!(runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_none());
    assert_eq!(
        runtime.runtime_state().await,
        ComputerRuntimeState::Shutdown
    );
    assert!(
        runtime.mcp_server_statuses().await.is_empty(),
        "committed deletion retained SDK MCP runtime state"
    );
    wait_for("committed deletion left the SMCP socket active", || {
        stats.active() == 0
    })
    .await;
}

#[tokio::test]
async fn close_succeeds_while_socket_reference_is_shared() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "shared-office",
            "shared-test-computer",
        )
        .await
        .expect("connect smcp socket");
    wait_for("server never observed the SMCP socket connection", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    let held_socket = runtime
        .clone_sdk_socketio_client_for_test()
        .await
        .expect("socket should be present");
    let connection = ConnectionState {
        profile_name: "shared-profile".to_string(),
        url: server_url.clone(),
        office_id: "shared-office".to_string(),
        computer_name: "shared-test-computer".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manual_smcp".to_string(),
        target_id: Some("shared-target".to_string()),
        target_name: Some("shared-profile".to_string()),
        employee_id: None,
        generation: 0,
    };
    *runtime.connection_handle_for_test().write_owned().await = Some(connection.clone());

    close_smcp_connection(&runtime, connection)
        .await
        .expect("shared references must not block SDK-owned disconnect");
    assert!(runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_none());
    wait_for("SMCP socket should be disconnected", || {
        stats.active() == 0 && stats.disconnected() == 1
    })
    .await;
    drop(held_socket);
    delete_computer_instance_core(&state, TEST_INSTANCE_ID.to_string())
        .await
        .expect("cleanup delete");
}

#[tokio::test]
async fn runtime_metadata_update_preserves_smcp_connection_snapshot() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "rebuild-office",
            "rebuild-computer",
        )
        .await
        .expect("connect smcp socket");

    wait_for("server never observed the SMCP socket connection", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    *runtime.connection_handle_for_test().write_owned().await = Some(ConnectionState {
        profile_name: "rebuild-profile".to_string(),
        url: server_url,
        office_id: "rebuild-office".to_string(),
        computer_name: "rebuild-computer".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manual_smcp".to_string(),
        target_id: Some("rebuild-target".to_string()),
        target_name: Some("rebuild-profile".to_string()),
        employee_id: None,
        generation: 0,
    });
    assert!(runtime.is_connected().await);

    let status = rename_computer_instance_core(
        &state,
        RenameComputerInstanceRequest {
            id: TEST_INSTANCE_ID.to_string(),
            name: "Renamed Test Computer".to_string(),
            description: None,
        },
    )
    .await
    .expect("rename should update runtime metadata");

    assert!(status.connected);
    assert!(runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_some());
    assert!(runtime.is_connected().await);
    assert_eq!(
        runtime.runtime_state().await,
        ComputerRuntimeState::JoinedOffice
    );

    runtime
        .clear_smcp_connection()
        .await
        .expect("cleanup preserved connection");
    assert!(!runtime.is_connected().await);
    assert_eq!(runtime.runtime_state().await, ComputerRuntimeState::Started);

    wait_for(
        "runtime rebuild did not close the underlying Socket.IO connection",
        || stats.active() == 0 && stats.disconnected() == 1,
    )
    .await;
}

#[tokio::test]
async fn failed_profile_switch_keeps_existing_smcp_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;
    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "existing-office",
            "existing-computer",
        )
        .await
        .expect("connect existing smcp socket");

    wait_for("server never observed the existing SMCP socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    {
        let mut conn = runtime.connection_handle_for_test().write_owned().await;
        *conn = Some(ConnectionState {
            profile_name: "existing-profile".to_string(),
            url: server_url,
            office_id: "existing-office".to_string(),
            computer_name: "existing-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some("existing-target".to_string()),
            target_name: Some("existing-profile".to_string()),
            employee_id: None,
            generation: 0,
        });
    }

    let err = connect_connection_target_core(&state, TEST_INSTANCE_ID, "missing-target")
        .await
        .expect_err("missing target should fail");
    assert!(
        err.contains("missing-target"),
        "error should identify the missing target, got: {err}"
    );

    let conn = runtime.connection_handle_for_test().read_owned().await;
    let connection = conn
        .as_ref()
        .expect("existing connection should be preserved");
    assert_eq!(connection.profile_name, "existing-profile");
    drop(conn);

    assert_eq!(stats.active(), 1);
    assert_eq!(stats.disconnected(), 0);

    let existing_connection = {
        let mut conn = runtime.connection_handle_for_test().write_owned().await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(&runtime, connection)
            .await
            .expect("close existing socket");
    }
}

#[tokio::test]
async fn profile_connect_requires_running_computer() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    assert!(!runtime.is_running().await);

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "stopped-target".to_string(),
            name: "stopped-target".to_string(),
            url: "http://127.0.0.1:9".to_string(),
            namespace: "/smcp".to_string(),
            office_id: "stopped-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let err = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id)
        .await
        .expect_err("stopped computer should not connect");

    assert_eq!(
        err,
        "runtime action 'connect' is unavailable while lifecycle is 'created' (not_running)"
    );
    assert!(runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_none());
}

#[tokio::test]
async fn profile_connect_uses_computer_instance_name_as_connection_identity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "identity-target".to_string(),
            name: "identity-target".to_string(),
            url: server_url.clone(),
            namespace: "/smcp".to_string(),
            office_id: "identity-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id)
        .await
        .expect("connect target");

    wait_for("server never observed the SMCP join", || {
        stats.active() == 1 && stats.join_events() == 1
    })
    .await;

    let connection = runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .clone()
        .unwrap();
    assert_eq!(connection.computer_name, "Test Computer");

    close_smcp_connection(&runtime, connection)
        .await
        .expect("close smcp socket");
}

#[tokio::test]
async fn profile_connect_exposes_backend_connecting_state_until_join_office_succeeds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) =
        start_smcp_socket_server_with_join_delay(Duration::from_millis(600)).await;
    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "state-machine-target".to_string(),
            name: "state-machine-target".to_string(),
            url: server_url,
            namespace: "/smcp".to_string(),
            office_id: "state-machine-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let connect = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id);
    let observe_connecting = async {
        wait_for("server never entered delayed join_office", || {
            stats.join_events() == 1
        })
        .await;
        runtime.connection_snapshot().await
    };
    let (result, connecting) = tokio::join!(connect, observe_connecting);
    result.expect("connect target");

    assert_eq!(connecting.status, ClientConnectionStatus::Connecting);
    assert!(!connecting.present);
    assert!(!connecting.actions.connect.enabled);
    assert!(!connecting.actions.disconnect.enabled);
    let connected = runtime.connection_snapshot().await;
    assert_eq!(connected.status, ClientConnectionStatus::Connected);
    assert!(connected.present);
    assert!(connected.context.is_some());
    assert!(connected.actions.disconnect.enabled);

    let connection = runtime
        .connection_state_snapshot()
        .await
        .expect("connection snapshot");
    close_smcp_connection(&runtime, connection)
        .await
        .expect("close smcp socket");
}

#[tokio::test]
async fn restart_during_delayed_join_cannot_leave_authority_without_transport() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) =
        start_smcp_socket_server_with_join_delay(Duration::from_millis(600)).await;
    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "restart-race-target".to_string(),
            name: "restart-race-target".to_string(),
            url: server_url,
            namespace: "/smcp".to_string(),
            office_id: "restart-race-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let connect = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id);
    let restart = async {
        wait_for("server never entered delayed join_office", || {
            stats.join_events() == 1
        })
        .await;
        runtime.restart().await
    };
    let (connect_result, restart_result) = tokio::join!(connect, restart);

    restart_result.expect("restart should settle after atomic connect commit");
    assert!(
        connect_result.is_err(),
        "the superseded connect command must not report success"
    );
    wait_for("restart left the superseded socket active", || {
        stats.active() == 0
    })
    .await;
    let snapshot = runtime.connection_snapshot().await;
    assert_eq!(snapshot.status, ClientConnectionStatus::Disconnected);
    assert!(!snapshot.present);
    assert!(snapshot.operation.is_none());
    assert!(runtime.clone_sdk_socketio_client_for_test().await.is_none());
}

#[tokio::test]
async fn deleting_manual_target_during_connect_prevents_stale_policy_commit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) =
        start_smcp_socket_server_with_join_delay(Duration::from_millis(600)).await;
    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "deleted-during-connect".to_string(),
            name: "deleted-during-connect".to_string(),
            url: server_url,
            namespace: "/smcp".to_string(),
            office_id: "deleted-during-connect-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let connect = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id);
    let delete = async {
        wait_for("server never entered delayed join_office", || {
            stats.join_events() == 1
        })
        .await;
        delete_manual_smcp_target_core(&state, &target.id).await
    };
    let (connect_result, delete_result) = tokio::join!(connect, delete);

    delete_result.expect("target deletion should win before connection commit");
    assert!(
        connect_result.is_err(),
        "connection must not commit a policy for a deleted target"
    );
    assert!(state.config.get_manual_smcp_target(&target.id).is_err());
    let persisted = state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .expect("Computer profile remains");
    assert!(persisted.connection_policy.target.is_none());
    wait_for("failed commit left the stale socket active", || {
        stats.active() == 0
    })
    .await;
    assert_eq!(
        runtime.connection_snapshot().await.status,
        ClientConnectionStatus::Disconnected
    );
}

#[tokio::test]
async fn deleting_computer_during_connect_cannot_resurrect_profile_or_runtime() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) =
        start_smcp_socket_server_with_join_delay(Duration::from_millis(600)).await;
    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "delete-computer-race".to_string(),
            name: "delete-computer-race".to_string(),
            url: server_url,
            namespace: "/smcp".to_string(),
            office_id: "delete-computer-race-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let connect = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id);
    let delete = async {
        wait_for("server never entered delayed join_office", || {
            stats.join_events() == 1
        })
        .await;
        delete_computer_instance_core(&state, TEST_INSTANCE_ID.to_string()).await
    };
    let (connect_result, delete_result) = tokio::join!(connect, delete);

    delete_result.expect("Computer deletion should complete");
    assert!(
        connect_result.is_err(),
        "superseded connection must not report success"
    );
    assert!(state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .is_err());
    assert!(state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .is_none());
    wait_for("Computer deletion left the stale socket active", || {
        stats.active() == 0
    })
    .await;
}

#[tokio::test]
async fn rename_during_connect_supersedes_stale_connection_commit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) =
        start_smcp_socket_server_with_join_delay(Duration::from_millis(600)).await;
    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "rename-during-connect".to_string(),
            name: "rename-during-connect".to_string(),
            url: server_url,
            namespace: "/smcp".to_string(),
            office_id: "rename-during-connect-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let connect = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id);
    let rename = async {
        wait_for("server never entered delayed join_office", || {
            stats.join_events() == 1
        })
        .await;
        rename_computer_instance_core(
            &state,
            RenameComputerInstanceRequest {
                id: TEST_INSTANCE_ID.to_string(),
                name: "Renamed During Connect".to_string(),
                description: None,
            },
        )
        .await
    };
    let (connect_result, rename_result) = tokio::join!(connect, rename);

    rename_result.expect("rename should complete");
    assert!(
        connect_result.is_err(),
        "connection established with the previous Computer name must be superseded"
    );
    let persisted = state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .expect("Computer profile");
    assert_eq!(persisted.name, "Renamed During Connect");
    assert!(persisted.connection_policy.target.is_none());
    let current_runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("runtime remains registered");
    assert_eq!(current_runtime.instance.name, "Renamed During Connect");

    wait_for("superseded rename connection left a socket active", || {
        stats.active() == 0
    })
    .await;
    assert_eq!(
        current_runtime.connection_snapshot().await.status,
        ClientConnectionStatus::Disconnected
    );
}

#[tokio::test]
async fn reconnect_terminal_failure_closes_transport_or_exposes_orphan_cleanup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "terminal-office",
            "terminal-computer",
        )
        .await
        .expect("connect terminal socket");
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "manager:1".to_string(),
            url: server_url.clone(),
            office_id: "terminal-office".to_string(),
            computer_name: "terminal-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:1".to_string()),
            target_name: Some("Terminal Robot".to_string()),
            employee_id: Some(1),
            generation: 80,
        })
        .await
        .expect("install terminal authority");
    assert!(runtime.begin_reconnect_for_generation(80).await);
    assert!(
        runtime
            .terminate_reconnect_for_generation(80, "Manager session expired".to_string(), false,)
            .await
    );
    wait_for("terminal reconnect failure left the socket active", || {
        stats.active() == 0
    })
    .await;
    let settled = runtime.connection_snapshot().await;
    assert_eq!(settled.status, ClientConnectionStatus::Disconnected);
    assert!(!settled.present);
    assert!(!settled.actions.disconnect.enabled);

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "orphan-office",
            "orphan-computer",
        )
        .await
        .expect("connect orphan socket");
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "manager:2".to_string(),
            url: server_url,
            office_id: "orphan-office".to_string(),
            computer_name: "orphan-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:2".to_string()),
            target_name: Some("Orphan Robot".to_string()),
            employee_id: Some(2),
            generation: 81,
        })
        .await
        .expect("install orphan authority");
    assert!(runtime.begin_reconnect_for_generation(81).await);
    runtime.fail_next_smcp_disconnect_for_test();
    assert!(
        runtime
            .terminate_reconnect_for_generation(
                81,
                "SMCP reconnect retry limit exhausted".to_string(),
                true,
            )
            .await
    );
    let orphan = runtime.connection_snapshot().await;
    assert_eq!(orphan.status, ClientConnectionStatus::Disconnected);
    assert!(!orphan.present);
    assert!(orphan.actions.disconnect.enabled);
    assert!(orphan.last_error.as_ref().is_some_and(|error| error
        .message
        .contains("failed to close stale SMCP transport")));
    assert!(runtime.clone_sdk_socketio_client_for_test().await.is_some());

    runtime
        .clear_smcp_connection()
        .await
        .expect("manual orphan cleanup");
    wait_for("manual orphan cleanup left the socket active", || {
        stats.active() == 0
    })
    .await;
}

#[tokio::test]
async fn disconnect_failure_fails_closed_when_transport_liveness_is_unproven() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "fail-closed-office",
            "fail-closed-computer",
        )
        .await
        .expect("connect fail-closed socket");
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "manual".to_string(),
            url: server_url,
            office_id: "fail-closed-office".to_string(),
            computer_name: "fail-closed-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some("fail-closed-target".to_string()),
            target_name: Some("Fail Closed Target".to_string()),
            employee_id: None,
            generation: 82,
        })
        .await
        .expect("install fail-closed authority");

    let operation = runtime
        .begin_connection_operation(
            tfrobot_client_lib::services::computer::ClientConnectionOperation::Disconnect,
            None,
        )
        .await
        .expect("begin disconnect");
    runtime.fail_next_smcp_disconnect_for_test();
    let error = runtime
        .disconnect_smcp_socketio()
        .await
        .expect_err("injected disconnect should fail");
    assert!(
        runtime
            .reconcile_disconnect_failure_for_token(operation, error)
            .await
    );

    let settled = runtime.connection_snapshot().await;
    assert_eq!(settled.status, ClientConnectionStatus::Disconnected);
    assert!(!settled.present);
    assert!(settled.actions.disconnect.enabled);
    assert!(runtime.clone_sdk_socketio_client_for_test().await.is_some());

    runtime
        .clear_smcp_connection()
        .await
        .expect("cleanup fail-closed orphan");
    wait_for("fail-closed orphan remained connected", || {
        stats.active() == 0
    })
    .await;
}

#[tokio::test]
async fn reconnect_teardown_timeout_bounds_attempt_and_terminal_settlement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "timeout-office",
            "timeout-computer",
        )
        .await
        .expect("connect timeout socket");
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "manager:3".to_string(),
            url: server_url.clone(),
            office_id: "timeout-office".to_string(),
            computer_name: "timeout-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:3".to_string()),
            target_name: Some("Timeout Robot".to_string()),
            employee_id: Some(3),
            generation: 83,
        })
        .await
        .expect("install timeout authority");
    assert!(runtime.begin_reconnect_for_generation(83).await);

    runtime.hang_next_smcp_disconnect_for_test();
    let attempt = tokio::time::timeout(
        Duration::from_secs(7),
        runtime.reconnect_smcp_socketio_for_generation(
            83,
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "timeout-office",
            "timeout-computer",
            60,
        ),
    )
    .await
    .expect("reconnect attempt teardown must be bounded")
    .expect_err("timed-out teardown must fail the attempt");
    assert!(attempt.contains("Timed out disconnecting SMCP socket"));

    runtime.hang_next_smcp_disconnect_for_test();
    let terminated = tokio::time::timeout(
        Duration::from_secs(7),
        runtime.terminate_reconnect_for_generation(
            83,
            "SMCP reconnect retry limit exhausted".to_string(),
            true,
        ),
    )
    .await
    .expect("terminal reconnect teardown must be bounded");
    assert!(terminated);

    let settled = runtime.connection_snapshot().await;
    assert_eq!(settled.status, ClientConnectionStatus::Disconnected);
    assert!(!settled.present);
    assert!(settled.actions.disconnect.enabled);
    assert!(settled.last_error.as_ref().is_some_and(|error| error
        .message
        .contains("Timed out disconnecting SMCP socket")));

    runtime
        .clear_smcp_connection()
        .await
        .expect("cleanup timeout orphan");
    wait_for("timeout orphan remained connected", || stats.active() == 0).await;
}

#[tokio::test]
async fn runtime_stop_bounds_pending_transport_teardown_and_clears_authority() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "stop-timeout-office",
            "stop-timeout-computer",
        )
        .await
        .expect("connect stop-timeout socket");
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "manual".to_string(),
            url: server_url,
            office_id: "stop-timeout-office".to_string(),
            computer_name: "stop-timeout-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some("stop-timeout-target".to_string()),
            target_name: Some("Stop Timeout Target".to_string()),
            employee_id: None,
            generation: 84,
        })
        .await
        .expect("install stop-timeout authority");

    runtime.hang_next_smcp_disconnect_for_test();
    let status = tokio::time::timeout(
        Duration::from_secs(8),
        stop_computer_instance_core(&state, TEST_INSTANCE_ID.to_string()),
    )
    .await
    .expect("runtime stop must not hang on transport teardown")
    .expect("runtime stop should complete after bounded cleanup");

    assert_eq!(
        status.connection_state.status,
        ClientConnectionStatus::Disconnected
    );
    assert!(!status.connection_state.present);
    assert!(status.connection_state.operation.is_none());
    assert!(!runtime.is_connected().await);
    wait_for("runtime stop left the socket active", || {
        stats.active() == 0
    })
    .await;
}

#[tokio::test]
async fn stale_snapshot_after_reconnect_failure_does_not_block_manual_reconnect() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "stale-office",
            "stale-computer",
        )
        .await
        .expect("connect old socket");

    wait_for("server never observed the original SMCP socket", || {
        stats.active() == 1 && stats.connected() == 1 && stats.join_events() == 1
    })
    .await;

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "stale-target".to_string(),
            name: "stale-target".to_string(),
            url: server_url.clone(),
            namespace: "/smcp".to_string(),
            office_id: "stale-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    {
        let mut conn = runtime.connection_handle_for_test().write_owned().await;
        *conn = Some(ConnectionState {
            profile_name: "stale-target".to_string(),
            url: server_url.clone(),
            office_id: "stale-office".to_string(),
            computer_name: "stale-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some(target.id.clone()),
            target_name: Some("stale-target".to_string()),
            employee_id: None,
            generation: 5,
        });
    }

    let dead_url = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{}", addr)
    };
    let params = ManagerConnectionParams {
        url: dead_url,
        office_id: "stale-office".to_string(),
        routing_headers: HashMap::new(),
        employee_id: 1,
        robot_account_id: 42,
        scope: None,
        robot_binding: RobotBindingMetadata {
            employee_id: 1,
            robot_id: Some("stale-office".to_string()),
            robot_account_id: Some(42),
            namespace: Some("tfrobotserver".to_string()),
            robot_name: Some("Robot".to_string()),
        },
    };
    let token = ExchangedToken {
        access_token: "new-jwt".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: 300,
        scope: None,
    };

    let outcome = reconnect_with_token(&runtime, &params, 5, &token).await;
    assert!(matches!(outcome, RefreshOutcome::Retry));
    assert_eq!(runtime.runtime_state().await, ComputerRuntimeState::Started);
    assert!(!runtime.is_connected().await);
    wait_for("old socket should be closed after failed reconnect", || {
        stats.active() == 0 && stats.disconnected() == 1
    })
    .await;

    connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id)
        .await
        .expect("manual reconnect should rebuild socket despite stale snapshot");

    wait_for("manual reconnect should open a fresh socket", || {
        stats.active() == 1 && stats.connected() == 2 && stats.join_events() == 2
    })
    .await;
    assert!(runtime.is_connected().await);
    assert_eq!(runtime.runtime_snapshot().await.last_error, None);

    let final_connection = runtime
        .connection_handle_for_test()
        .write_owned()
        .await
        .take();
    if let Some(connection) = final_connection {
        close_smcp_connection(&runtime, connection)
            .await
            .expect("close final socket");
    }
}

#[tokio::test]
async fn profile_switch_to_different_robot_requires_disconnect() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (old_server_url, old_stats) = start_smcp_socket_server().await;
    let (new_server_url, new_stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &old_server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "old-office",
            "old-computer",
        )
        .await
        .expect("connect old smcp socket");

    wait_for("server never observed the old SMCP socket", || {
        old_stats.active() == 1 && old_stats.connected() == 1
    })
    .await;

    {
        let mut conn = runtime.connection_handle_for_test().write_owned().await;
        *conn = Some(ConnectionState {
            profile_name: "old-profile".to_string(),
            url: old_server_url,
            office_id: "old-office".to_string(),
            computer_name: "old-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some("old-target".to_string()),
            target_name: Some("old-profile".to_string()),
            employee_id: None,
            generation: 0,
        });
    }

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "new-target".to_string(),
            name: "new-target".to_string(),
            url: new_server_url.clone(),
            namespace: "/smcp".to_string(),
            office_id: "new-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let err = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id)
        .await
        .expect_err("switching robots without disconnect should fail");
    assert!(
        err.contains("disconnect before switching Robot"),
        "error should instruct the user to disconnect first, got: {err}"
    );

    let conn = runtime.connection_handle_for_test().read_owned().await;
    let connection = conn.as_ref().expect("old connection should be retained");
    assert_eq!(connection.profile_name, "old-profile");
    assert_eq!(connection.office_id, "old-office");
    drop(conn);
    assert_eq!(old_stats.active(), 1);
    assert_eq!(old_stats.disconnected(), 0);
    assert_eq!(
        new_stats.active(),
        0,
        "rejected profile switch must not open a new socket"
    );

    let old_connection = {
        let mut conn = runtime.connection_handle_for_test().write_owned().await;
        conn.take()
    };
    if let Some(connection) = old_connection {
        close_smcp_connection(&runtime, connection)
            .await
            .expect("close old socket");
    }
}

#[tokio::test]
async fn profile_connect_rejects_robot_already_connected_by_another_instance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    let other_instance_id = "other-computer";
    state
        .config
        .add_computer_instance(ComputerInstance::new(other_instance_id, "Other Computer"))
        .unwrap();
    let other_runtime = state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(other_instance_id)
                .unwrap(),
        )
        .await
        .unwrap();
    other_runtime.start().await.expect("start other runtime");

    other_runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "shared-office",
            "other-computer",
        )
        .await
        .expect("connect other smcp socket");
    wait_for("server never observed the other SMCP socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    {
        let mut conn = other_runtime
            .connection_handle_for_test()
            .write_owned()
            .await;
        *conn = Some(ConnectionState {
            profile_name: "other-profile".to_string(),
            url: server_url.clone(),
            office_id: "shared-office".to_string(),
            computer_name: "other-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some("other-target".to_string()),
            target_name: Some("other-profile".to_string()),
            employee_id: None,
            generation: 0,
        });
    }

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "target-profile".to_string(),
            name: "target-profile".to_string(),
            url: server_url,
            namespace: "/smcp".to_string(),
            office_id: "shared-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let err = connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id)
        .await
        .expect_err("same robot connected by another instance should fail");
    assert!(
        err.contains("Other Computer"),
        "error should identify the owning computer, got: {err}"
    );

    assert_eq!(
        stats.active(),
        1,
        "rejected profile connect must not open a second socket"
    );

    let existing_connection = {
        let mut conn = other_runtime
            .connection_handle_for_test()
            .write_owned()
            .await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(&other_runtime, connection)
            .await
            .expect("close other socket");
    }
}

#[tokio::test]
async fn profile_connect_rejects_robot_owned_by_refreshing_instance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let owner_runtime = create_test_runtime(&state).await;
    owner_runtime.start().await.expect("start owner runtime");
    let (old_server_url, old_stats) = start_smcp_socket_server().await;
    let (refresh_server_url, refresh_stats) =
        start_smcp_socket_server_with_join_delay(Duration::from_millis(600)).await;

    owner_runtime
        .connect_smcp_socketio(
            &old_server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "refreshing-office",
            "owner-computer",
        )
        .await
        .expect("connect owner socket");
    wait_for("server never observed owner socket", || {
        old_stats.active() == 1 && old_stats.connected() == 1
    })
    .await;

    let target_id = "refreshing-target";
    {
        let mut conn = owner_runtime
            .connection_handle_for_test()
            .write_owned()
            .await;
        *conn = Some(ConnectionState {
            profile_name: "owner-profile".to_string(),
            url: old_server_url,
            office_id: "refreshing-office".to_string(),
            computer_name: "owner-computer".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manual_smcp".to_string(),
            target_id: Some(target_id.to_string()),
            target_name: Some("owner-profile".to_string()),
            employee_id: None,
            generation: 77,
        });
    }

    let params = ManagerConnectionParams {
        url: refresh_server_url.clone(),
        office_id: "refreshing-office".to_string(),
        routing_headers: HashMap::new(),
        employee_id: 1,
        robot_account_id: 42,
        scope: None,
        robot_binding: RobotBindingMetadata {
            employee_id: 1,
            robot_id: Some("refreshing-office".to_string()),
            robot_account_id: Some(42),
            namespace: Some("tfrobotserver".to_string()),
            robot_name: Some("Robot".to_string()),
        },
    };
    let token = ExchangedToken {
        access_token: "new-jwt".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: 300,
        scope: None,
    };
    let owner_runtime_for_refresh = owner_runtime.clone();
    let refresh_task = tokio::spawn(async move {
        reconnect_with_token(&owner_runtime_for_refresh, &params, 77, &token).await
    });

    for _ in 0..40 {
        if refresh_stats.connected() == 1
            && refresh_stats.join_events() == 1
            && owner_runtime.runtime_state().await == ComputerRuntimeState::Connected
        {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        owner_runtime.runtime_state().await,
        ComputerRuntimeState::Connected,
        "owner runtime never entered refresh join window"
    );

    let challenger_instance_id = "challenger-computer";
    state
        .config
        .add_computer_instance(ComputerInstance::new(
            challenger_instance_id,
            "Challenger Computer",
        ))
        .unwrap();
    let challenger_runtime = state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(challenger_instance_id)
                .unwrap(),
        )
        .await
        .unwrap();
    challenger_runtime
        .start()
        .await
        .expect("start challenger runtime");

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: target_id.to_string(),
            name: "refreshing-target".to_string(),
            url: refresh_server_url,
            namespace: "/smcp".to_string(),
            office_id: "refreshing-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let err = connect_connection_target_core(&state, challenger_instance_id, &target.id)
        .await
        .expect_err("refreshing owner should still block another instance");
    assert!(
        err.contains("Test Computer"),
        "error should identify the refreshing owner, got: {err}"
    );

    assert_eq!(
        refresh_stats.connected(),
        1,
        "rejected challenger must not open another socket"
    );

    let outcome = refresh_task.await.expect("refresh task should complete");
    assert!(matches!(outcome, RefreshOutcome::Renewed(300)));

    let owner_connection = owner_runtime
        .connection_handle_for_test()
        .write_owned()
        .await
        .take();
    if let Some(connection) = owner_connection {
        close_smcp_connection(&owner_runtime, connection)
            .await
            .expect("close owner socket");
    }
}

#[tokio::test]
async fn concurrent_profile_connect_same_robot_allows_only_one_instance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let target_runtime = create_test_runtime(&state).await;
    target_runtime.start().await.expect("start target runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    let other_instance_id = "other-computer";
    state
        .config
        .add_computer_instance(ComputerInstance::new(other_instance_id, "Other Computer"))
        .unwrap();
    let other_runtime = state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(other_instance_id)
                .unwrap(),
        )
        .await
        .unwrap();
    other_runtime.start().await.expect("start other runtime");

    let target = state
        .config
        .save_manual_smcp_target(ManualSmcpTarget {
            id: "shared-target".to_string(),
            name: "shared-target".to_string(),
            url: server_url.clone(),
            namespace: "/smcp".to_string(),
            office_id: "shared-office".to_string(),
            headers: HashMap::new(),
        })
        .expect("save target");

    let (target_result, other_result) = tokio::join!(
        connect_connection_target_core(&state, TEST_INSTANCE_ID, &target.id),
        connect_connection_target_core(&state, other_instance_id, &target.id)
    );

    let success_count = usize::from(target_result.is_ok()) + usize::from(other_result.is_ok());
    assert_eq!(
        success_count, 1,
        "exactly one concurrent connection may win; target={target_result:?} other={other_result:?}"
    );
    let error = target_result.err().or_else(|| other_result.err()).unwrap();
    assert!(
        error.contains("already connected")
            || error.contains("already being established")
            || error.contains("disconnect before switching Robot"),
        "losing connection should fail by duplicate-connection guard, got: {error}"
    );

    wait_for("server should only have one active socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    for runtime in [target_runtime, other_runtime] {
        let existing_connection = {
            let mut conn = runtime.connection_handle_for_test().write_owned().await;
            conn.take()
        };
        if let Some(connection) = existing_connection {
            close_smcp_connection(&runtime, connection)
                .await
                .expect("close concurrent socket");
        }
    }
}

// ───────────────────── 预刷新重连：generation 守卫 + abort（TFRC-11 / C1） ─────────────────────

/// generation 守卫：换连接仅在代际匹配时生效；用户期间断开/改连（代际不符）时旧刷新任务
/// 走 Stale 分支、不覆盖新连接。
#[tokio::test]
async fn try_install_refreshed_client_respects_generation_guard() {
    let runtime = ComputerInstanceRuntime::new(
        ComputerInstance::new("generation-guard", "Generation Guard"),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    runtime
        .install_connection_state(ConnectionState {
            profile_name: "p".to_string(),
            url: "http://127.0.0.1:1".to_string(),
            office_id: "office".to_string(),
            computer_name: "gen-test".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:1".to_string()),
            target_name: Some("Robot".to_string()),
            employee_id: Some(1),
            generation: 7,
        })
        .await
        .unwrap();

    // 代际匹配 → Replaced（更新业务 snapshot 时间戳）。
    match try_install_refreshed_client(&runtime, 7).await {
        SwapResult::Replaced => {}
        SwapResult::Stale => panic!("expected Replaced on matching generation"),
    }

    // 代际不符（模拟用户已改连别的机器人）→ Stale（不覆盖现连接）。
    match try_install_refreshed_client(&runtime, 999).await {
        SwapResult::Stale => {}
        SwapResult::Replaced => panic!("expected Stale on non-matching generation"),
    }

    // 现连接仍是代际 7（client B）。
    {
        assert_eq!(
            runtime
                .connection_state_snapshot()
                .await
                .as_ref()
                .map(|c| c.generation),
            Some(7)
        );
    }
}

/// TFRC-51 gives failed token pre-refresh a clear runtime state: reconnect failure returns `Retry`,
/// leaves the business snapshot guarded by generation, and records a client runtime diagnostic.
#[tokio::test]
async fn reconnect_with_token_records_diagnostic_on_sdk_build_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    // 旧连接连到「好」server（initial join → join_events = 1）。
    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "office",
            "rollback-test",
        )
        .await
        .expect("connect old socket");
    {
        let mut guard = runtime.connection_handle_for_test().write_owned().await;
        *guard = Some(ConnectionState {
            profile_name: "p".to_string(),
            url: server_url.clone(),
            office_id: "office".to_string(),
            computer_name: "rollback-test".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:1".to_string()),
            target_name: Some("Robot".to_string()),
            employee_id: Some(1),
            generation: 5,
        });
    }

    // 预刷新参数指向一个「死」地址 → build_and_join 必失败。
    let dead_url = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        format!("http://{}", addr)
    };
    let params = ManagerConnectionParams {
        url: dead_url,
        office_id: "office".to_string(),
        routing_headers: HashMap::new(),
        employee_id: 1,
        robot_account_id: 42,
        scope: None,
        robot_binding: RobotBindingMetadata {
            employee_id: 1,
            robot_id: Some("office".to_string()),
            robot_account_id: Some(42),
            namespace: Some("tfrobotserver".to_string()),
            robot_name: Some("Robot".to_string()),
        },
    };
    let token = ExchangedToken {
        access_token: "new-jwt".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: 300,
        scope: None,
    };

    let outcome = reconnect_with_token(&runtime, &params, 5, &token).await;

    // build 失败 → Retry（不替换连接）；SDK 回到 Started，client diagnostic 保留失败信息。
    assert!(
        matches!(outcome, RefreshOutcome::Retry),
        "expected Retry on build failure"
    );
    assert_eq!(runtime.runtime_state().await, ComputerRuntimeState::Started);
    let snapshot = runtime.runtime_snapshot().await;
    assert_eq!(
        snapshot.last_error, None,
        "client-owned diagnostics must not be projected as SDK last_error"
    );
    let problem = snapshot
        .problems
        .iter()
        .find(|problem| problem.source == ComputerRuntimeProblemSource::ClientConnection)
        .expect("failed reconnect should surface a current structured problem");
    assert_eq!(problem.severity, ComputerRuntimeProblemSeverity::Degraded);
    assert_eq!(
        problem.message,
        ComputerRuntimeProblemMessage::ReconnectionFailed
    );
    assert_eq!(problem.operation, "reconnect");
    assert!(problem.current);
    assert!(problem.occurred_at.contains('T'));
    assert_eq!(
        problem.affected_capabilities,
        vec![ComputerRuntimeAffectedCapability::Connection]
    );
    assert_eq!(
        problem.technical_detail.as_deref(),
        Some("SMCP reconnect failed; retrying")
    );
    assert!(!runtime.is_connected().await);
    assert!(
        runtime.connection_status().await.is_none(),
        "failed reconnect must not expose a healthy connection status"
    );

    wait_for(
        "expected old socket to leave before reconnect retry",
        || stats.leave_events() == 1,
    )
    .await;
    wait_for(
        "expected old socket to disconnect after reconnect failure",
        || stats.active() == 0 && stats.disconnected() == 1,
    )
    .await;

    // 现连接仍是同一代际、未变僵尸 None。
    {
        let connection = runtime.connection_state_snapshot().await;
        assert_eq!(
            connection.as_ref().map(|c| c.generation),
            Some(5),
            "old connection must remain at its generation after rollback"
        );
    }

    let final_conn = runtime.take_connection_state().await;
    if let Some(cs) = final_conn {
        close_smcp_connection(&runtime, cs)
            .await
            .expect("close reconnect socket");
    }
}

#[tokio::test]
async fn reconnect_with_token_reconnects_socket_and_refreshes_snapshot() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (old_server_url, old_stats) = start_smcp_socket_server().await;
    let (new_server_url, new_stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &old_server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "office",
            "refresh-test",
        )
        .await
        .expect("connect old socket");
    wait_for("server never observed the old SMCP socket", || {
        old_stats.active() == 1 && old_stats.connected() == 1
    })
    .await;

    let connected_at = chrono::Utc::now() - chrono::Duration::seconds(5);
    {
        let mut guard = runtime.connection_handle_for_test().write_owned().await;
        *guard = Some(ConnectionState {
            profile_name: "p".to_string(),
            url: old_server_url,
            office_id: "office".to_string(),
            computer_name: "refresh-test".to_string(),
            connected_at,
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:1".to_string()),
            target_name: Some("Robot".to_string()),
            employee_id: Some(1),
            generation: 8,
        });
    }
    let params = ManagerConnectionParams {
        url: new_server_url.clone(),
        office_id: "office".to_string(),
        routing_headers: HashMap::new(),
        employee_id: 1,
        robot_account_id: 42,
        scope: None,
        robot_binding: RobotBindingMetadata {
            employee_id: 1,
            robot_id: Some("office".to_string()),
            robot_account_id: Some(42),
            namespace: Some("tfrobotserver".to_string()),
            robot_name: Some("Robot".to_string()),
        },
    };
    let token = ExchangedToken {
        access_token: "new-jwt".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: 300,
        scope: None,
    };

    let outcome = reconnect_with_token(&runtime, &params, 8, &token).await;
    assert!(
        matches!(outcome, RefreshOutcome::Renewed(300)),
        "expected successful renewal"
    );
    assert_eq!(
        runtime.runtime_state().await,
        ComputerRuntimeState::JoinedOffice
    );
    assert!(runtime.is_connected().await);
    assert!(
        runtime.connection_status().await.is_some(),
        "successful refresh should expose healthy connection status"
    );

    wait_for("old socket should be closed during refresh", || {
        old_stats.leave_events() == 1 && old_stats.active() == 0 && old_stats.disconnected() == 1
    })
    .await;
    wait_for("new socket should be connected and joined", || {
        new_stats.active() == 1 && new_stats.connected() == 1 && new_stats.join_events() == 1
    })
    .await;

    {
        let connection = runtime.connection_state_snapshot().await;
        let snapshot = connection
            .as_ref()
            .expect("connection snapshot should remain");
        assert_eq!(snapshot.generation, 8);
        assert_eq!(snapshot.computer_name, "refresh-test");
        assert!(
            snapshot.connected_at > connected_at,
            "successful refresh should update connected_at"
        );
    }

    let final_conn = runtime.take_connection_state().await;
    if let Some(cs) = final_conn {
        close_smcp_connection(&runtime, cs)
            .await
            .expect("close refreshed socket");
    }
}

#[tokio::test]
async fn reconnect_with_token_stale_generation_does_not_touch_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, stats) = start_smcp_socket_server().await;

    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "office",
            "stale-test",
        )
        .await
        .expect("connect socket");
    wait_for("server never observed the SMCP socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    {
        let mut guard = runtime.connection_handle_for_test().write_owned().await;
        *guard = Some(ConnectionState {
            profile_name: "p".to_string(),
            url: server_url.clone(),
            office_id: "office".to_string(),
            computer_name: "stale-test".to_string(),
            connected_at: chrono::Utc::now(),
            source_type: "manager_robot".to_string(),
            target_id: Some("manager:1".to_string()),
            target_name: Some("Robot".to_string()),
            employee_id: Some(1),
            generation: 99,
        });
    }
    let params = ManagerConnectionParams {
        url: server_url.clone(),
        office_id: "office".to_string(),
        routing_headers: HashMap::new(),
        employee_id: 1,
        robot_account_id: 42,
        scope: None,
        robot_binding: RobotBindingMetadata {
            employee_id: 1,
            robot_id: Some("office".to_string()),
            robot_account_id: Some(42),
            namespace: Some("tfrobotserver".to_string()),
            robot_name: Some("Robot".to_string()),
        },
    };
    let token = ExchangedToken {
        access_token: "new-jwt".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: 300,
        scope: None,
    };

    let outcome = reconnect_with_token(&runtime, &params, 5, &token).await;
    assert!(
        matches!(outcome, RefreshOutcome::Gone),
        "stale refresh task should exit"
    );
    assert_eq!(
        runtime.runtime_state().await,
        ComputerRuntimeState::JoinedOffice
    );
    assert_eq!(stats.active(), 1);
    assert_eq!(stats.leave_events(), 0);
    assert_eq!(stats.disconnected(), 0);

    let final_conn = runtime.take_connection_state().await;
    if let Some(cs) = final_conn {
        close_smcp_connection(&runtime, cs)
            .await
            .expect("close stale socket");
    }
}

/// `close_smcp_connection` 必须 abort 预刷新任务——断开后不得有后台任务继续 revive 连接。
#[tokio::test]
async fn close_smcp_connection_aborts_refresh_task() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    runtime.start().await.expect("start runtime");
    let (server_url, _stats) = start_smcp_socket_server().await;
    runtime
        .connect_smcp_socketio(
            &server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            "office",
            "abort-test",
        )
        .await
        .expect("connect socket");

    // 代表预刷新任务的长驻后台循环；abort 后计数应停止增长。
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_in_task = counter.clone();
    let task = tokio::spawn(async move {
        loop {
            counter_in_task.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(10)).await;
        }
    });
    runtime.set_refresh_task(task).await;

    let cs = ConnectionState {
        profile_name: "p".to_string(),
        url: server_url,
        office_id: "office".to_string(),
        computer_name: "abort-test".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manager_robot".to_string(),
        target_id: Some("manager:1".to_string()),
        target_name: Some("Robot".to_string()),
        employee_id: Some(1),
        generation: 1,
    };

    close_smcp_connection(&runtime, cs)
        .await
        .expect("close socket and abort refresh task");

    let snapshot = counter.load(Ordering::SeqCst);
    sleep(Duration::from_millis(150)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        snapshot,
        "refresh task must be aborted by close_smcp_connection (no revive)"
    );
}
