use super::super::transfers::tests::fixture as resource_fixture;
use super::*;
use futures_util::future::join_all;

#[tokio::test]
async fn real_loopback_stream_preserves_bytes_and_routes_authorized_proxy_request() {
    let mut fixture = resource_fixture(200, false).await;
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/图 a.png")
        .await
        .unwrap();
    let client = reqwest::Client::new();
    // A stable capability performs a fresh authorized read on every request, without a signed-URL cache.
    for _ in 0..2 {
        let response = client
            .get(&handle.url)
            .header("Origin", "tauri://localhost")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-store, private");
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(
            response.headers()["access-control-allow-origin"],
            "tauri://localhost"
        );
        assert!(!response.headers().contains_key("set-cookie"));
        assert_eq!(
            &response.bytes().await.unwrap()[..],
            &[0, 255, 128, 13, 10, 1]
        );
        assert_eq!(fixture.scopes.recv().await.unwrap(), "config:read");
        let request = fixture.requests.recv().await.unwrap();
        assert_eq!(request.headers["X-TF-Route"], "robot-42");
        assert_eq!(request.headers["cache-control"], "no-cache, no-store");
        let url = Url::parse(&format!("http://test{}", request.path)).unwrap();
        assert_eq!(url.path(), "/proxy/resource/s3");
        assert_eq!(
            url.query_pairs().find(|(k, _)| k == "s3uri").unwrap().1,
            "s3://bucket/图 a.png"
        );
    }
    assert_eq!(
        fixture.service.credential("lease").await.unwrap().token,
        "short-chat-token"
    );
}

#[tokio::test]
async fn range_status_and_bytes_are_preserved_and_ignored_range_is_not_faked() {
    let fixture = resource_fixture(200, false).await;
    let client = reqwest::Client::new();
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/range.png")
        .await
        .unwrap();
    let response = client
        .get(&handle.url)
        .header("Range", "bytes=1-3")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.headers()["content-range"], "bytes 1-3/6");
    assert_eq!(&response.bytes().await.unwrap()[..], &[255, 128, 13]);
    let response = client
        .get(&handle.url)
        .header("Range", "bytes=9-")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 416);
    assert_eq!(response.headers()["content-range"], "bytes */6");
    let response = client.head(&handle.url).send().await.unwrap();
    assert_eq!(response.headers()["content-length"], "6");
    assert!(response.bytes().await.unwrap().is_empty());
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/ordinary.png")
        .await
        .unwrap();
    let response = client
        .get(&handle.url)
        .header("Range", "bytes=1-3")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["accept-ranges"], "none");
    assert_eq!(response.bytes().await.unwrap().len(), 6);
}

#[tokio::test]
async fn loopback_rejects_wrong_host_origin_method_range_and_unregistered_resources() {
    let mut fixture = resource_fixture(200, false).await;
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/image.png")
        .await
        .unwrap();
    let client = reqwest::Client::new();
    for response in [
        client
            .get(&handle.url)
            .header("Host", "attacker.test")
            .send()
            .await
            .unwrap(),
        client
            .get(&handle.url)
            .header("Origin", "https://attacker.test")
            .send()
            .await
            .unwrap(),
        client.post(&handle.url).send().await.unwrap(),
        client
            .get(&handle.url)
            .header("Range", "bytes=1-3,5-6")
            .send()
            .await
            .unwrap(),
    ] {
        assert_eq!(response.status(), 400);
    }
    assert_eq!(
        client
            .get(handle.url.replace(&handle.id, "missing"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert!(fixture.requests.try_recv().is_err());
}

#[tokio::test]
async fn more_than_four_resources_queue_and_all_finish_without_manual_retry() {
    let fixture = resource_fixture(200, false).await;
    let client = reqwest::Client::new();
    let mut reads = Vec::new();
    for number in 0..12 {
        let handle = fixture
            .service
            .register_resource("lease", &format!("s3://bucket/{number}.png"))
            .await
            .unwrap();
        let client = client.clone();
        reads.push(tokio::spawn(async move {
            let response = client.get(handle.url).send().await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.bytes().await.unwrap().len(), 6);
        }));
    }
    for result in tokio::time::timeout(Duration::from_secs(10), join_all(reads))
        .await
        .unwrap()
    {
        result.unwrap();
    }
    assert_eq!(
        fixture.service.resources.slots.available_permits(),
        RESOURCE_CONCURRENCY
    );
}

#[tokio::test]
async fn release_and_lease_close_revoke_capabilities_and_cancel_inflight_reads() {
    for close in [false, true] {
        let mut fixture = resource_fixture(200, true).await;
        let handle = fixture
            .service
            .register_resource("lease", "s3://bucket/image.png")
            .await
            .unwrap();
        fixture
            .service
            .release_resource("different-lease", &handle.id);
        assert!(fixture.service.resource_entry("lease", &handle.id).is_ok());
        let url = handle.url.clone();
        let read = tokio::spawn(async move { reqwest::get(url).await });
        tokio::time::timeout(Duration::from_secs(2), fixture.requests.recv())
            .await
            .unwrap()
            .unwrap();
        if close {
            fixture.service.close("lease").await;
        } else {
            fixture.service.release_resource("lease", &handle.id);
        }
        let result = tokio::time::timeout(Duration::from_secs(2), read)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err() || result.unwrap().status() == 404);
        assert_eq!(reqwest::get(&handle.url).await.unwrap().status(), 404);
        assert!(fixture.service.resource_entry("lease", &handle.id).is_err());
    }
}

#[tokio::test]
async fn save_and_open_write_exact_bytes_and_cancelled_save_preserves_existing_destination() {
    let fixture = resource_fixture(200, false).await;
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/image.png")
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("图.png");
    fixture
        .service
        .save_resource("lease", &handle.id, &destination)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        [0, 255, 128, 13, 10, 1]
    );
    let mut opened_path = None;
    fixture
        .service
        .open_resource("lease", &handle.id, "image.png", |path| {
            assert_eq!(std::fs::read(path).unwrap(), [0, 255, 128, 13, 10, 1]);
            opened_path = Some(path.to_path_buf());
            Ok(())
        })
        .await
        .unwrap();
    let opened_path = opened_path.unwrap();
    assert!(opened_path.exists());
    fixture.service.close("lease").await;
    assert!(!opened_path.exists());
    assert!(destination.exists());

    let mut fixture = resource_fixture(200, true).await;
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/image.png")
        .await
        .unwrap();
    let service = fixture.service.clone();
    let destination_for_task = destination.clone();
    let id = handle.id.clone();
    let save = tokio::spawn(async move {
        service
            .save_resource("lease", &id, &destination_for_task)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), fixture.requests.recv())
        .await
        .unwrap()
        .unwrap();
    fixture.service.release_resource("lease", &handle.id);
    assert!(matches!(
        save.await.unwrap(),
        Err(ChatResourceError::Cancelled)
    ));
    assert_eq!(
        std::fs::read(destination).unwrap(),
        [0, 255, 128, 13, 10, 1]
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn authorization_and_missing_resource_errors_are_sanitized() {
    for (status, expected) in [(403, "permission"), (404, "not_found"), (500, "network")] {
        let fixture = resource_fixture(status, false).await;
        let handle = fixture
            .service
            .register_resource("lease", "s3://bucket/image.png")
            .await
            .unwrap();
        let response = reqwest::get(handle.url).await.unwrap();
        let value: serde_json::Value = response.json().await.unwrap();
        assert_eq!(value, serde_json::json!({"code": expected}));
    }
}

#[tokio::test]
async fn waiting_resources_are_cancelled_without_requesting_more_credentials() {
    let mut fixture = resource_fixture(200, false).await;
    let slots = fixture
        .service
        .resources
        .slots
        .acquire_many(RESOURCE_CONCURRENCY as u32)
        .await
        .unwrap();
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/image.png")
        .await
        .unwrap();
    let entry = fixture.service.resource_entry("lease", &handle.id).unwrap();
    let service = fixture.service.clone();
    let task = tokio::spawn(async move { service.resource_stream(&entry, None).await.map(|_| ()) });
    fixture.service.close("lease").await;
    assert!(matches!(
        task.await.unwrap(),
        Err(ChatResourceError::Cancelled)
    ));
    drop(slots);
    assert!(fixture.requests.try_recv().is_err());
    assert!(fixture.scopes.try_recv().is_err());
}

#[tokio::test]
async fn large_download_streams_to_disk_and_revocation_interrupts_body() {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;
    let fixture = resource_fixture(200, false).await;
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("large.bin");
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/large.bin")
        .await
        .unwrap();
    fixture
        .service
        .save_resource("lease", &handle.id, &destination)
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(&destination).unwrap().len(),
        128 * 1024 * 1024
    );
    let mut file = tokio::fs::File::open(&destination).await.unwrap();
    let mut actual = Sha256::new();
    let mut expected = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file.read(&mut buffer).await.unwrap();
        if read == 0 {
            break;
        }
        actual.update(&buffer[..read]);
    }
    for _ in 0..2048 {
        expected.update([42; 65536]);
    }
    assert_eq!(actual.finalize(), expected.finalize());

    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/slow.bin")
        .await
        .unwrap();
    let mut response = reqwest::get(&handle.url).await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(!response.chunk().await.unwrap().unwrap().is_empty());
    fixture.service.release_resource("lease", &handle.id);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), response.bytes())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(
        fixture.service.resources.slots.available_permits(),
        RESOURCE_CONCURRENCY
    );
}

#[tokio::test]
async fn stream_stops_on_webview_disconnect_and_does_not_hold_a_queue_slot() {
    let fixture = resource_fixture(200, false).await;
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/slow.bin")
        .await
        .unwrap();
    let mut response = reqwest::get(&handle.url).await.unwrap();
    assert!(!response.chunk().await.unwrap().unwrap().is_empty());
    drop(response);
    // Acquiring all permits is an event-driven assertion that the disconnected stream was dropped.
    let permits = tokio::time::timeout(
        Duration::from_secs(2),
        fixture
            .service
            .resources
            .slots
            .acquire_many(RESOURCE_CONCURRENCY as u32),
    )
    .await
    .unwrap()
    .unwrap();
    drop(permits);
}

#[tokio::test]
async fn revocation_interrupts_backpressured_connections_without_consumer_progress() {
    let fixture = resource_fixture(200, false).await;
    let mut responses = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..RESOURCE_CONCURRENCY {
        let handle = fixture
            .service
            .register_resource("lease", "s3://bucket/large.bin")
            .await
            .unwrap();
        responses.push(reqwest::get(&handle.url).await.unwrap());
        handles.push(handle);
    }
    // One scheduling delay lets bounded socket buffers fill. No client reads or disconnects follow.
    tokio::time::sleep(Duration::from_millis(200)).await;
    for handle in handles {
        fixture.service.release_resource("lease", &handle.id);
    }
    let permits = tokio::time::timeout(
        Duration::from_secs(2),
        fixture
            .service
            .resources
            .slots
            .acquire_many(RESOURCE_CONCURRENCY as u32),
    )
    .await;
    assert!(
        permits.is_ok(),
        "revocation must free resource slots even while consumers are paused"
    );
    drop(responses);
}

#[tokio::test]
#[ignore = "explicit browser test: requires Playwright Chromium, Node and Vite on localhost:1420"]
async fn browser_decodes_and_downloads_twelve_real_private_images() {
    let fixture = resource_fixture(200, false).await;
    let mut urls = Vec::new();
    for _ in 0..12 {
        urls.push(
            fixture
                .service
                .register_resource("lease", "s3://bucket/actual.png")
                .await
                .unwrap()
                .url,
        );
    }
    let mut process = tokio::process::Command::new("node");
    process
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
        .arg("e2e/chat-resource-read.mjs")
        .env(
            "CHAT_RESOURCE_TEST_URLS",
            serde_json::to_string(&urls).unwrap(),
        )
        .kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(45), process.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[tokio::test]
async fn streaming_limit_reports_the_same_sanitized_error_to_the_ui() {
    let fixture = resource_fixture(200, false).await;
    let handle = fixture
        .service
        .register_resource("lease", "s3://bucket/image.png")
        .await
        .unwrap();
    let entry = fixture.service.resource_entry("lease", &handle.id).unwrap();
    let mut diagnostics = fixture.service.subscribe_resource_errors();
    let mut stream = fixture.service.resource_stream(&entry, None).await.unwrap();
    // Position the counter at the boundary; the next chunk still comes from the real HTTP body.
    stream.received = MAX_RESOURCE_BYTES;
    assert!(matches!(
        stream.chunk().await,
        Err(ChatResourceError::TooLarge)
    ));
    let diagnostic = diagnostics.recv().await.unwrap();
    assert_eq!(diagnostic.lease_id, "lease");
    assert!(matches!(diagnostic.error, ChatResourceError::TooLarge));
}
