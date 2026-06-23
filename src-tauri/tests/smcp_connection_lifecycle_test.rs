use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use smcp_computer::mcp_clients::manager::MCPServerManager;
use smcp_computer::mcp_clients::model::MCPServerInput;
use smcp_computer::socketio_client::SmcpComputerClient;
use socketioxide::extract::{AckSender, SocketRef};
use socketioxide::SocketIo;
use tfrobot_client_lib::commands::connection::{
    close_smcp_connection, connect_smcp_core, reconnect_with_token, try_install_refreshed_client,
    ConnectionProfile, ConnectionState, ManagerConnectionParams, RefreshOutcome, SwapResult,
};
use tfrobot_client_lib::services::computer::{
    ComputerInstance, ComputerInstanceRuntime, RobotBindingMetadata,
};
use tfrobot_client_lib::services::manager_client::ExchangedToken;
use tfrobot_client_lib::AppState;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tower::service_fn;
use tower::Layer;

#[allow(dead_code)]
mod common;

const SERVER_JOIN_OFFICE: &str = "server:join_office";
const SERVER_LEAVE_OFFICE: &str = "server:leave_office";
const TEST_INSTANCE_ID: &str = "test-computer";

async fn create_test_runtime(state: &AppState) -> ComputerInstanceRuntime {
    state
        .config
        .add_computer_instance(ComputerInstance {
            id: TEST_INSTANCE_ID.to_string(),
            name: "Test Computer".to_string(),
            ..ComputerInstance::new("", "")
        })
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
    let stats = Arc::new(SocketStats::default());
    let (socket_layer, io) = SocketIo::new_layer();

    let connect_stats = stats.clone();
    io.ns("/smcp", move |socket: SocketRef| {
        connect_stats.active.fetch_add(1, Ordering::SeqCst);
        connect_stats.connected.fetch_add(1, Ordering::SeqCst);

        let join_stats = connect_stats.clone();
        socket.on(SERVER_JOIN_OFFICE, move |ack: AckSender| {
            join_stats.join_events.fetch_add(1, Ordering::SeqCst);
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

#[tokio::test]
async fn close_smcp_connection_closes_underlying_socket_after_leaving_office() {
    let (server_url, stats) = start_smcp_socket_server().await;
    let manager = Arc::new(RwLock::new(Some(MCPServerManager::new())));
    let inputs: Arc<RwLock<HashMap<String, MCPServerInput>>> =
        Arc::new(RwLock::new(HashMap::new()));

    let client = SmcpComputerClient::new(
        &server_url,
        manager,
        "lifecycle-test-computer".to_string(),
        None,
        inputs,
        None,
    )
    .await
    .expect("connect smcp client");

    client
        .join_office("lifecycle-office")
        .await
        .expect("join office");

    wait_for("server never observed the SMCP socket connection", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    close_smcp_connection(ConnectionState {
        client,
        profile_name: "lifecycle-profile".to_string(),
        url: server_url,
        office_id: "lifecycle-office".to_string(),
        computer_name: "lifecycle-test-computer".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manual_smcp".to_string(),
        target_id: Some("lifecycle-target".to_string()),
        target_name: Some("lifecycle-profile".to_string()),
        employee_id: None,
        generation: 0,
        refresh_task: None,
    })
    .await;

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
async fn failed_profile_switch_keeps_existing_smcp_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    let (server_url, stats) = start_smcp_socket_server().await;
    let manager = runtime.manager.clone();
    let inputs = runtime.inputs.clone();

    let client = SmcpComputerClient::new(
        &server_url,
        manager,
        "existing-computer".to_string(),
        None,
        inputs,
        None,
    )
    .await
    .expect("connect existing smcp client");

    client
        .join_office("existing-office")
        .await
        .expect("join existing office");

    wait_for("server never observed the existing SMCP socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    {
        let mut conn = runtime.connection.write().await;
        *conn = Some(ConnectionState {
            client,
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
            refresh_task: None,
        });
    }

    let err = connect_smcp_core(&state, TEST_INSTANCE_ID, "missing-profile".to_string())
        .await
        .expect_err("missing profile should fail");
    assert_eq!(err, "Profile not found: missing-profile");

    let conn = runtime.connection.read().await;
    let connection = conn
        .as_ref()
        .expect("existing connection should be preserved");
    assert_eq!(connection.profile_name, "existing-profile");
    drop(conn);

    assert_eq!(stats.active(), 1);
    assert_eq!(stats.disconnected(), 0);

    let existing_connection = {
        let mut conn = runtime.connection.write().await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(connection).await;
    }
}

#[tokio::test]
async fn profile_switch_to_different_robot_requires_disconnect() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let runtime = create_test_runtime(&state).await;
    let (old_server_url, old_stats) = start_smcp_socket_server().await;
    let (new_server_url, new_stats) = start_smcp_socket_server().await;

    let old_client = SmcpComputerClient::new(
        &old_server_url,
        runtime.manager.clone(),
        "old-computer".to_string(),
        None,
        runtime.inputs.clone(),
        None,
    )
    .await
    .expect("connect old smcp client");

    old_client
        .join_office("old-office")
        .await
        .expect("join old office");

    wait_for("server never observed the old SMCP socket", || {
        old_stats.active() == 1 && old_stats.connected() == 1
    })
    .await;

    {
        let mut conn = runtime.connection.write().await;
        *conn = Some(ConnectionState {
            client: old_client,
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
            refresh_task: None,
        });
    }

    state
        .config
        .save_profiles_for_instance(
            TEST_INSTANCE_ID,
            &[ConnectionProfile {
                name: "new-profile".to_string(),
                url: new_server_url.clone(),
                namespace: "/smcp".to_string(),
                office_id: "new-office".to_string(),
                computer_name: "new-computer".to_string(),
                api_key_ref: None,
                headers: HashMap::new(),
                auto_connect: true,
                auto_reconnect: true,
            }],
        )
        .expect("save profiles");

    let err = connect_smcp_core(&state, TEST_INSTANCE_ID, "new-profile".to_string())
        .await
        .expect_err("switching robots without disconnect should fail");
    assert!(
        err.contains("disconnect before switching Robot"),
        "error should instruct the user to disconnect first, got: {err}"
    );

    let conn = runtime.connection.read().await;
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
        let mut conn = runtime.connection.write().await;
        conn.take()
    };
    if let Some(connection) = old_connection {
        close_smcp_connection(connection).await;
    }
}

#[tokio::test]
async fn profile_connect_rejects_robot_already_connected_by_another_instance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    create_test_runtime(&state).await;
    let (server_url, stats) = start_smcp_socket_server().await;

    let other_instance_id = "other-computer";
    state
        .config
        .add_computer_instance(ComputerInstance {
            id: other_instance_id.to_string(),
            name: "Other Computer".to_string(),
            ..ComputerInstance::new("", "")
        })
        .unwrap();
    let other_runtime = state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(other_instance_id)
                .unwrap(),
        )
        .await;

    let other_client = SmcpComputerClient::new(
        &server_url,
        other_runtime.manager.clone(),
        "other-computer".to_string(),
        None,
        other_runtime.inputs.clone(),
        None,
    )
    .await
    .expect("connect other smcp client");
    other_client
        .join_office("shared-office")
        .await
        .expect("join shared office");
    wait_for("server never observed the other SMCP socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    {
        let mut conn = other_runtime.connection.write().await;
        *conn = Some(ConnectionState {
            client: other_client,
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
            refresh_task: None,
        });
    }

    state
        .config
        .save_profiles_for_instance(
            TEST_INSTANCE_ID,
            &[ConnectionProfile {
                name: "target-profile".to_string(),
                url: server_url,
                namespace: "/smcp".to_string(),
                office_id: "shared-office".to_string(),
                computer_name: "target-computer".to_string(),
                api_key_ref: None,
                headers: HashMap::new(),
                auto_connect: true,
                auto_reconnect: true,
            }],
        )
        .expect("save target profile");

    let err = connect_smcp_core(&state, TEST_INSTANCE_ID, "target-profile".to_string())
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
        let mut conn = other_runtime.connection.write().await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(connection).await;
    }
}

#[tokio::test]
async fn concurrent_profile_connect_same_robot_allows_only_one_instance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let target_runtime = create_test_runtime(&state).await;
    let (server_url, stats) = start_smcp_socket_server().await;

    let other_instance_id = "other-computer";
    state
        .config
        .add_computer_instance(ComputerInstance {
            id: other_instance_id.to_string(),
            name: "Other Computer".to_string(),
            ..ComputerInstance::new("", "")
        })
        .unwrap();
    let other_runtime = state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(other_instance_id)
                .unwrap(),
        )
        .await;

    let profile_name = "shared-profile";
    for instance_id in [TEST_INSTANCE_ID, other_instance_id] {
        state
            .config
            .save_profiles_for_instance(
                instance_id,
                &[ConnectionProfile {
                    name: profile_name.to_string(),
                    url: server_url.clone(),
                    namespace: "/smcp".to_string(),
                    office_id: "shared-office".to_string(),
                    computer_name: format!("{instance_id}-computer"),
                    api_key_ref: None,
                    headers: HashMap::new(),
                    auto_connect: true,
                    auto_reconnect: true,
                }],
            )
            .expect("save profile");
    }

    let (target_result, other_result) = tokio::join!(
        connect_smcp_core(&state, TEST_INSTANCE_ID, profile_name.to_string()),
        connect_smcp_core(&state, other_instance_id, profile_name.to_string())
    );

    let success_count = usize::from(target_result.is_ok()) + usize::from(other_result.is_ok());
    assert_eq!(
        success_count, 1,
        "exactly one concurrent connection may win; target={target_result:?} other={other_result:?}"
    );
    let error = target_result.err().or_else(|| other_result.err()).unwrap();
    assert!(
        error.contains("already connected") || error.contains("disconnect before switching Robot"),
        "losing connection should fail by duplicate-connection guard, got: {error}"
    );

    wait_for("server should only have one active socket", || {
        stats.active() == 1 && stats.connected() == 1
    })
    .await;

    for runtime in [target_runtime, other_runtime] {
        let existing_connection = {
            let mut conn = runtime.connection.write().await;
            conn.take()
        };
        if let Some(connection) = existing_connection {
            close_smcp_connection(connection).await;
        }
    }
}

// ───────────────────── 预刷新重连：generation 守卫 + abort（TFRC-11 / C1） ─────────────────────

async fn connect_client(
    url: &str,
    manager: &Arc<RwLock<Option<MCPServerManager>>>,
    inputs: &Arc<RwLock<HashMap<String, MCPServerInput>>>,
    name: &str,
    office: &str,
) -> SmcpComputerClient {
    let c = SmcpComputerClient::new(
        url,
        manager.clone(),
        name.to_string(),
        None,
        inputs.clone(),
        None,
    )
    .await
    .expect("client connect");
    c.join_office(office).await.expect("join office");
    c
}

/// generation 守卫：换连接仅在代际匹配时生效；用户期间断开/改连（代际不符）时旧刷新任务
/// 走 Stale 分支、不覆盖新连接。
#[tokio::test]
async fn try_install_refreshed_client_respects_generation_guard() {
    let (server_url, _stats) = start_smcp_socket_server().await;
    let manager = Arc::new(RwLock::new(Some(MCPServerManager::new())));
    let inputs: Arc<RwLock<HashMap<String, MCPServerInput>>> =
        Arc::new(RwLock::new(HashMap::new()));

    let client_a = connect_client(&server_url, &manager, &inputs, "gen-test", "office").await;
    let connection = Arc::new(RwLock::new(Some(ConnectionState {
        client: client_a,
        profile_name: "p".to_string(),
        url: server_url.clone(),
        office_id: "office".to_string(),
        computer_name: "gen-test".to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manager_robot".to_string(),
        target_id: Some("manager:1".to_string()),
        target_name: Some("Robot".to_string()),
        employee_id: Some(1),
        generation: 7,
        refresh_task: None,
    })));

    // 代际匹配 → Replaced（拿回旧 client A 关闭）。
    let client_b = connect_client(&server_url, &manager, &inputs, "gen-test", "office").await;
    match try_install_refreshed_client(&connection, 7, client_b).await {
        SwapResult::Replaced(old) => {
            old.leave_office("office").await.ok();
            old.disconnect().await.ok();
        }
        SwapResult::Stale(_) => panic!("expected Replaced on matching generation"),
    }

    // 代际不符（模拟用户已改连别的机器人）→ Stale（新 client C 被交还、不覆盖现连接）。
    let client_c = connect_client(&server_url, &manager, &inputs, "gen-test", "office").await;
    match try_install_refreshed_client(&connection, 999, client_c).await {
        SwapResult::Stale(new) => {
            new.leave_office("office").await.ok();
            new.disconnect().await.ok();
        }
        SwapResult::Replaced(_) => panic!("expected Stale on non-matching generation"),
    }

    // 现连接仍是代际 7（client B）。
    {
        let guard = connection.read().await;
        assert_eq!(guard.as_ref().map(|c| c.generation), Some(7));
    }
    let final_conn = {
        let mut conn = connection.write().await;
        conn.take()
    };
    if let Some(cs) = final_conn {
        close_smcp_connection(cs).await;
    }
}

/// build_and_join 失败时，预刷新必须**回滚**（把旧连接重新 join_office）、返回 `Retry`，
/// 不留「已 leave 却仍标记 connected」的僵尸——旧连接仍在、仍属同一代际。
#[tokio::test]
async fn reconnect_with_token_rolls_back_old_connection_on_build_failure() {
    let (server_url, stats) = start_smcp_socket_server().await;
    let manager = Arc::new(RwLock::new(Some(MCPServerManager::new())));
    let inputs: Arc<RwLock<HashMap<String, MCPServerInput>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // 旧连接连到「好」server（initial join → join_events = 1）。
    let old_client =
        connect_client(&server_url, &manager, &inputs, "rollback-test", "office").await;
    let connection = Arc::new(RwLock::new(Some(ConnectionState {
        client: old_client,
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
        refresh_task: None,
    })));

    // 预刷新参数指向一个「死」地址 → build_and_join 必失败。
    let dead_url = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        format!("http://{}", addr)
    };
    let params = ManagerConnectionParams {
        url: dead_url,
        computer_name: "rollback-test".to_string(),
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

    let outcome = reconnect_with_token(&connection, &manager, &inputs, &params, 5, &token).await;

    // build 失败 → Retry（不替换连接）。
    assert!(
        matches!(outcome, RefreshOutcome::Retry),
        "expected Retry on build failure"
    );

    // 发生过一次 leave（step 2 腾 room）+ 一次回滚 re-join；连同 initial join 共 2 次 join。
    wait_for("expected rollback re-join (join_events == 2)", || {
        stats.join_events() == 2
    })
    .await;
    assert_eq!(stats.leave_events(), 1, "exactly one leave before rollback");
    // 旧连接从未断开（leave_office 不断 socket），回滚后仍在 room。
    assert_eq!(
        stats.active(),
        1,
        "old connection must stay connected (rolled back)"
    );

    // 现连接仍是同一代际、未变僵尸 None。
    {
        let guard = connection.read().await;
        assert_eq!(
            guard.as_ref().map(|c| c.generation),
            Some(5),
            "old connection must remain at its generation after rollback"
        );
    }

    let final_conn = {
        let mut conn = connection.write().await;
        conn.take()
    };
    if let Some(cs) = final_conn {
        close_smcp_connection(cs).await;
    }
}

/// `close_smcp_connection` 必须 abort 预刷新任务——断开后不得有后台任务继续 revive 连接。
#[tokio::test]
async fn close_smcp_connection_aborts_refresh_task() {
    let (server_url, _stats) = start_smcp_socket_server().await;
    let manager = Arc::new(RwLock::new(Some(MCPServerManager::new())));
    let inputs: Arc<RwLock<HashMap<String, MCPServerInput>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let client = connect_client(&server_url, &manager, &inputs, "abort-test", "office").await;

    // 代表预刷新任务的长驻后台循环；abort 后计数应停止增长。
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_in_task = counter.clone();
    let task = tokio::spawn(async move {
        loop {
            counter_in_task.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(10)).await;
        }
    });

    let cs = ConnectionState {
        client,
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
        refresh_task: Some(task),
    };

    close_smcp_connection(cs).await;

    let snapshot = counter.load(Ordering::SeqCst);
    sleep(Duration::from_millis(150)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        snapshot,
        "refresh task must be aborted by close_smcp_connection (no revive)"
    );
}
