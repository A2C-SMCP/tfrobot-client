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
        other => other,
    }
}

pub fn redact_text(value: &str) -> String {
    value
        .lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| *character != '_' && *character != '-' && *character != ' ')
        .collect::<String>()
        .to_ascii_lowercase();
    SENSITIVE_KEYS
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
}
