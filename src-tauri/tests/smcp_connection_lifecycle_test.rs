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
    close_smcp_connection, connect_smcp_core, ConnectionProfile, ConnectionState,
};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tower::service_fn;
use tower::Layer;

#[allow(dead_code)]
mod common;

const SERVER_JOIN_OFFICE: &str = "server:join_office";
const SERVER_LEAVE_OFFICE: &str = "server:leave_office";

#[derive(Default)]
struct SocketStats {
    active: AtomicUsize,
    connected: AtomicUsize,
    disconnected: AtomicUsize,
    leave_events: AtomicUsize,
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
    let (server_url, stats) = start_smcp_socket_server().await;
    let manager = state.manager.clone();
    let inputs = state.inputs.clone();

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
        let mut conn = state.connection.write().await;
        *conn = Some(ConnectionState {
            client,
            profile_name: "existing-profile".to_string(),
            url: server_url,
            office_id: "existing-office".to_string(),
            computer_name: "existing-computer".to_string(),
            connected_at: chrono::Utc::now(),
        });
    }

    let err = connect_smcp_core(&state, "missing-profile".to_string())
        .await
        .expect_err("missing profile should fail");
    assert_eq!(err, "Profile not found: missing-profile");

    let conn = state.connection.read().await;
    let connection = conn
        .as_ref()
        .expect("existing connection should be preserved");
    assert_eq!(connection.profile_name, "existing-profile");
    drop(conn);

    assert_eq!(stats.active(), 1);
    assert_eq!(stats.disconnected(), 0);

    let existing_connection = {
        let mut conn = state.connection.write().await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(connection).await;
    }
}

#[tokio::test]
async fn successful_profile_switch_closes_previous_smcp_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = common::create_test_app_state(tmp.path());
    let (old_server_url, old_stats) = start_smcp_socket_server().await;
    let (new_server_url, new_stats) = start_smcp_socket_server().await;

    let old_client = SmcpComputerClient::new(
        &old_server_url,
        state.manager.clone(),
        "old-computer".to_string(),
        None,
        state.inputs.clone(),
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
        let mut conn = state.connection.write().await;
        *conn = Some(ConnectionState {
            client: old_client,
            profile_name: "old-profile".to_string(),
            url: old_server_url,
            office_id: "old-office".to_string(),
            computer_name: "old-computer".to_string(),
            connected_at: chrono::Utc::now(),
        });
    }

    state
        .config
        .save_profiles(&[ConnectionProfile {
            name: "new-profile".to_string(),
            url: new_server_url.clone(),
            namespace: "/smcp".to_string(),
            office_id: "new-office".to_string(),
            computer_name: "new-computer".to_string(),
            api_key_ref: None,
            headers: HashMap::new(),
            auto_connect: true,
            auto_reconnect: true,
        }])
        .expect("save profiles");

    connect_smcp_core(&state, "new-profile".to_string())
        .await
        .expect("connect new profile");

    wait_for("server never observed the new SMCP socket", || {
        new_stats.active() == 1 && new_stats.connected() == 1
    })
    .await;
    wait_for("old SMCP connection was not closed after switch", || {
        old_stats.leave_events() == 1 && old_stats.active() == 0 && old_stats.disconnected() == 1
    })
    .await;

    let conn = state.connection.read().await;
    let connection = conn.as_ref().expect("new connection should be retained");
    assert_eq!(connection.profile_name, "new-profile");
    assert_eq!(connection.office_id, "new-office");
    drop(conn);

    let new_connection = {
        let mut conn = state.connection.write().await;
        conn.take()
    };
    if let Some(connection) = new_connection {
        close_smcp_connection(connection).await;
    }
}
