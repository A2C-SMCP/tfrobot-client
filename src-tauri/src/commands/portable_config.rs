use crate::commands::computer::{
    generate_instance_id, normalize_name, status_from_instance, ComputerInstanceStatus,
};
use crate::commands::inputs::InputDefinition;
use crate::services::computer::{ComputerInstance, ComputerProfile};
use crate::services::input_value_store::InputValueStore;
use crate::services::portable_config::{
    parse_package, preview_package, validate_package_for_import, PackageGroup, PackageManifest,
    PackagePreview, PortableComputerPackage, PortableMarketplaceDeclaration, PortableProfile,
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

/// Parses a package and returns an immutable import preview. No write is performed.
#[tauri::command]
pub async fn preview_computer_package_import(
    state: State<'_, AppState>,
    path: String,
) -> Result<PackagePreview, String> {
    preview_computer_package_import_core(&state, &path).await
}

pub async fn preview_computer_package_import_core(
    state: &AppState,
    path: &str,
) -> Result<PackagePreview, String> {
    let package = read_package(path)?;
    validate_package_for_import(&package).map_err(|error| error.to_string())?;
    let existing_names = collect_existing_names(state).await;
    Ok(preview_package(&package, &existing_names))
}

/// Commits an import: creates a new Computer with a fresh id and restores the
/// selected durable configuration transactionally.
#[tauri::command]
pub async fn commit_computer_package_import(
    state: State<'_, AppState>,
    path: String,
    final_name: String,
) -> Result<ComputerInstanceStatus, String> {
    commit_computer_package_import_core(&state, &path, &final_name).await
}

pub async fn commit_computer_package_import_core(
    state: &AppState,
    path: &str,
    final_name: &str,
) -> Result<ComputerInstanceStatus, String> {
    let package = read_package(path)?;
    validate_package_for_import(&package).map_err(|error| error.to_string())?;

    let final_name = normalize_name(final_name)?;
    let existing_names = collect_existing_names(state).await;
    if existing_names
        .iter()
        .any(|name| name.trim().eq_ignore_ascii_case(&final_name))
    {
        return Err(format!(
            "Computer name '{final_name}' conflicts with an existing Computer"
        ));
    }

    let id = generate_instance_id();
    let profile = package
        .profile
        .as_ref()
        .map(|profile| {
            let mut profile = profile.to_computer_profile(id.clone());
            profile.name = final_name.clone();
            profile
        })
        .unwrap_or_else(|| ComputerProfile::new(id.clone(), final_name.clone()));
    let instance = ComputerInstance::from(profile);
    let destination_storage_root = state.config.computer_instance_storage_root(&instance.id);

    let result = async {
        state
            .config
            .add_computer_instance(instance.clone())
            .map_err(|error| error.to_string())?;

        if let Some(sdk_config) = package.sdk_config.as_ref() {
            if let Err(error) = restore_complete_sdk_config(state, &instance.id, sdk_config) {
                return Err(
                    rollback_import(state, &instance.id, &destination_storage_root, error).await,
                );
            }
        }
        if let Some(values) = package.input_values.as_ref() {
            if let Err(error) = write_input_values(state, &instance.id, values) {
                return Err(
                    rollback_import(state, &instance.id, &destination_storage_root, error).await,
                );
            }
        }
        if let Some(skills) = package.skills.as_ref() {
            if let Err(error) = write_skill_home_user_files(state, &instance.id, &skills.user_files)
            {
                return Err(
                    rollback_import(state, &instance.id, &destination_storage_root, error).await,
                );
            }
        }

        let runtime = match state
            .computer_registry
            .upsert_runtime(instance.clone())
            .await
        {
            Ok(runtime) => runtime,
            Err(error) => {
                return Err(
                    rollback_import(state, &instance.id, &destination_storage_root, error).await,
                )
            }
        };

        // Declaration + best-effort materialization via the existing SDK command paths.
        if let Some(skills) = package.skills.as_ref() {
            let pending = rebuild_marketplaces_and_plugins(state, &instance.id, skills).await;
            if !pending.is_empty() {
                log::warn!(
                    "Imported Computer '{}' has plugins pending install: {}",
                    instance.id,
                    pending.join(", ")
                );
            }
        }

        Ok(status_from_instance(&instance, &runtime).await)
    }
    .await;
    result
}

fn read_package(path: &str) -> Result<PortableComputerPackage, String> {
    let path = require_non_empty("path", path)?;
    let bytes = std::fs::read(path).map_err(|error| format!("Failed to read {path}: {error}"))?;
    parse_package(&bytes).map_err(|error| error.to_string())
}

async fn collect_existing_names(state: &AppState) -> std::collections::HashSet<String> {
    state
        .computer_registry
        .list_runtimes()
        .await
        .into_iter()
        .map(|runtime| runtime.instance.name.clone())
        .collect()
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

async fn rollback_import(
    state: &AppState,
    instance_id: &str,
    storage_root: &Path,
    primary_error: String,
) -> String {
    crate::commands::computer::rollback_failed_computer_creation(
        state,
        instance_id,
        storage_root,
        "import portable configuration",
        primary_error,
    )
    .await
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
