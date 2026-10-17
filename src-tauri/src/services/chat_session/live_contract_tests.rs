//! Opt-in, real test-environment contract verification. Credentials come from a temporary file.
use super::*;

#[tokio::test]
#[ignore = "requires explicit test credentials; creates and cleans only its own conversation/file"]
async fn live_test_server_chat_and_upload() {
    let input: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::env::var("TFRC_CHAT_ACCEPTANCE_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(input["environment"], "test");
    let info: ConnectionInfoResponse = serde_json::from_value(input["connection"].clone()).unwrap();
    let endpoints = resolve_chat_endpoints(&info).unwrap();
    assert!(endpoints
        .http_base_url
        .host_str()
        .unwrap()
        .ends_with(".test.turingfocus.cn"));
    let context = ManagerContextKey {
        environment: crate::services::manager_environment::ManagerEnvironment::Staging,
        account_id: "isolated-contract-test".into(),
        organization_id: "isolated-contract-test".into(),
    };
    let mut target = tests::test_target(endpoints.http_base_url.clone(), context);
    target.resource_base_url = endpoints.resource_base_url;
    target.routing_headers = info.routing_headers;
    let service = ChatSessionService::new(Weak::new());
    let token = ChatSessionCredential {
        token: input["chatToken"].as_str().unwrap().into(),
        expires_at: 0,
    };
    let (_cancel, cancelled) = watch::channel(false);
    let request = |path: &str, method: &'static str, body: Option<String>| {
        service.proxy_authorized(
            target.clone(),
            cancelled.clone(),
            target.http_base_url.join(path).unwrap(),
            method,
            body,
            token.clone(),
        )
    };
    let platforms = request("v1/chat/platforms", "GET", None).await.unwrap();
    assert_eq!(platforms.status, 200);
    let platforms: serde_json::Value = serde_json::from_str(&platforms.body).unwrap();
    let platform_id = platforms["data"][0]["platformId"].as_u64().unwrap();
    let title = format!("codex-native-0101-{}", Uuid::new_v4());
    let created = request(
        "v1/chat/conversations",
        "POST",
        Some(serde_json::json!({"title":title,"platformId":platform_id}).to_string()),
    )
    .await
    .unwrap();
    assert_eq!(created.status, 200);
    let created: serde_json::Value = serde_json::from_str(&created.body).unwrap();
    let id = created["data"]["conversationId"].as_u64().unwrap();
    let history = request(
        &format!("v1/chat/conversations/{id}/messages?count=20"),
        "GET",
        None,
    )
    .await;
    // Cleanup remains outside the production IPC allowlist (users cannot invoke arbitrary DELETE).
    let cleanup = service
        .authorized_request(
            &target,
            target
                .http_base_url
                .join(&format!("v1/chat/conversations/{id}"))
                .unwrap(),
            reqwest::Method::DELETE,
            &token,
        )
        .unwrap()
        .send()
        .await
        .unwrap();
    assert!(cleanup.status().is_success());
    assert_eq!(history.unwrap().status, 200);

    let upload_token = ChatSessionCredential {
        token: input["uploadToken"].as_str().unwrap().into(),
        expires_at: 0,
    };
    let upload = service
        .authorized_request(
            &target,
            target
                .http_base_url
                .join("v1/dashboard/remote/source/cos/upload")
                .unwrap(),
            reqwest::Method::POST,
            &upload_token,
        )
        .unwrap()
        .multipart(
            reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::text("Isolated Client 0.10.1 upload verification")
                    .file_name(format!("codex-client-0101-{}.txt", Uuid::new_v4()))
                    .mime_str("text/plain")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    assert!(upload.status().is_success());
    let uploaded: serde_json::Value = upload.json().await.unwrap();
    let uri = uploaded["data"]["uri"].as_str().unwrap();
    assert!(uri.starts_with("s3://"));
    let mut cleanup_url = target
        .http_base_url
        .join("v1/dashboard/remote/source/cos/delete")
        .unwrap();
    cleanup_url.query_pairs_mut().append_pair("uri", uri);
    let cleanup = service
        .authorized_request(&target, cleanup_url, reqwest::Method::DELETE, &upload_token)
        .unwrap()
        .send()
        .await
        .unwrap();
    assert!(cleanup.status().is_success());
}
