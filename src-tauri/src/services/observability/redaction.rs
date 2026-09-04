use serde_json::Value;

const REDACTED: &str = "[REDACTED]";
const SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "cookie",
];

pub fn redact_json(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let redacted = if is_sensitive_key(&key) {
                        Value::String(REDACTED.to_string())
                    } else {
                        redact_json(value)
                    };
                    (key, redacted)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_json).collect()),
        Value::String(value) => Value::String(redact_urls_in_text(&value)),
        other => other,
    }
}

fn redact_string_value(value: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(value) else {
        return value.to_string();
    };
    let has_sensitive_components = !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some();
    if !has_sensitive_components {
        return value.to_string();
    }
    if !parsed.username().is_empty() {
        let _ = parsed.set_username("");
    }
    if parsed.password().is_some() {
        let _ = parsed.set_password(None);
    }
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

pub fn redact_text(value: &str) -> String {
    redact_urls_in_text(value)
        .lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_urls_in_text(value: &str) -> String {
    let mut sanitized = value.to_string();
    let mut search_from = 0;
    while let Some(scheme_offset) = sanitized[search_from..].find("://") {
        let scheme_marker = search_from + scheme_offset;
        let url_start = sanitized[..scheme_marker]
            .char_indices()
            .rev()
            .take_while(|(_, character)| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            })
            .last()
            .map(|(index, _)| index)
            .unwrap_or(scheme_marker);
        let candidate_end = sanitized[scheme_marker + 3..]
            .char_indices()
            .find(|(_, character)| character.is_whitespace())
            .map(|(offset, _)| scheme_marker + 3 + offset)
            .unwrap_or(sanitized.len());
        let mut url_end = candidate_end;
        while url_end > url_start
            && sanitized[..url_end]
                .chars()
                .next_back()
                .is_some_and(|character| matches!(character, ',' | ';' | ')' | ']' | '}'))
        {
            url_end = sanitized[..url_end]
                .char_indices()
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(url_start);
        }
        let candidate = &sanitized[url_start..url_end];
        let replacement = redact_string_value(candidate);
        if replacement == candidate {
            search_from = url_end.max(scheme_marker + 3);
            continue;
        }
        sanitized.replace_range(url_start..url_end, &replacement);
        search_from = url_start + replacement.len();
    }
    sanitized
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| *character != '_' && *character != '-' && *character != ' ')
        .collect::<String>()
        .to_ascii_lowercase();
    normalized == "code"
        || normalized == "oauthcode"
        || normalized == "authorizationcode"
        || SENSITIVE_KEYS
            .iter()
            .map(|candidate| candidate.replace('_', ""))
            .chain(["credential".to_string()])
            .any(|candidate| normalized.contains(&candidate))
}

fn redact_sensitive_line(line: &str) -> String {
    if !is_sensitive_key(line) {
        return line.to_string();
    }
    let lower = line.to_ascii_lowercase();
    let position = [
        "access_token",
        "access-token",
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "credential",
        "password",
        "passwd",
        "secret",
        "cookie",
        "token",
        "oauth_code",
        "oauth-code",
        "authorization_code",
        "authorization-code",
        "?code",
        "&code",
    ]
    .iter()
    .filter_map(|pattern| lower.find(pattern))
    .min();
    let Some(position) = position else {
        return REDACTED.to_string();
    };
    let delimiter = line[position..]
        .char_indices()
        .find_map(|(offset, character)| {
            (character == ':' || character == '=').then_some(position + offset)
        });
    delimiter
        .map(|index| format!("{} {REDACTED}", &line[..=index]))
        .unwrap_or_else(|| REDACTED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_nested_secret_values() {
        assert_eq!(
            redact_json(json!({"token": "secret", "nested": {"api-key": "key", "safe": 1}})),
            json!({"token": REDACTED, "nested": {"api-key": REDACTED, "safe": 1}})
        );
    }

    #[test]
    fn strips_credentials_query_and_fragment_from_url_values() {
        assert_eq!(
            redact_json(json!({
                "uri": "https://user:password@example.com/window?safe=1&access_token=secret&code=oauth#fragment"
            })),
            json!({"uri": "https://example.com/window"})
        );
    }

    #[test]
    fn redacts_oauth_code_keys_without_hiding_error_codes() {
        assert_eq!(
            redact_json(json!({"code": "oauth-secret", "error_code": "timeout"})),
            json!({"code": REDACTED, "error_code": "timeout"})
        );
    }

    #[test]
    fn strips_credentials_and_query_from_urls_embedded_in_text() {
        assert_eq!(
            redact_text(
                "request failed: https://alice:s3cr3t@example.com/mcp?code=oauth&token=secret"
            ),
            "request failed: https://example.com/mcp"
        );
    }

    #[test]
    fn sanitizes_urls_embedded_in_json_string_leaves() {
        assert_eq!(
            redact_json(json!({
                "error": "request failed: https://alice:password@example.com/mcp?code=oauth#fragment",
                "args": ["--endpoint=https://alice:password@example.com/mcp?token=secret"]
            })),
            json!({
                "error": "request failed: https://example.com/mcp",
                "args": ["--endpoint=https://example.com/mcp"]
            })
        );
    }
}
