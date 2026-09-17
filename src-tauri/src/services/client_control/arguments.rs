//! Diagnostics cross a persistence boundary: never format an unexpected input value.
use super::{ClientControlError, ClientControlErrorCode, ToolCatalog, ToolId};
use serde::de::{self, DeserializeOwned};
use serde_json::Value;
use std::fmt;

#[derive(Debug)]
struct SafeDecodeError(String);

impl fmt::Display for SafeDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SafeDecodeError {}

impl de::Error for SafeDecodeError {
    fn custom<T: fmt::Display>(_message: T) -> Self {
        Self("value failed validation (details omitted)".into())
    }

    fn invalid_type(_actual: de::Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        Self(format!("invalid type; expected {expected}"))
    }

    fn invalid_value(_actual: de::Unexpected<'_>, expected: &dyn de::Expected) -> Self {
        Self(format!("invalid value; expected {expected}"))
    }

    fn invalid_length(_length: usize, expected: &dyn de::Expected) -> Self {
        Self(format!("invalid length; expected {expected}"))
    }

    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        Self(format!(
            "unknown variant; expected one of {}",
            expected.join(", ")
        ))
    }

    fn unknown_field(_field: &str, expected: &'static [&'static str]) -> Self {
        Self(format!(
            "unknown field; expected one of {}",
            expected.join(", ")
        ))
    }

    fn missing_field(field: &'static str) -> Self {
        Self(format!("missing field `{field}`"))
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self(format!("duplicate field `{field}`"))
    }
}

pub(super) fn decode<T: DeserializeOwned>(
    value: Value,
    tool: ToolId,
) -> Result<T, ClientControlError> {
    // Keep serde_json as the source of truth for acceptance and returned values. Only replay a
    // rejected input with a structured error type: serde_json's Display embeds secret values.
    serde_path_to_error::deserialize(value.clone()).map_err(|error| {
        let schema = ToolCatalog.get(tool).input_schema();
        // A path can contain user-supplied map keys. Expose only a catalog-defined argument name.
        let argument = error
            .path()
            .iter()
            .next()
            .and_then(|segment| match segment {
                serde_path_to_error::Segment::Map { key }
                    if schema["properties"].get(key).is_some() =>
                {
                    Some(key.as_str())
                }
                _ => None,
            });
        let reason = serde_value::to_value(&value)
            .ok()
            .and_then(|value| {
                T::deserialize(serde_value::ValueDeserializer::<SafeDecodeError>::new(
                    value,
                ))
                .err()
            })
            .map(|error| error.to_string())
            .unwrap_or_else(|| "value failed validation (details omitted)".into());
        ClientControlError::new(
            ClientControlErrorCode::InvalidArguments,
            format!(
                "invalid tool arguments at {}: {reason}",
                argument.unwrap_or("arguments")
            ),
        )
    })
}

pub(super) fn audit_fields(tool: ToolId, parameters: &Value, error: &ClientControlError) -> Value {
    // Invalid arguments are untrusted even in otherwise harmless metadata fields. Record only
    // recognized field presence; the typed diagnostic carries the validation reason.
    let schema = ToolCatalog.get(tool).input_schema();
    let fields: Vec<_> = parameters
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| schema["properties"].get(key.as_str()).is_some())
        .collect();
    serde_json::json!({
        "argument_fields": fields,
        // Structured metadata preserves schema field names (e.g. api_key_action) even if the
        // generic free-text redactor suppresses that name in the legacy error string.
        "validation": {"category": error.code, "message": error.message}
    })
}
