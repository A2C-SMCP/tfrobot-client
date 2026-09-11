use super::*;
use crate::services::chat_session::tests::{insert_test_lease, test_target};
use crate::services::keychain::InMemorySecretStore;
use crate::services::manager_client::ManagerClient;
use crate::services::manager_environment::ManagerEnvironment;
use crate::services::manager_token_bridge::{
    ManagerTokenBridgeCompletion, ManagerTokenBridgeRequest, ManagerTokenBridgeSink,
};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::{
    body::{Bytes, Frame},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::sync::{mpsc, Notify};

pub(crate) struct Captured {
    pub(crate) path: String,
    pub(crate) headers: hyper::HeaderMap,
    pub(crate) body: Bytes,
}

pub(crate) struct Fixture {
    pub(crate) service: Arc<ChatSessionService>,
    // Keep the real authenticated coordinator and its filesystem alive.
    _manager: Arc<ManagerContextCoordinator>,
    _directory: tempfile::TempDir,
    server: tokio::task::JoinHandle<()>,
    bridge: tokio::task::JoinHandle<()>,
    pub(crate) scopes: mpsc::UnboundedReceiver<String>,
    pub(crate) requests: mpsc::UnboundedReceiver<Captured>,
    pub(crate) release: Arc<Notify>,
    pub(crate) base: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.release.notify_waiters();
        self.server.abort();
        self.bridge.abort();
    }
}

struct BridgeSink(mpsc::UnboundedSender<ManagerTokenBridgeRequest>);
#[async_trait::async_trait]
impl ManagerTokenBridgeSink for BridgeSink {
    async fn emit_token_request(&self, request: &ManagerTokenBridgeRequest) -> Result<(), String> {
        self.0
            .send(request.clone())
            .map_err(|_| "Bridge closed".into())
    }
}

pub(crate) async fn fixture(status: u16, hold_upload: bool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/proxy/", listener.local_addr().unwrap());
    let (sender, requests) = mpsc::unbounded_channel();
    let (scope_sender, scopes) = mpsc::unbounded_channel();
    let release = Arc::new(Notify::new());
    let release_server = release.clone();
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let sender = sender.clone();
            let scope_sender = scope_sender.clone();
            let release = release_server.clone();
            connections.spawn(async move {
                let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service_fn(
                    move |request: Request<hyper::body::Incoming>| {
                        let sender = sender.clone();
                        let scope_sender = scope_sender.clone();
                        let release = release.clone();
                        async move {
                            let path = request.uri().to_string();
                            let headers = request.headers().clone();
                            let body = request.into_body().collect().await.unwrap().to_bytes();
                            if path.ends_with("/oauth/token") {
                                let form: HashMap<_, _> = url::form_urlencoded::parse(&body).into_owned().collect();
                                assert_eq!(form["audience"], "robot:turingfocus:robot-42");
                                let scope = &form["scope"];
                                let token = match scope.as_str() {
                                    "config:write" => "upload-token",
                                    "config:read" => "resource-token",
                                    _ => panic!("Unexpected transfer scope"),
                                };
                                scope_sender.send(scope.clone()).unwrap();
                                return Ok::<_, std::convert::Infallible>(Response::builder().header("Content-Type", "application/json")
                                    .body(Full::new(Bytes::from(json!({"access_token":token,"token_type":"Bearer","expires_in":300,"scope":scope}).to_string())).boxed()).unwrap());
                            }
                            if path.starts_with("/proxy/resource/s3?") {
                                assert!(headers[COOKIE].to_str().unwrap().contains("tfUserToken=resource-token"));
                                let uri = Url::parse(&format!("http://test{path}")).unwrap().query_pairs()
                                    .find(|(key, _)| key == "s3uri").unwrap().1.into_owned();
                                let range = headers.get("range").and_then(|h| h.to_str().ok()).map(str::to_owned);
                                sender.send(Captured { path, headers, body }).unwrap();
                                if hold_upload { release.notified().await; }
                                let mut response = Response::builder().status(status).header("Content-Type", "image/png")
                                    .header("Cache-Control", "public,max-age=31536000").header("Set-Cookie", "secret=bad");
                                if uri.contains("large.bin") || uri.contains("slow.bin") {
                                    let slow = uri.contains("slow.bin");
                                    let body = StreamBody::new(futures_util::stream::unfold((0usize, release), move |(index, release)| async move {
                                        if index == 2048 { return None; }
                                        if slow && index == 1 { release.notified().await; }
                                        let bytes = Bytes::from_static(&[42; 65536]);
                                        Some((Ok::<_, std::convert::Infallible>(Frame::data(bytes)), (index + 1, release)))
                                    })).boxed();
                                    return Ok(response.header("Content-Length", 128 * 1024 * 1024).body(body).unwrap());
                                }
                                let mut bytes = Bytes::from_static(&[0, 255, 128, 13, 10, 1]);
                                if uri.contains("actual.png") {
                                    bytes = Bytes::from(STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGP4z8AARAwQCgAf7gP9i18U1AAAAABJRU5ErkJggg==").unwrap());
                                }
                                if uri.contains("range.png") {
                                    match range.as_deref() {
                                        Some("bytes=1-3") => { response = response.status(206).header("Content-Range", "bytes 1-3/6").header("Accept-Ranges", "bytes"); bytes = bytes.slice(1..4); },
                                        Some("bytes=9-") => { response = response.status(416).header("Content-Range", "bytes */6"); bytes = Bytes::new(); },
                                        _ => {},
                                    }
                                }
                                return Ok(response.header("Content-Length", bytes.len()).body(Full::new(bytes).boxed()).unwrap());
                            }
                            let data = if path.ends_with("/auth/login-by-password") {
                                json!({"token":"manager-test-token","userId":9,"accountId":16,"accountName":"test"})
                            } else if path.ends_with("/auth/me") {
                                json!({"id":9,"nickname":"Test","email":"test@example.com","phone":"", "accountId":16,"accountName":"test","organizationId":9,"organizationName":"Test","organizationType":"enterprise","permissions":[]})
                            } else if path.starts_with("/proxy/v1/utils/cos/presign?") {
                                json!({"url":"https://storage.example/image.png?signature=test"})
                            } else {
                                assert_eq!(path, "/proxy/v1/dashboard/remote/source/cos/upload");
                                json!({"uri":"s3://robot-bucket/image.png"})
                            };
                            let is_upload = path.ends_with("/cos/upload");
                            if is_upload { assert!(headers[COOKIE].to_str().unwrap().contains("tfUserToken=upload-token")); }
                            if path.starts_with("/proxy/v1/utils/cos/presign?") { assert!(headers[COOKIE].to_str().unwrap().contains("tfUserToken=resource-token")); }
                            sender.send(Captured { path, headers, body }).unwrap();
                            if hold_upload && is_upload { release.notified().await; }
                            let response_status = if is_upload { status } else { 200 };
                            Ok::<_, std::convert::Infallible>(Response::builder()
                                .status(response_status).header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(json!({"code":response_status,"message":"test","data":data}).to_string())).boxed()).unwrap())
                        }
                    })).await;
            });
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let manager = Arc::new(ManagerContextCoordinator::new_with_base_url_override(
        Arc::new(ManagerClient::new_with_secret_store(
            InMemorySecretStore::shared(),
        )),
        Arc::new(SettingsService::new(directory.path().to_path_buf())),
        base.clone(),
    ));
    manager
        .login(
            ManagerEnvironment::Staging,
            "test@example.com",
            "test-password",
        )
        .await
        .unwrap();
    let (bridge_sender, mut bridge_receiver) =
        mpsc::unbounded_channel::<ManagerTokenBridgeRequest>();
    manager
        .set_token_bridge_sink(Arc::new(BridgeSink(bridge_sender)))
        .await;
    manager.set_token_bridge_ready("test-bridge", true).await;
    let bridge_manager = manager.clone();
    // Exercise the real registered token HTTP transport, substituting only the JS consumer.
    let bridge = tokio::spawn(async move {
        while let Some(request) = bridge_receiver.recv().await {
            let form = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:token-exchange",
                    ),
                    ("subject_token", request.user_jwt.as_str()),
                    ("subject_token_type", "urn:ietf:params:oauth:token-type:jwt"),
                    ("audience", request.audience.as_str()),
                    ("scope", request.scope.as_deref().unwrap()),
                    ("token_profile", "session"),
                ])
                .finish();
            let (status, body, _) = bridge_manager
                .token_bridge_http_request(&request.request_id, request.generation, form)
                .await
                .unwrap();
            assert_eq!(status, 200);
            let token: serde_json::Value = serde_json::from_str(&body).unwrap();
            bridge_manager
                .complete_token_bridge_request(
                    &request.request_id,
                    request.generation,
                    ManagerTokenBridgeCompletion::Success {
                        access_token: token["access_token"].as_str().unwrap().into(),
                        token_type: "Bearer".into(),
                        expires_in: 300,
                        scope: Some(token["scope"].as_str().unwrap().into()),
                    },
                )
                .await
                .unwrap();
        }
    });
    let service = Arc::new(ChatSessionService::new(Arc::downgrade(&manager)));
    let mut target = test_target(
        Url::parse(&base).unwrap(),
        manager.snapshot().await.context_key.unwrap(),
    );
    target.robot_account_id = "turingfocus:robot-42".into();
    target.manager_generation = manager.capture_authenticated_generation().await.unwrap();
    insert_test_lease(&service, "lease", target, 1).await;
    let mut result = Fixture {
        service,
        _manager: manager,
        _directory: directory,
        server,
        bridge,
        scopes,
        requests,
        release,
        base,
    };
    // Login and current-user requests have already completed over real HTTP.
    result.requests.recv().await.unwrap();
    result.requests.recv().await.unwrap();
    result
}

fn file() -> ChatUploadFile {
    ChatUploadFile {
        name: "图.png".into(),
        mime_type: "image/png".into(),
        data_base64: STANDARD.encode([0, 255, 128, 13, 10, 1]),
    }
}

#[tokio::test]
async fn multipart_bytes_and_private_resource_use_the_authenticated_lease() {
    let mut fixture = fixture(200, false).await;
    let id = fixture.service.prepare_transfer("lease").await.unwrap();
    let response = fixture
        .service
        .upload(
            "lease",
            &id,
            &format!("{}{}", fixture.base, UPLOAD_PATH),
            file(),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    assert!(response.body.contains("s3://robot-bucket/image.png"));
    assert_eq!(fixture.scopes.recv().await.unwrap(), "config:write");
    assert_eq!(
        fixture.service.credential("lease").await.unwrap().token,
        "short-chat-token"
    );
    let request = fixture.requests.recv().await.unwrap();
    assert_eq!(request.path, "/proxy/v1/dashboard/remote/source/cos/upload");
    assert_eq!(
        request.headers[COOKIE],
        "tfNamespace=tenant-acme; tfRobotId=rid-42; tfUserToken=upload-token"
    );
    assert_eq!(request.headers["X-TF-Route"], "robot-42");
    assert!(!request.headers.contains_key("authorization"));
    let content_type = request.headers[CONTENT_TYPE].to_str().unwrap();
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap();
    let body = &request.body;
    assert!(body.starts_with(format!("--{boundary}\r\n").as_bytes()));
    assert!(body.ends_with(format!("\r\n--{boundary}--\r\n").as_bytes()));
    let split = body.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let part_headers = std::str::from_utf8(&body[..split]).unwrap();
    assert!(part_headers.contains("name=\"file\""));
    assert!(part_headers.contains("filename=\"图.png\""));
    assert!(part_headers.contains("image/png"));
    assert_eq!(&body[split + 4..split + 10], &[0, 255, 128, 13, 10, 1]);
    assert!(fixture.service.transfers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn permission_failure_is_preserved_and_slot_is_released() {
    let fixture = fixture(403, false).await;
    let id = fixture.service.prepare_transfer("lease").await.unwrap();
    let response = fixture
        .service
        .upload(
            "lease",
            &id,
            &format!("{}{}", fixture.base, UPLOAD_PATH),
            file(),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 403);
    assert!(fixture.service.transfers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_before_execute_prevents_network_and_cross_lease_cancel_is_ignored() {
    let mut fixture = fixture(200, false).await;
    let id = fixture.service.prepare_transfer("lease").await.unwrap();
    fixture.service.cancel_transfer("other-lease", &id);
    assert!(fixture.service.transfers.lock().unwrap().contains_key(&id));
    fixture.service.cancel_transfer("lease", &id);
    assert!(fixture
        .service
        .upload(
            "lease",
            &id,
            &format!("{}{}", fixture.base, UPLOAD_PATH),
            file()
        )
        .await
        .is_err());
    assert!(fixture.requests.try_recv().is_err());
    assert!(fixture.service.transfers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancelling_inflight_or_closing_lease_terminates_the_http_wait() {
    for close_lease in [false, true] {
        let mut fixture = fixture(200, true).await;
        let id = fixture.service.prepare_transfer("lease").await.unwrap();
        let service = fixture.service.clone();
        let upload_id = id.clone();
        let url = format!("{}{}", fixture.base, UPLOAD_PATH);
        let request =
            tokio::spawn(async move { service.upload("lease", &upload_id, &url, file()).await });
        tokio::time::timeout(Duration::from_secs(2), fixture.requests.recv())
            .await
            .unwrap()
            .unwrap();
        if close_lease {
            fixture.service.close("lease").await;
        } else {
            fixture.service.cancel_transfer("lease", &id);
        }
        assert!(tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(fixture.service.transfers.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn rejects_wrong_routes_invalid_files_and_excess_reservations() {
    let mut fixture = fixture(200, false).await;
    for url in [
        "https://attacker.example/upload".to_string(),
        format!("{}{}?token=bad", fixture.base, UPLOAD_PATH),
    ] {
        let id = fixture.service.prepare_transfer("lease").await.unwrap();
        assert!(fixture
            .service
            .upload("lease", &id, &url, file())
            .await
            .is_err());
    }
    let id = fixture.service.prepare_transfer("lease").await.unwrap();
    let mut oversized = file();
    oversized.data_base64 = "A".repeat(MAX_FILE_BYTES.div_ceil(3) * 4 + 4);
    assert!(fixture
        .service
        .upload(
            "lease",
            &id,
            &format!("{}{}", fixture.base, UPLOAD_PATH),
            oversized
        )
        .await
        .is_err());
    for uri in [
        "file:///etc/passwd",
        "s3://bucket",
        "s3://user:password@bucket/file",
        "s3://bucket/key?token=bad",
    ] {
        assert!(fixture
            .service
            .register_resource("lease", uri)
            .await
            .is_err());
    }
    assert!(fixture.requests.try_recv().is_err());
    for _ in 0..MAX_PENDING_PER_LEASE {
        fixture.service.prepare_transfer("lease").await.unwrap();
    }
    assert!(fixture.service.prepare_transfer("lease").await.is_err());
    fixture.service.close("lease").await;
    assert!(fixture.service.transfers.lock().unwrap().is_empty());
    assert!(fixture.service.prepare_transfer("lease").await.is_err());
}
