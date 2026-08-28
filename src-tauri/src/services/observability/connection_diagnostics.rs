use url::Url;

pub const CONNECTION_LOG_TARGET: &str = "tfrobot_client::connection";

/// Returns the transport endpoint without credentials or request-specific query data.
///
/// Connection diagnostics must remain useful for routing failures while never retaining URL
/// userinfo, query parameters, or fragments, all of which may carry credentials.
pub fn sanitize_connection_endpoint(raw: &str) -> String {
    let Ok(mut endpoint) = Url::parse(raw) else {
        return "[invalid-endpoint]".to_string();
    };
    let _ = endpoint.set_username("");
    let _ = endpoint.set_password(None);
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    endpoint.to_string()
}

pub fn classify_connection_error(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if error.contains("timed out") || error.contains("timeout") {
        "timeout"
    } else if error.contains("superseded") || error.contains("stale") {
        "superseded"
    } else if error.contains("unauthorized")
        || error.contains("no session")
        || error.contains("auth")
        || error.contains("token")
    {
        "authentication"
    } else if error.contains("already connected")
        || error.contains("already being established")
        || error.contains("disconnect before switching")
    {
        "connection_conflict"
    } else if error.contains("join") || error.contains("office") {
        "office_membership"
    } else if error.contains("not running") || error.contains("not found") {
        "runtime_unavailable"
    } else {
        "transport_or_internal"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_endpoint_removes_credentials_query_and_fragment() {
        assert_eq!(
            sanitize_connection_endpoint(
                "https://operator:secret@example.com:8443/socket?token=secret#private"
            ),
            "https://example.com:8443/socket"
        );
    }

    #[test]
    fn invalid_connection_endpoint_does_not_echo_input() {
        assert_eq!(
            sanitize_connection_endpoint("token=do-not-log"),
            "[invalid-endpoint]"
        );
    }

    #[test]
    fn connection_errors_are_classified_without_exposing_the_message() {
        assert_eq!(
            classify_connection_error("Socket.IO namespace connect timed out"),
            "timeout"
        );
        assert_eq!(
            classify_connection_error("Connection operation was superseded"),
            "superseded"
        );
        assert_eq!(
            classify_connection_error("Manager token refresh failed"),
            "authentication"
        );
    }
}
