use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::model::{make_resource, ResourceContents};
use tauri::State;

use crate::AppState;

/// Desktop window resource info (metadata only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopWindow {
    pub uri: String,
    pub title: String,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Window content item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Window detail with content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowDetail {
    pub uri: String,
    pub title: Option<String>,
    pub server: String,
    pub contents: Vec<WindowContent>,
}

/// Get desktop resources (window:// URIs)
///
/// Lists all window resources from connected MCP servers.
#[tauri::command]
pub async fn get_desktop(
    state: State<'_, AppState>,
    instance_id: String,
    uri: Option<String>,
) -> Result<Vec<DesktopWindow>, String> {
    let runtime = state
        .computer_registry
        .runtime(require_instance_id(&instance_id)?)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;

    log::info!("get_desktop called with uri filter: {:?}", uri);

    // Call smcp-computer API to list all windows
    let windows = mgr.list_all_windows(uri.as_deref()).await;

    log::info!("Found {} window resources", windows.len());

    Ok(windows
        .into_iter()
        .map(|(server, resource)| DesktopWindow {
            uri: resource.raw.uri.clone(),
            title: resource.raw.name.clone(),
            server,
            description: resource.raw.description.clone(),
            mime_type: resource.raw.mime_type.clone(),
        })
        .collect())
}

/// Get single window detail with content
#[tauri::command]
pub async fn get_window_detail(
    state: State<'_, AppState>,
    instance_id: String,
    server_name: String,
    uri: String,
) -> Result<WindowDetail, String> {
    let runtime = state
        .computer_registry
        .runtime(require_instance_id(&instance_id)?)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;

    log::info!(
        "get_window_detail called: server={}, uri={}",
        server_name,
        uri
    );

    // Create a Resource object for the request
    let resource = make_resource(uri.clone(), uri.clone(), None, None);

    let result = mgr
        .get_window_detail(&server_name, resource)
        .await
        .map_err(|e| format!("Failed to get window detail: {}", e))?;

    // Convert ResourceContents to WindowContent
    let contents: Vec<WindowContent> = result
        .contents
        .into_iter()
        .map(|rc| match rc {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                ..
            } => WindowContent {
                content_type: "text".to_string(),
                uri,
                mime_type,
                text: Some(text),
                blob: None,
            },
            ResourceContents::BlobResourceContents {
                uri,
                mime_type,
                blob,
                ..
            } => WindowContent {
                content_type: "blob".to_string(),
                uri,
                mime_type,
                text: None,
                blob: Some(blob),
            },
        })
        .collect();

    Ok(WindowDetail {
        uri: uri.clone(),
        title: None, // Title not available in detail response
        server: server_name,
        contents,
    })
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}

// =============================================================================
// TODO: Batch window details API (future implementation)
// 已经实现get_windows_details的api在smcp-computer依赖当中,如需批量获取window信息
// 可调用该api进行获取.
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_window_serialization() {
        let window = DesktopWindow {
            uri: "window://test".to_string(),
            title: "Test Window".to_string(),
            server: "test-server".to_string(),
            description: Some("A test window".to_string()),
            mime_type: None,
        };
        let json = serde_json::to_string(&window).unwrap();
        assert!(json.contains("window://test"));
        assert!(json.contains("Test Window"));
        assert!(json.contains("test-server"));
    }

    #[test]
    fn test_desktop_window_without_optional_fields() {
        let window = DesktopWindow {
            uri: "window://minimal".to_string(),
            title: "Minimal".to_string(),
            server: "server".to_string(),
            description: None,
            mime_type: None,
        };
        let json = serde_json::to_string(&window).unwrap();
        // Optional fields should not be serialized when None
        assert!(!json.contains("description"));
        assert!(!json.contains("mime_type"));
    }

    #[test]
    fn test_window_content_text() {
        let content = WindowContent {
            content_type: "text".to_string(),
            uri: "window://test".to_string(),
            mime_type: Some("text/plain".to_string()),
            text: Some("Hello World".to_string()),
            blob: None,
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains("Hello World"));
        assert!(json.contains("text/plain"));
        assert!(!json.contains("blob"));
    }

    #[test]
    fn test_window_content_blob() {
        let content = WindowContent {
            content_type: "blob".to_string(),
            uri: "window://screenshot".to_string(),
            mime_type: Some("image/png".to_string()),
            text: None,
            blob: Some("base64data".to_string()),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"blob""#));
        assert!(json.contains("base64data"));
        assert!(json.contains("image/png"));
        assert!(!json.contains("text"));
    }

    #[test]
    fn test_window_detail_serialization() {
        let detail = WindowDetail {
            uri: "window://test".to_string(),
            title: Some("Test Window".to_string()),
            server: "test-server".to_string(),
            contents: vec![
                WindowContent {
                    content_type: "text".to_string(),
                    uri: "window://test".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: Some("Sample content".to_string()),
                    blob: None,
                },
                WindowContent {
                    content_type: "blob".to_string(),
                    uri: "window://test/image".to_string(),
                    mime_type: Some("image/png".to_string()),
                    text: None,
                    blob: Some("iVBORw0KGgo".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains("window://test"));
        assert!(json.contains("contents"));
        assert!(json.contains("Sample content"));
        assert!(json.contains("iVBORw0KGgo"));
    }

    #[test]
    fn test_window_detail_with_none_title() {
        let detail = WindowDetail {
            uri: "window://notitle".to_string(),
            title: None,
            server: "test-server".to_string(),
            contents: vec![],
        };
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains("window://notitle"));
        // title should be null or not present when None
        assert!(!json.contains("\"title\"") || json.contains("\"title\":null"));
    }

    #[test]
    fn test_window_detail_empty_contents() {
        let detail = WindowDetail {
            uri: "window://empty".to_string(),
            title: Some("Empty Window".to_string()),
            server: "server".to_string(),
            contents: vec![],
        };
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains(r#""contents":[]"#));
    }

    #[test]
    fn test_desktop_window_deserialization() {
        let json = r#"{
            "uri": "window://chrome",
            "title": "Chrome Browser",
            "server": "desktop-mcp",
            "description": "Active Chrome window"
        }"#;
        let window: DesktopWindow = serde_json::from_str(json).unwrap();
        assert_eq!(window.uri, "window://chrome");
        assert_eq!(window.title, "Chrome Browser");
        assert_eq!(window.server, "desktop-mcp");
        assert_eq!(window.description, Some("Active Chrome window".to_string()));
    }

    #[test]
    fn test_window_detail_deserialization() {
        let json = r#"{
            "uri": "window://app",
            "title": "App Window",
            "server": "mcp-server",
            "contents": [
                {
                    "type": "text",
                    "uri": "window://app",
                    "mime_type": "text/plain",
                    "text": "Window text content"
                }
            ]
        }"#;
        let detail: WindowDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.uri, "window://app");
        assert_eq!(detail.contents.len(), 1);
        assert_eq!(detail.contents[0].content_type, "text");
        assert_eq!(
            detail.contents[0].text,
            Some("Window text content".to_string())
        );
    }

    #[test]
    fn test_window_detail_deserialization_without_title() {
        let json = r#"{
            "uri": "window://notitle",
            "server": "mcp-server",
            "contents": []
        }"#;
        let detail: WindowDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.uri, "window://notitle");
        assert_eq!(detail.title, None);
    }
}
