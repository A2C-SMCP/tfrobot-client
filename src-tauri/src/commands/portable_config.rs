use crate::commands::inputs::InputDefinition;
use crate::services::input_value_store::InputValueStore;
use crate::services::portable_config::{
    parse_package, validate_package_for_import, ComputerPackageInspection, PackageGroup,
    PackageManifest, PortableComputerPackage, PortableMarketplaceDeclaration, PortableProfile,
    PortableSdkConfig, PortableSkills, A2C_SMCP_SDK_VERSION, PORTABLE_PACKAGE_FORMAT_VERSION,
    PORTABLE_SDK_SCHEMA_VERSION,
};
use crate::services::storage::{write_atomically, write_json_atomically};
use crate::AppState;
use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use a2c_smcp::smcp_computer::settings::{AddMarketplaceParams, EnableOptions, InstallOptions};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use tauri::State;

const MAX_USER_SKILL_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Exports a Computer's durable configuration as a portable package.
#[tauri::command]
pub async fn export_computer_package(
    state: State<'_, AppState>,
    instance_id: String,
    path: String,
    groups: Option<Vec<PackageGroup>>,
) -> Result<(), String> {
    export_computer_package_core(&state, &instance_id, &path, groups).await
}

pub async fn export_computer_package_core(
    state: &AppState,
    instance_id: &str,
    path: &str,
    groups: Option<Vec<PackageGroup>>,
) -> Result<(), String> {
    let _operation_guard = state
        .computer_registry
        .shared_operation_lease(instance_id)
        .await;
    let instance_id = require_non_empty("instance_id", instance_id)?;
    let instance = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    let selected = normalize_groups(groups);

    let profile = if selected.contains(&PackageGroup::BasicProfile) {
        Some(PortableProfile::from_computer_profile(
            &crate::services::computer::ComputerProfile::from(&instance),
        ))
    } else {
        None
    };
    let sdk_config = if selected.contains(&PackageGroup::McpAndInputs) {
        Some(export_complete_sdk_config(state, instance_id)?)
    } else {
        None
    };
    let input_values = if selected.contains(&PackageGroup::NonSensitiveInputValues) {
        Some(export_non_sensitive_input_values(state, instance_id)?)
    } else {
        None
    };
    let skills = if selected.contains(&PackageGroup::SkillsAndPlugins) {
        Some(export_skills(state, instance_id).await?)
    } else {
        None
    };

    let manifest = PackageManifest {
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        sdk_version: A2C_SMCP_SDK_VERSION.to_string(),
        schema_version: PORTABLE_SDK_SCHEMA_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        source_computer_name: instance.name.clone(),
    };
    let package = PortableComputerPackage {
        format_version: PORTABLE_PACKAGE_FORMAT_VERSION,
        manifest,
        groups: selected,
        profile,
        sdk_config,
        input_values,
        skills,
    };
    write_json_atomically(Path::new(path), &package).map_err(|error| error.to_string())?;
    log::info!("Computer configuration package exported to: {}", path);
    Ok(())
}

/// Parses and validates a package, returning the metadata used to prefill the
/// create-Computer form. No write is performed and no runtime dependency is probed.
#[tauri::command]
pub async fn inspect_computer_package(
    state: State<'_, AppState>,
    path: String,
) -> Result<ComputerPackageInspection, String> {
    inspect_computer_package_core(&state, &path).await
}

pub async fn inspect_computer_package_core(
    state: &AppState,
    path: &str,
) -> Result<ComputerPackageInspection, String> {
    let package = read_and_validate_package(state, path)?;
    Ok(ComputerPackageInspection::from_package(&package))
}

/// Parses a package and applies every validation gate before any write: package
/// structure/version plus schema-only structural validation of the SDK config.
pub(crate) fn read_and_validate_package(
    state: &AppState,
    path: &str,
) -> Result<PortableComputerPackage, String> {
    let path = require_non_empty("path", path)?;
    let bytes = std::fs::read(path).map_err(|error| format!("Failed to read {path}: {error}"))?;
    let package = parse_package(&bytes).map_err(|error| error.to_string())?;
    validate_package_for_import(&package).map_err(|error| error.to_string())?;
    if let Some(sdk_config) = package.sdk_config.as_ref() {
        validate_portable_sdk_config(state, sdk_config)?;
    }
    Ok(package)
}

/// Applies the durable, non-runtime parts of a package to a freshly created
/// instance (SDK config, non-sensitive input values, Skill Home user files).
pub(crate) async fn apply_portable_package_config(
    state: &AppState,
    instance_id: &str,
    package: &PortableComputerPackage,
) -> Result<(), String> {
    if let Some(sdk_config) = package.sdk_config.as_ref() {
        restore_complete_sdk_config(state, instance_id, sdk_config)?;
    }
    if let Some(values) = package.input_values.as_ref() {
        write_input_values(state, instance_id, values)?;
    }
    if let Some(skills) = package.skills.as_ref() {
        write_skill_home_user_files(state, instance_id, &skills.user_files)?;
    }
    Ok(())
}

/// Rebuilds Marketplace/plugin declarations through the existing SDK command paths
/// and materializes them best-effort. Safe to call once the runtime is published.
pub(crate) async fn rebuild_portable_plugins(
    state: &AppState,
    instance_id: &str,
    package: &PortableComputerPackage,
) {
    if let Some(skills) = package.skills.as_ref() {
        let pending = rebuild_marketplaces_and_plugins(state, instance_id, skills).await;
        if !pending.is_empty() {
            log::warn!(
                "Imported Computer '{}' has plugins pending install: {}",
                instance_id,
                pending.join(", ")
            );
        }
    }
}

/// Schema-only structural validation of the package's SDK configuration. It never
/// resolves inputs/secrets and never probes commands, paths or reachability.
fn validate_portable_sdk_config(state: &AppState, sdk: &PortableSdkConfig) -> Result<(), String> {
    let project_doc = ProjectConfigDoc {
        settings: sdk.settings.clone(),
        settings_local: sdk.settings_local.clone(),
        mcp: sdk.mcp.clone(),
        mcp_local: sdk.mcp_local.clone(),
    };
    let report = state.sdk_config.validate(&project_doc);
    if !report.is_valid() {
        let details = report
            .errors
            .iter()
            .map(|error| {
                format!(
                    "{}:{}: {}",
                    error.source_path.as_deref().unwrap_or("package config"),
                    error.field,
                    error.reason
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "Portable package contains an invalid SDK configuration: {details}"
        ));
    }
    if let Some(mcp) = sdk.user_mcp.as_ref() {
        validate_mcp_map_structure(mcp, "user mcp")?;
    }
    Ok(())
}

/// Structural check for a user-scope mcp map: every server must decode as an MCP
/// server declaration. Inputs are validated by the SDK schema pass above.
fn validate_mcp_map_structure(mcp: &Map<String, Value>, label: &str) -> Result<(), String> {
    let Some(servers) = mcp.get("servers") else {
        return Ok(());
    };
    let servers = servers
        .as_object()
        .ok_or_else(|| format!("Portable package {label} 'servers' must be an object"))?;
    for (name, body) in servers {
        let mut body = body
            .as_object()
            .cloned()
            .ok_or_else(|| format!("Portable package {label} server '{name}' must be an object"))?;
        body.entry("name".to_string())
            .or_insert_with(|| Value::String(name.clone()));
        serde_json::from_value::<a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig>(
            Value::Object(body),
        )
        .map_err(|error| {
            format!(
                "Portable package {label} server '{name}' is not a valid MCP declaration: {error}"
            )
        })?;
    }
    Ok(())
}

fn restore_complete_sdk_config(
    state: &AppState,
    instance_id: &str,
    sdk: &PortableSdkConfig,
) -> Result<(), String> {
    let project_doc = ProjectConfigDoc {
        settings: sdk.settings.clone(),
        settings_local: sdk.settings_local.clone(),
        mcp: sdk.mcp.clone(),
        mcp_local: sdk.mcp_local.clone(),
    };
    state
        .sdk_config
        .save(instance_id, &project_doc)
        .map_err(|error| error.to_string())?;

    let anchor = state.sdk_config.project_anchor(instance_id);
    if let Some(user_settings) = sdk.user_settings.as_ref() {
        write_json_atomically(&anchor.join("a2c").join("settings.json"), user_settings)
            .map_err(|error| error.to_string())?;
    }
    if let Some(user_mcp) = sdk.user_mcp.as_ref() {
        write_json_atomically(&anchor.join("a2c").join("mcp.json"), user_mcp)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_input_values(
    state: &AppState,
    instance_id: &str,
    values: &BTreeMap<String, Value>,
) -> Result<(), String> {
    let store = InputValueStore::for_computer(&state.config, instance_id);
    for (id, value) in values {
        store.set(id, value)?;
    }
    Ok(())
}

fn write_skill_home_user_files(
    state: &AppState,
    instance_id: &str,
    user_files: &BTreeMap<String, String>,
) -> Result<(), String> {
    if user_files.is_empty() {
        return Ok(());
    }
    let skill_home = state.config.default_local_skills_root(instance_id);
    let user_root = skill_home.join("user");
    for (relative, content) in user_files {
        let destination = user_root.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
        }
        write_atomically(&destination, content.as_bytes()).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Rebuilds marketplace/plugin declarations through the existing SDK command paths and
/// materializes them best-effort. Returns plugin ids that could not be materialized.
async fn rebuild_marketplaces_and_plugins(
    state: &AppState,
    instance_id: &str,
    skills: &PortableSkills,
) -> Vec<String> {
    let Some(runtime) = state.computer_registry.runtime(instance_id).await else {
        return skills.installed_plugins.clone();
    };
    let env = state.sdk_config.env(instance_id);

    for declaration in &skills.marketplaces {
        let Some(source) = marketplace_source_url(&declaration.source) else {
            continue;
        };
        let _ = runtime
            .sdk_add_marketplace(
                &source,
                AddMarketplaceParams {
                    name: Some(declaration.name.as_str()),
                    auto_update: false,
                    no_clone: false,
                },
            )
            .await;
    }

    let mut pending = Vec::new();
    for plugin_id in &skills.installed_plugins {
        let install = runtime
            .sdk_install_plugin(
                plugin_id,
                InstallOptions {
                    scope: Some("user"),
                    env: Some(&env),
                    ..Default::default()
                },
                None,
            )
            .await;
        if let Err(error) = install {
            log::warn!("Failed to install imported plugin '{plugin_id}': {error}");
            pending.push(plugin_id.clone());
            continue;
        }
        let _ = runtime
            .sdk_enable_plugin(
                plugin_id,
                EnableOptions {
                    scope: Some("user"),
                    env: Some(&env),
                    ..Default::default()
                },
                None,
            )
            .await;
    }
    runtime.mark_sdk_skills_dirty().await;
    pending
}

fn marketplace_source_url(source: &Value) -> Option<String> {
    let object = source.as_object()?;
    object
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            object
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn normalize_groups(groups: Option<Vec<PackageGroup>>) -> Vec<PackageGroup> {
    match groups {
        Some(groups) if !groups.is_empty() => {
            let mut seen = std::collections::HashSet::new();
            groups
                .into_iter()
                .filter(|group| seen.insert(*group))
                .collect()
        }
        _ => PackageGroup::ALL.to_vec(),
    }
}

fn export_complete_sdk_config(
    state: &AppState,
    instance_id: &str,
) -> Result<PortableSdkConfig, String> {
    let project_doc = state
        .sdk_config
        .load_raw_project_config(instance_id)
        .map_err(|error| error.to_string())?;
    let anchor = state.sdk_config.project_anchor(instance_id);
    let user_settings = read_optional_json_object(&anchor.join("a2c").join("settings.json"))?;
    let user_mcp = read_optional_json_object(&anchor.join("a2c").join("mcp.json"))?;
    Ok(PortableSdkConfig {
        settings: project_doc.settings,
        settings_local: project_doc.settings_local,
        mcp: sanitize_mcp_map(project_doc.mcp),
        mcp_local: sanitize_mcp_map(project_doc.mcp_local),
        user_settings,
        user_mcp: sanitize_mcp_map(user_mcp),
    })
}

fn read_optional_json_object(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("Failed to read {}: {error}", path.display()));
        }
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
    match value {
        Value::Object(map) => Ok(Some(map)),
        _ => Err(format!("Expected a JSON object at {}", path.display())),
    }
}

/// Strips machine-local env files and password-type Input defaults while preserving
/// `env`/`headers` literals (CLI-native semantics, no redaction).
fn sanitize_mcp_map(mcp: Option<Map<String, Value>>) -> Option<Map<String, Value>> {
    let mut out = mcp?;
    if let Some(Value::Object(servers)) = out.get_mut("servers") {
        for server in servers.values_mut() {
            if let Value::Object(server) = server {
                server.remove("envFile");
                server.remove("env_file");
            }
        }
    }
    if let Some(Value::Array(inputs)) = out.get_mut("inputs") {
        for input in inputs.iter_mut() {
            if let Value::Object(input) = input {
                if input.get("password").and_then(Value::as_bool) == Some(true) {
                    input.remove("default");
                }
            }
        }
    }
    Some(out)
}

fn export_non_sensitive_input_values(
    state: &AppState,
    instance_id: &str,
) -> Result<BTreeMap<String, Value>, String> {
    let definitions = state.sdk_config.load_input_definitions(instance_id);
    let secret_ids = definitions
        .iter()
        .filter(|definition| definition.is_secret() || !definition.supports_persistent_value())
        .map(InputDefinition::id)
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let store = InputValueStore::for_computer(&state.config, instance_id);
    let values = store.list()?;
    Ok(values
        .into_iter()
        .filter(|(id, _)| !secret_ids.contains(id))
        .collect())
}

async fn export_skills(state: &AppState, instance_id: &str) -> Result<PortableSkills, String> {
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let skill_home = runtime.sdk_skill_home().await;

    let user_files = read_user_skill_files(&skill_home.join("user"))?;
    let marketplaces = read_marketplace_declarations(&skill_home)?;
    let installed_plugins = read_installed_plugin_intent(&skill_home)?;

    Ok(PortableSkills {
        user_files,
        marketplaces,
        installed_plugins,
    })
}

fn read_marketplace_declarations(
    skill_home: &Path,
) -> Result<Vec<PortableMarketplaceDeclaration>, String> {
    let file = a2c_smcp::smcp_computer::settings::load_known_marketplaces(Some(skill_home), None);
    let mut declarations = file
        .account
        .marketplaces
        .into_iter()
        .map(|(name, entry)| {
            let commit_sha = entry
                .extra
                .get("commitSha")
                .and_then(Value::as_str)
                .map(str::to_string);
            PortableMarketplaceDeclaration {
                name,
                source: entry.source,
                commit_sha,
            }
        })
        .collect::<Vec<_>>();
    declarations.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(declarations)
}

fn read_installed_plugin_intent(skill_home: &Path) -> Result<Vec<String>, String> {
    let file = a2c_smcp::smcp_computer::settings::store::load_installed_plugins_intent(
        Some(skill_home),
        None,
    );
    let mut plugins = file
        .account
        .installed_plugins
        .into_iter()
        .collect::<Vec<_>>();
    plugins.sort();
    Ok(plugins)
}

fn read_user_skill_files(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    if !root.exists() {
        return Ok(files);
    }
    collect_text_files(root, root, &mut files)?;
    Ok(files)
}

fn collect_text_files(
    base: &Path,
    directory: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_text_files(base, &path, out)?;
            continue;
        }
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if metadata.len() > MAX_USER_SKILL_FILE_BYTES {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return Err(format!("Failed to read {}: {error}", path.display())),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if text.contains('\0') {
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        out.insert(relative, text);
    }
    Ok(())
}

fn require_non_empty<'a>(label: &str, value: &'a str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::sanitize_mcp_map;
    use serde_json::json;

    #[test]
    fn sanitize_strips_machine_local_env_files_and_preserves_env_headers_literals() {
        let mcp = json!({
            "servers": {
                "echo": {
                    "type": "Stdio",
                    "env": { "TOKEN": "literal-value", "SECRET": "keep-me" },
                    "headers": { "Authorization": "Bearer literal" },
                    "envFile": "/tmp/.env",
                    "env_file": "/tmp/alt.env"
                }
            },
            "inputs": []
        });
        let map = mcp.as_object().cloned().unwrap();
        let sanitized = sanitize_mcp_map(Some(map)).unwrap();
        let server = sanitized
            .get("servers")
            .unwrap()
            .get("echo")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(!server.contains_key("envFile"));
        assert!(!server.contains_key("env_file"));
        assert_eq!(
            server.get("env").unwrap().get("TOKEN").unwrap(),
            "literal-value"
        );
        assert_eq!(server.get("env").unwrap().get("SECRET").unwrap(), "keep-me");
        assert_eq!(
            server.get("headers").unwrap().get("Authorization").unwrap(),
            "Bearer literal"
        );
    }

    #[test]
    fn sanitize_removes_password_defaults_but_keeps_regular_defaults() {
        let mcp = json!({
            "servers": {},
            "inputs": [
                { "id": "token", "type": "PromptString", "password": true, "default": "do-not-export" },
                { "id": "region", "type": "PromptString", "password": false, "default": "cn" }
            ]
        });
        let map = mcp.as_object().cloned().unwrap();
        let sanitized = sanitize_mcp_map(Some(map)).unwrap();
        let inputs = sanitized.get("inputs").unwrap().as_array().unwrap();
        let token = inputs[0].as_object().unwrap();
        let region = inputs[1].as_object().unwrap();
        assert!(!token.contains_key("default"));
        assert_eq!(region.get("default").unwrap(), "cn");
    }
}
