//! Backward-compatible serde helpers for persisted and remote identifiers.

use serde::{de::Error as _, Deserialize, Deserializer};

/// Deserialize a required opaque identifier from its canonical string form or a legacy number.
pub fn deserialize_opaque_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Number(value) if value.as_u64().is_some() => Ok(value.to_string()),
        value => Err(D::Error::custom(format!(
            "expected opaque identifier as string or number, got {value}"
        ))),
    }
}

/// Deserialize an optional opaque identifier from either its canonical string form or a legacy
/// JSON number. New serialization always emits a string because account identifiers are not
/// numeric values, even when an older Manager happened to encode one as a number.
pub fn deserialize_optional_opaque_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        Some(serde_json::Value::Number(value)) if value.as_u64().is_some() => {
            Ok(Some(value.to_string()))
        }
        Some(value) => Err(D::Error::custom(format!(
            "expected opaque identifier as string or number, got {value}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        #[serde(default, deserialize_with = "super::deserialize_optional_opaque_id")]
        id: Option<String>,
    }

    #[derive(Deserialize)]
    struct RequiredFixture {
        #[serde(deserialize_with = "super::deserialize_opaque_id")]
        id: String,
    }

    #[test]
    fn accepts_string_number_null_and_missing() {
        let string: Fixture = serde_json::from_str(r#"{"id":"org-1:account-2"}"#).unwrap();
        assert_eq!(string.id.as_deref(), Some("org-1:account-2"));

        let number: Fixture = serde_json::from_str(r#"{"id":42}"#).unwrap();
        assert_eq!(number.id.as_deref(), Some("42"));

        let null: Fixture = serde_json::from_str(r#"{"id":null}"#).unwrap();
        assert!(null.id.is_none());

        let missing: Fixture = serde_json::from_str("{}").unwrap();
        assert!(missing.id.is_none());
    }

    #[test]
    fn required_id_accepts_string_and_legacy_number() {
        let string: RequiredFixture =
            serde_json::from_str(r#"{"id":"org-1:department-2"}"#).unwrap();
        assert_eq!(string.id, "org-1:department-2");

        let number: RequiredFixture = serde_json::from_str(r#"{"id":42}"#).unwrap();
        assert_eq!(number.id, "42");
    }

    #[test]
    fn rejects_values_outside_the_legacy_u64_domain() {
        for json in [
            r#"{"id":-1}"#,
            r#"{"id":1.5}"#,
            r#"{"id":true}"#,
            r#"{"id":{"nested":42}}"#,
        ] {
            assert!(serde_json::from_str::<Fixture>(json).is_err(), "{json}");
            assert!(
                serde_json::from_str::<RequiredFixture>(json).is_err(),
                "{json}"
            );
        }
    }
}
