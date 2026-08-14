use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const SDK_INPUT_FIELDS: [&str; 2] = ["server_parameters", "envFile"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputReferenceLocation {
    pub input_id: String,
    pub server_name: String,
    pub layer: String,
    pub field_path: String,
}

pub fn referenced_input_ids(value: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    visit_value(value, &mut Vec::new(), &mut |text, _| {
        for (id, _, _) in input_references(text) {
            ids.insert(id.to_string());
        }
    });
    ids
}

pub fn find_project_input_references(document: &ProjectConfigDoc) -> Vec<InputReferenceLocation> {
    let mut locations = Vec::new();
    for (layer_name, layer) in [
        ("project", document.mcp.as_ref()),
        ("local", document.mcp_local.as_ref()),
    ] {
        let Some(servers) = layer
            .and_then(|value| value.get("servers"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (server_name, server) in servers {
            for field_name in SDK_INPUT_FIELDS {
                let Some(field) = server.get(field_name) else {
                    continue;
                };
                visit_value(
                    field,
                    &mut vec![field_name.to_string()],
                    &mut |text, path| {
                        for (input_id, _, _) in input_references(text) {
                            locations.push(InputReferenceLocation {
                                input_id: input_id.to_string(),
                                server_name: server_name.clone(),
                                layer: layer_name.to_string(),
                                field_path: path.join("."),
                            });
                        }
                    },
                );
            }
        }
    }
    locations.sort_by(|left, right| {
        (
            &left.layer,
            &left.server_name,
            &left.field_path,
            &left.input_id,
        )
            .cmp(&(
                &right.layer,
                &right.server_name,
                &right.field_path,
                &right.input_id,
            ))
    });
    locations.dedup();
    locations
}

pub fn replace_project_input_reference(
    document: &mut ProjectConfigDoc,
    input_id: &str,
    literal: &str,
) -> usize {
    replace_project_input_references(
        document,
        &BTreeMap::from([(input_id.to_string(), literal.to_string())]),
    )
}

/// Replaces references in one pass over the original SDK-consumed fields. A replacement literal
/// that resembles another Input reference is deliberately not interpreted again.
pub fn replace_project_input_references(
    document: &mut ProjectConfigDoc,
    literals: &BTreeMap<String, String>,
) -> usize {
    let mut replacements = 0;
    for layer in [&mut document.mcp, &mut document.mcp_local]
        .into_iter()
        .flatten()
    {
        let Some(Value::Object(servers)) = layer.get_mut("servers") else {
            continue;
        };
        for server in servers.values_mut() {
            for field_name in SDK_INPUT_FIELDS {
                if let Some(field) = server.get_mut(field_name) {
                    replace_in_value(field, literals, &mut replacements);
                }
            }
        }
    }
    replacements
}

fn visit_value(value: &Value, path: &mut Vec<String>, visitor: &mut impl FnMut(&str, &[String])) {
    match value {
        Value::String(text) => visitor(text, path),
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                path.push(format!("[{index}]"));
                visit_value(value, path, visitor);
                path.pop();
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                path.push(key.clone());
                visit_value(value, path, visitor);
                path.pop();
            }
        }
        _ => {}
    }
}

fn replace_in_value(
    value: &mut Value,
    literals: &BTreeMap<String, String>,
    replacements: &mut usize,
) {
    match value {
        Value::String(text) => {
            let references = input_references(text);
            if references.is_empty() {
                return;
            }
            let mut next = String::with_capacity(text.len());
            let mut cursor = 0;
            for (input_id, start, end) in references {
                let Some(literal) = literals.get(input_id) else {
                    continue;
                };
                next.push_str(&text[cursor..start]);
                next.push_str(literal);
                cursor = end;
                *replacements += 1;
            }
            next.push_str(&text[cursor..]);
            *text = next;
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| replace_in_value(value, literals, replacements)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| replace_in_value(value, literals, replacements)),
        _ => {}
    }
}

fn input_references(value: &str) -> Vec<(&str, usize, usize)> {
    let mut references = Vec::new();
    let mut offset = 0;
    while let Some(relative_start) = value[offset..].find("${input:") {
        let start = offset + relative_start;
        let id_start = start + "${input:".len();
        let Some(relative_end) = value[id_start..].find('}') else {
            break;
        };
        let end = id_start + relative_end;
        if end > id_start {
            references.push((&value[id_start..end], start, end + 1));
        }
        offset = end + 1;
    }
    references
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scans_and_rewrites_both_layers_with_field_paths() {
        let mut document = ProjectConfigDoc {
            mcp: Some(
                json!({
                    "servers": {"alpha": {
                        "server_parameters": {"env": {"TOKEN": "${input:key}"}},
                        "vrl": "${input:ignored}",
                        "tool_meta": {"description": "${input:also_ignored}"}
                    }}
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            mcp_local: Some(
                json!({
                    "servers": {"beta": {"server_parameters": {"args": ["--env", "${input:env}"]}}}
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..ProjectConfigDoc::default()
        };

        let locations = find_project_input_references(&document);
        assert_eq!(locations.len(), 2);
        assert!(!locations
            .iter()
            .any(|location| location.input_id == "ignored"));
        assert!(!locations
            .iter()
            .any(|location| location.input_id == "also_ignored"));
        assert!(locations.iter().any(|location| {
            location.input_id == "env"
                && location.layer == "local"
                && location.field_path == "server_parameters.args.[1]"
        }));
        assert_eq!(
            replace_project_input_reference(&mut document, "env", "prod"),
            1
        );
        assert!(!find_project_input_references(&document)
            .iter()
            .any(|location| location.input_id == "env"));
        assert!(serde_json::to_string(&(&document.mcp, &document.mcp_local))
            .unwrap()
            .contains("${input:ignored}"));
        assert!(serde_json::to_string(&(&document.mcp, &document.mcp_local))
            .unwrap()
            .contains("${input:also_ignored}"));
    }

    #[test]
    fn replacement_literals_are_not_reinterpreted_in_the_same_pass() {
        let mut document = ProjectConfigDoc {
            mcp: Some(
                json!({"servers":{"alpha":{"server_parameters":{"env":{
                    "A":"${input:a}", "B":"${input:b}"
                }}}}})
                .as_object()
                .unwrap()
                .clone(),
            ),
            ..ProjectConfigDoc::default()
        };
        let replacements = replace_project_input_references(
            &mut document,
            &BTreeMap::from([
                ("a".to_string(), "${input:b}".to_string()),
                ("b".to_string(), "literal-b".to_string()),
            ]),
        );

        assert_eq!(replacements, 2);
        let serialized = serde_json::to_string(&(&document.mcp, &document.mcp_local)).unwrap();
        assert!(serialized.contains("${input:b}"));
        assert!(serialized.contains("literal-b"));
    }
}
