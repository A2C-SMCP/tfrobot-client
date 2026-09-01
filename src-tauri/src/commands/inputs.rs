use crate::services::input_entry_store::{InputEntryStorageKind, InputEntryStore, InputEntryView};
use crate::services::input_references::{find_project_input_references, InputReferenceLocation};
use crate::services::input_value_index::{self, InputValueStorageKind};
use crate::services::sdk_config::{ensure_portable_cli_arguments, SdkConfigService};
use crate::AppState;
use a2c_smcp::smcp_computer::inputs::{run_command, InputKind};
use a2c_smcp::smcp_computer::mcp_clients::model::{
    CommandInput, MCPServerInput, PickStringInput, PickStringOption, PromptStringInput,
};
use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::State;

/// Input variable definition for the frontend
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum InputDefinition {
    PromptString {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<bool>,
    },
    PickString {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        options: Vec<PickOption>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Command {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PickOption {
    pub label: String,
    pub value: String,
}

const COMMAND_PREVIEW_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandPreviewResult {
    pub stdout: String,
    pub truncated: bool,
}

impl InputDefinition {
    pub fn id(&self) -> &str {
        match self {
            InputDefinition::PromptString { id, .. } => id,
            InputDefinition::PickString { id, .. } => id,
            InputDefinition::Command { id, .. } => id,
        }
    }

    pub fn is_secret(&self) -> bool {
        matches!(
            self,
            InputDefinition::PromptString {
                password: Some(true),
                ..
            }
        )
    }

    fn storage_kind(&self) -> Option<InputValueStorageKind> {
        if !self.supports_persistent_value() {
            None
        } else if self.is_secret() {
            Some(InputValueStorageKind::Secret)
        } else {
            Some(InputValueStorageKind::Value)
        }
    }

    pub fn is_prompt_string(&self) -> bool {
        matches!(self, InputDefinition::PromptString { .. })
    }

    pub fn supports_persistent_value(&self) -> bool {
        !matches!(self, InputDefinition::Command { .. })
    }
}

pub(crate) fn input_definition_to_sdk(input: &InputDefinition) -> MCPServerInput {
    match input {
        InputDefinition::PromptString {
            id,
            label,
            description,
            default,
            password,
        } => MCPServerInput::PromptString(PromptStringInput {
            id: id.clone(),
            description: effective_description(id, label, description),
            default: default.clone(),
            password: *password,
        }),
        InputDefinition::PickString {
            id,
            label,
            description,
            options,
            default,
        } => MCPServerInput::PickString(PickStringInput {
            id: id.clone(),
            description: effective_description(id, label, description),
            options: options
                .iter()
                .map(|option| PickStringOption {
                    label: option.label.clone(),
                    value: option.value.clone(),
                })
                .collect(),
            default: default.clone(),
        }),
        InputDefinition::Command {
            id,
            label,
            command,
            args,
        } => MCPServerInput::Command(CommandInput {
            id: id.clone(),
            description: effective_description(id, label, &None),
            command: command.clone(),
            args: args.as_ref().map(|args| {
                args.iter()
                    .enumerate()
                    .map(|(index, value)| (format!("{index:06}"), value.clone()))
                    .collect()
            }),
        }),
    }
}

pub(crate) fn input_definition_from_sdk(input: &MCPServerInput) -> InputDefinition {
    match input {
        MCPServerInput::PromptString(input) => InputDefinition::PromptString {
            id: input.id.clone(),
            label: Some(input.description.clone()),
            description: None,
            default: input.default.clone(),
            password: input.password,
        },
        MCPServerInput::PickString(input) => InputDefinition::PickString {
            id: input.id.clone(),
            label: Some(input.description.clone()),
            description: None,
            options: input
                .options
                .iter()
                .map(|option| PickOption {
                    label: option.label.clone(),
                    value: option.value.clone(),
                })
                .collect(),
            default: input.default.clone(),
        },
        MCPServerInput::Command(input) => {
            let args = input.args.as_ref().map(|args| {
                let mut args = args.iter().collect::<Vec<_>>();
                args.sort_by_key(|(key, _)| *key);
                args.into_iter().map(|(_, value)| value.clone()).collect()
            });
            InputDefinition::Command {
                id: input.id.clone(),
                label: Some(input.description.clone()),
                command: input.command.clone(),
                args,
            }
        }
    }
}

fn effective_description(id: &str, label: &Option<String>, description: &Option<String>) -> String {
    label
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| description.clone().filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| id.to_string())
}

/// Applies the single client-owned validation and portability boundary for input definitions.
pub(crate) fn prepare_portable_input_definitions(
    inputs: &[InputDefinition],
) -> Result<Vec<InputDefinition>, String> {
    let mut ids = HashSet::new();
    let mut prepared = Vec::with_capacity(inputs.len());
    for input in inputs.iter().cloned() {
        let mut input = input;
        let id = input.id();
        if id.is_empty() || id.trim() != id {
            return Err("Input id must be non-empty and trimmed".to_string());
        }
        if !ids.insert(id.to_string()) {
            return Err(format!("Duplicate Input id: {id}"));
        }

        match &mut input {
            InputDefinition::PromptString {
                id,
                label,
                description,
                default,
                password,
            } => {
                normalize_optional_text(label);
                normalize_optional_text(description);
                normalize_optional_text(default);
                if *password == Some(true) && default.is_some() {
                    return Err(format!(
                        "Password PromptString input '{id}' cannot contain a plaintext default"
                    ));
                }
            }
            InputDefinition::PickString {
                id,
                label,
                description,
                options,
                default,
            } => {
                normalize_optional_text(label);
                normalize_optional_text(description);
                if options.is_empty() {
                    return Err(format!(
                        "PickString input '{id}' must define at least one option"
                    ));
                }
                for option in &mut *options {
                    option.label = option.label.trim().to_string();
                    if option.label.is_empty() {
                        return Err(format!("PickString input '{id}' option label is required"));
                    }
                    if option.value.trim().is_empty() {
                        return Err(format!("PickString input '{id}' option value is required"));
                    }
                }
                if let Some(default) = default {
                    if !options.iter().any(|option| option.value == *default) {
                        return Err(format!(
                            "PickString input '{id}' default must match an option value"
                        ));
                    }
                }
            }
            InputDefinition::Command {
                id,
                label,
                command,
                args,
            } => {
                normalize_optional_text(label);
                *command = command.trim().to_string();
                if command.is_empty() {
                    return Err(format!("Command input '{id}' command is required"));
                }
                if let Some(args) = args {
                    ensure_portable_cli_arguments(&format!("inputs.{id}.args"), args)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        prepared.push(input);
    }
    Ok(prepared)
}

fn normalize_optional_text(value: &mut Option<String>) {
    *value = value
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputValueView {
    pub configured: bool,
    pub status: InputValueStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputValueStatus {
    Configured,
    UsingDefault,
    FirstOption,
    InvalidSelection,
    Missing,
    RuntimeCommand,
}

#[tauri::command]
pub async fn list_input_entries(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<InputEntryView>, String> {
    list_input_entries_core(&state, &instance_id)
}

pub fn list_input_entries_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<InputEntryView>, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    let store = migrated_input_entry_store(state, instance_id)?;
    store.list()
}

/// Create or update a client-owned InputEntry. A missing value preserves the current value, which
/// allows UI to move an existing secret between backends without reading its plaintext.
#[tauri::command]
pub async fn upsert_input_entry(
    state: State<'_, AppState>,
    instance_id: String,
    key: String,
    value: Option<String>,
    secret: bool,
) -> Result<(), String> {
    upsert_input_entry_core(&state, &instance_id, &key, value, secret).await
}

pub async fn upsert_input_entry_core(
    state: &AppState,
    instance_id: &str,
    key: &str,
    value: Option<String>,
    secret: bool,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    require_existing_instance(state, instance_id)?;
    migrated_input_entry_store(state, instance_id)?.upsert(
        key,
        value.map(serde_json::Value::String),
        secret,
    )
}

#[tauri::command]
pub async fn delete_input_entry(
    state: State<'_, AppState>,
    instance_id: String,
    key: String,
) -> Result<(), String> {
    delete_input_entry_core(&state, &instance_id, &key).await
}

pub async fn delete_input_entry_core(
    state: &AppState,
    instance_id: &str,
    key: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    require_existing_instance(state, instance_id)?;
    migrated_input_entry_store(state, instance_id)?.delete(key)
}

fn input_entry_store(state: &AppState, instance_id: &str) -> InputEntryStore {
    InputEntryStore::for_computer(
        state.config.as_ref(),
        instance_id.to_string(),
        state.secret_store.clone(),
    )
}

fn migrated_input_entry_store(
    state: &AppState,
    instance_id: &str,
) -> Result<InputEntryStore, String> {
    let store = input_entry_store(state, instance_id);
    let preferred = state
        .sdk_config
        .load_input_definitions(instance_id)
        .into_iter()
        .filter_map(|definition| {
            definition.storage_kind().map(|kind| {
                (
                    definition.id().to_string(),
                    InputEntryStorageKind::from(kind),
                )
            })
        })
        .collect();
    store.migrate_legacy(
        input_value_index::load_with_provenance(state.config.as_ref(), instance_id)?,
        &preferred,
    )?;
    Ok(store)
}

pub(crate) struct InputDefinitionsConfigSnapshot {
    config: ProjectConfigDoc,
}

#[derive(Debug)]
pub(crate) enum InputDefinitionsConfigMutationError {
    Unchanged(String),
    Reverted(String),
    OutcomeUncertain(String),
}

impl InputDefinitionsConfigMutationError {
    pub(crate) fn is_safe_to_abort(&self) -> bool {
        matches!(self, Self::Unchanged(_) | Self::Reverted(_))
    }
}

impl std::fmt::Display for InputDefinitionsConfigMutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unchanged(message)
            | Self::Reverted(message)
            | Self::OutcomeUncertain(message) => formatter.write_str(message),
        }
    }
}

/// List all input variable definitions
#[tauri::command]
pub async fn list_inputs(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<InputDefinition>, String> {
    list_inputs_core(&state, &instance_id)
}

pub fn list_inputs_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<InputDefinition>, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    Ok(state.sdk_config.load_input_definitions(instance_id))
}

/// Get a single input definition by ID
#[tauri::command]
pub async fn get_input(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<InputDefinition>, String> {
    get_input_core(&state, &instance_id, &id)
}

pub fn get_input_core(
    state: &AppState,
    instance_id: &str,
    id: &str,
) -> Result<Option<InputDefinition>, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    let inputs = state.sdk_config.load_input_definitions(instance_id);
    Ok(inputs.into_iter().find(|i| i.id() == id))
}

/// Returns the exact definition currently used by the runtime, including plugin-owned inputs.
/// This is definition context for PickString/PromptString rendering only; it has no storage
/// authority over client-owned InputEntries.
#[tauri::command]
pub async fn get_runtime_input(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<InputDefinition>, String> {
    get_runtime_input_core(&state, &instance_id, &id).await
}

pub async fn get_runtime_input_core(
    state: &AppState,
    instance_id: &str,
    id: &str,
) -> Result<Option<InputDefinition>, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    if let Some(runtime) = state.computer_registry.runtime(instance_id).await {
        if let Some(definition) = runtime.runtime_input_definition(id).await {
            return Ok(Some(input_definition_from_sdk(&definition)));
        }
    }
    get_input_core(state, instance_id, id)
}

/// Add or update an input variable definition
#[tauri::command]
pub async fn add_or_update_input(
    state: State<'_, AppState>,
    instance_id: String,
    input: InputDefinition,
) -> Result<(), String> {
    add_or_update_input_core(&state, &instance_id, input).await
}

pub async fn add_or_update_input_core(
    state: &AppState,
    instance_id: &str,
    input: InputDefinition,
) -> Result<(), String> {
    let input = prepare_portable_input_definitions(std::slice::from_ref(&input))?
        .into_iter()
        .next()
        .expect("one input definition was prepared");
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    let id = input.id().to_string();
    log::info!("Adding/updating input for instance {}: {}", instance_id, id);
    require_existing_instance(state, instance_id)?;

    let previous_config = state
        .sdk_config
        .load_project_input_document(instance_id)
        .map_err(|e| e.to_string())?;
    let mut inputs = state
        .sdk_config
        .load_project_input_definitions(instance_id)
        .map_err(|error| error.to_string())?;
    inputs.retain(|i| i.id() != id);
    inputs.push(input);
    if let Err(error) = state
        .sdk_config
        .replace_input_definitions(instance_id, &inputs)
    {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_config),
            error.to_string(),
        )
        .await);
    }

    Ok(())
}

fn validate_pick_selection(id: &str, options: &[PickOption], value: &str) -> Result<(), String> {
    if options.iter().any(|option| option.value == value) {
        Ok(())
    } else {
        Err(format!(
            "PickString input '{id}' selection does not match any current option value"
        ))
    }
}

fn input_type_name(input: &InputDefinition) -> &'static str {
    match input {
        InputDefinition::PromptString { .. } => "PromptString",
        InputDefinition::PickString { .. } => "PickString",
        InputDefinition::Command { .. } => "Command",
    }
}

/// Remove an input variable definition
#[tauri::command]
pub async fn remove_input(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<(), String> {
    remove_input_core(&state, &instance_id, &id).await
}

pub async fn remove_input_core(
    state: &AppState,
    instance_id: &str,
    id: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    log::info!("Removing input for instance {}: {}", instance_id, id);
    require_existing_instance(state, instance_id)?;

    let references = find_project_input_references(
        &state
            .sdk_config
            .load_raw_project_config(instance_id)
            .map_err(|error| error.to_string())?,
    )
    .into_iter()
    .filter(|reference| reference.input_id == id)
    .collect::<Vec<_>>();
    if !references.is_empty() {
        let locations = references
            .iter()
            .map(|reference| {
                format!(
                    "{}:{}:{}",
                    reference.layer, reference.server_name, reference.field_path
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Input '{id}' is still referenced by MCP configuration: {locations}"
        ));
    }

    let previous_config = state
        .sdk_config
        .load_project_input_document(instance_id)
        .map_err(|e| e.to_string())?;
    let mut inputs = state
        .sdk_config
        .load_project_input_definitions(instance_id)
        .map_err(|error| error.to_string())?;
    let original_len = inputs.len();
    inputs.retain(|i| i.id() != id);

    if inputs.len() == original_len {
        return Err(format!("Input not found: {}", id));
    }

    if let Err(error) = state
        .sdk_config
        .replace_input_definitions(instance_id, &inputs)
    {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_config),
            error.to_string(),
        )
        .await);
    }

    Ok(())
}

#[tauri::command]
pub async fn list_input_reference_issues(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<InputReferenceLocation>, String> {
    list_input_reference_issues_core(&state, &instance_id)
}

pub fn list_input_reference_issues_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<InputReferenceLocation>, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    let document = state
        .sdk_config
        .load_raw_project_config(instance_id)
        .map_err(|error| error.to_string())?;
    Ok(find_project_input_references(&document))
}

/// List all cached input values
#[tauri::command]
pub async fn list_input_values(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<std::collections::HashMap<String, InputValueView>, String> {
    list_input_values_core(&state, require_instance_id(&instance_id)?)
}

pub fn list_input_values_for_control_core(
    state: &AppState,
    instance_id: &str,
) -> Result<std::collections::HashMap<String, InputValueView>, String> {
    list_input_values_core(state, require_instance_id(instance_id)?)
}

/// Get a single cached input value
#[tauri::command]
pub async fn get_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<InputValueView>, String> {
    get_input_value_core(&state, &instance_id, &id)
}

pub fn get_input_value_core(
    state: &AppState,
    instance_id: &str,
    id: &str,
) -> Result<Option<InputValueView>, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    let definition = state
        .sdk_config
        .load_input_definitions(instance_id)
        .into_iter()
        .find(|input| input.id() == id)
        .ok_or_else(|| format!("Input not found: {id}"))?;
    input_value_view(state, instance_id, &definition).map(Some)
}

/// Set a cached input value
#[tauri::command]
pub async fn set_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
    value: serde_json::Value,
) -> Result<(), String> {
    set_input_value_core(&state, &instance_id, id, value).await
}

pub async fn set_input_value_core(
    state: &AppState,
    instance_id: &str,
    id: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    log::info!("Setting input value: {}", id);
    require_existing_instance(state, instance_id)?;
    let inputs = state.sdk_config.load_input_definitions(instance_id);
    let definition = inputs
        .iter()
        .find(|input| input.id() == id)
        .ok_or_else(|| format!("Input not found: {id}"))?;
    if !definition.supports_persistent_value() {
        return Err(format!(
            "{} inputs do not support persistent values",
            input_type_name(definition)
        ));
    }
    let string_value = value.as_str().ok_or_else(|| {
        format!(
            "{} input '{id}' must be a string",
            input_type_name(definition)
        )
    })?;
    if let InputDefinition::PickString { options, .. } = definition {
        validate_pick_selection(&id, options, string_value)?;
    }
    let store = migrated_input_entry_store(state, instance_id)?;
    let secret = store
        .storage_kind(&id)?
        .map(InputEntryStorageKind::is_secret)
        .unwrap_or_else(|| definition.is_secret());
    store.upsert(&id, Some(value), secret)
}

/// Stores a Computer-level InputEntry for an exact definition present in the SDK runtime InputPool.
///
/// The SDK definition remains runtime-only, while the resulting Entry is visible to normal
/// InputEntry management. Returns `false` when the exact runtime definition does not exist.
#[tauri::command]
pub async fn set_runtime_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
    value: serde_json::Value,
) -> Result<bool, String> {
    set_runtime_input_value_core(&state, &instance_id, id, value).await
}

pub async fn set_runtime_input_value_core(
    state: &AppState,
    instance_id: &str,
    id: String,
    value: serde_json::Value,
) -> Result<bool, String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    require_existing_instance(state, instance_id)?;
    let Some(runtime) = state.computer_registry.runtime(instance_id).await else {
        return Ok(false);
    };
    let Some(kind) = runtime.runtime_input_kind(&id).await else {
        return Ok(false);
    };

    log::info!(
        "Setting Computer InputEntry requested by runtime {} input for instance {}: {}",
        kind,
        instance_id,
        id
    );
    if !value.is_string() {
        return Err(format!("Runtime input '{id}' must be a string"));
    }
    let store = migrated_input_entry_store(state, instance_id)?;
    let secret = store
        .storage_kind(&id)?
        .map(InputEntryStorageKind::is_secret)
        .unwrap_or(matches!(kind, InputKind::Secret));
    store.upsert(&id, Some(value), secret)?;
    Ok(true)
}

/// Remove a cached input value
#[tauri::command]
pub async fn remove_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<(), String> {
    remove_input_value_core(&state, &instance_id, &id).await
}

pub async fn remove_input_value_core(
    state: &AppState,
    instance_id: &str,
    id: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    require_existing_instance(state, instance_id)?;
    migrated_input_entry_store(state, instance_id)?.delete(id)
}

/// Clear all cached input values
#[tauri::command]
pub async fn clear_input_values(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    clear_input_values_core(&state, &instance_id).await
}

pub async fn clear_input_values_core(state: &AppState, instance_id: &str) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    require_existing_instance(state, instance_id)?;
    let store = migrated_input_entry_store(state, instance_id)?;
    for entry in store.list()? {
        store.delete(&entry.key)?;
    }
    Ok(())
}

/// Import input definitions from a JSON file
#[tauri::command]
pub async fn import_inputs(
    state: State<'_, AppState>,
    instance_id: String,
    path: String,
) -> Result<usize, String> {
    import_inputs_core(&state, &instance_id, &path).await
}

/// Execute an unsaved Command Input definition once without mutating Computer configuration.
#[tauri::command]
pub async fn preview_command_input(
    state: State<'_, AppState>,
    instance_id: String,
    command: String,
    args: Vec<String>,
) -> Result<CommandPreviewResult, String> {
    preview_command_input_core(&state, &instance_id, command, args).await
}

pub async fn preview_command_input_core(
    state: &AppState,
    instance_id: &str,
    command: String,
    args: Vec<String>,
) -> Result<CommandPreviewResult, String> {
    let instance_id = require_instance_id(instance_id)?;
    require_existing_instance(state, instance_id)?;
    let command = command.trim();
    if command.is_empty() {
        return Err("Command is required".to_string());
    }

    let stdout = run_command(command, &args)
        .await
        .map_err(|error| truncate_command_preview_text(&error.to_string()).0)?;
    let (stdout, truncated) = truncate_command_preview_text(&stdout);
    Ok(CommandPreviewResult { stdout, truncated })
}

fn truncate_command_preview_text(value: &str) -> (String, bool) {
    if value.len() <= COMMAND_PREVIEW_OUTPUT_LIMIT_BYTES {
        return (value.to_string(), false);
    }

    let mut end = COMMAND_PREVIEW_OUTPUT_LIMIT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

pub async fn import_inputs_core(
    state: &AppState,
    instance_id: &str,
    path: &str,
) -> Result<usize, String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
    require_existing_instance(state, instance_id)?;
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let imported: Vec<InputDefinition> =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let imported = prepare_portable_input_definitions(&imported)?;
    let count = imported.len();

    let previous_config = state
        .sdk_config
        .load_project_input_document(instance_id)
        .map_err(|e| e.to_string())?;
    let mut inputs = state
        .sdk_config
        .load_project_input_definitions(instance_id)
        .map_err(|error| error.to_string())?;
    for input in imported {
        let id = input.id().to_string();
        inputs.retain(|i| i.id() != id);
        inputs.push(input);
    }
    if let Err(error) = state
        .sdk_config
        .replace_input_definitions(instance_id, &inputs)
    {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_config),
            error.to_string(),
        )
        .await);
    }
    Ok(count)
}

/// Replaces client-owned input definitions without rebuilding or reloading any runtime.
///
/// Configuration import uses this boundary so definition persistence remains independent from
/// runtime availability. The returned snapshot can roll the change back if the paired SDK config
/// mutation fails.
pub(crate) fn replace_input_definitions_config_only_locked(
    sdk_config: &SdkConfigService,
    instance_id: &str,
    definitions: &[InputDefinition],
) -> Result<InputDefinitionsConfigSnapshot, InputDefinitionsConfigMutationError> {
    let definitions = prepare_portable_input_definitions(definitions)
        .map_err(InputDefinitionsConfigMutationError::Unchanged)?;
    let previous = sdk_config
        .load_project_input_document(instance_id)
        .map_err(|error| InputDefinitionsConfigMutationError::Unchanged(error.to_string()))?;
    let snapshot = InputDefinitionsConfigSnapshot { config: previous };

    if let Err(primary_error) = sdk_config.replace_input_definitions(instance_id, &definitions) {
        return Err(
            match restore_input_definitions_config_only_locked(
                sdk_config,
                instance_id,
                &snapshot,
            ) {
                Ok(()) => InputDefinitionsConfigMutationError::Reverted(format!(
                    "Failed to update input definitions; changes were reverted: {primary_error}"
                )),
                Err(rollback_error) => {
                    InputDefinitionsConfigMutationError::OutcomeUncertain(format!(
                        "Failed to update input definitions: {primary_error}; rollback also failed: {rollback_error}"
                    ))
                }
            },
        );
    }
    Ok(snapshot)
}

pub(crate) fn restore_input_definitions_config_only_locked(
    sdk_config: &SdkConfigService,
    instance_id: &str,
    snapshot: &InputDefinitionsConfigSnapshot,
) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = sdk_config.restore_project_input_document(instance_id, &snapshot.config) {
        errors.push(format!("restore input definitions: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn rollback_input_mutation(
    state: &AppState,
    instance_id: &str,
    previous_inputs: Option<&ProjectConfigDoc>,
    primary_error: String,
) -> String {
    let mut rollback_errors = Vec::new();
    if let Some(inputs) = previous_inputs {
        if let Err(error) = state
            .sdk_config
            .restore_project_input_document(instance_id, inputs)
        {
            rollback_errors.push(format!("restore Computer input definitions: {error}"));
        }
    }
    if rollback_errors.is_empty() {
        format!("Failed to apply Computer input mutation; changes were reverted: {primary_error}")
    } else {
        format!(
            "Failed to apply Computer input mutation: {primary_error}; rollback also failed and the outcome is uncertain: {}",
            rollback_errors.join("; ")
        )
    }
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}

fn require_existing_instance(state: &AppState, instance_id: &str) -> Result<(), String> {
    state
        .config
        .get_computer_instance(instance_id)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn list_input_values_core(
    state: &AppState,
    instance_id: &str,
) -> Result<std::collections::HashMap<String, InputValueView>, String> {
    require_existing_instance(state, instance_id)?;
    let mut values = std::collections::HashMap::new();
    for input in state.sdk_config.load_input_definitions(instance_id) {
        values.insert(
            input.id().to_string(),
            input_value_view(state, instance_id, &input)?,
        );
    }
    Ok(values)
}

fn input_value_view(
    state: &AppState,
    instance_id: &str,
    definition: &InputDefinition,
) -> Result<InputValueView, String> {
    if let InputDefinition::Command { .. } = definition {
        return Ok(InputValueView {
            configured: false,
            status: InputValueStatus::RuntimeCommand,
            value: None,
        });
    }
    let legacy_preference = if definition.is_secret() {
        InputEntryStorageKind::Secret
    } else {
        InputEntryStorageKind::Value
    };
    let stored = migrated_input_entry_store(state, instance_id)?
        .resolve_entry(definition.id(), legacy_preference)?;
    match definition {
        InputDefinition::PromptString { default, .. } => Ok(match stored {
            Some(entry) => InputValueView {
                configured: true,
                status: InputValueStatus::Configured,
                value: (!entry.storage_kind.is_secret()).then_some(entry.value),
            },
            None if default.is_some() => InputValueView {
                configured: false,
                status: InputValueStatus::UsingDefault,
                value: None,
            },
            None => InputValueView {
                configured: false,
                status: InputValueStatus::Missing,
                value: None,
            },
        }),
        InputDefinition::PickString {
            options, default, ..
        } => Ok(match stored {
            Some(entry)
                if entry.value.as_str().is_some_and(|selected| {
                    options.iter().any(|option| option.value == selected)
                }) =>
            {
                InputValueView {
                    configured: true,
                    status: InputValueStatus::Configured,
                    value: (!entry.storage_kind.is_secret()).then_some(entry.value),
                }
            }
            Some(entry) => InputValueView {
                configured: true,
                status: InputValueStatus::InvalidSelection,
                value: (!entry.storage_kind.is_secret()).then_some(entry.value),
            },
            None if default.is_some() => InputValueView {
                configured: false,
                status: InputValueStatus::UsingDefault,
                value: None,
            },
            None => InputValueView {
                configured: false,
                status: InputValueStatus::Missing,
                value: None,
            },
        }),
        InputDefinition::Command { .. } => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::input_value_store::InputValueStore;
    use crate::services::keychain::{self, InMemorySecretStore, KeychainError, SecretStore};
    use crate::services::observability::ObservabilityService;
    use crate::services::settings::SettingsService;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_state() -> (AppState, Arc<InMemorySecretStore>, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let store = Arc::new(InMemorySecretStore::default());
        let state = AppState::new_with_secret_store(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            store.clone(),
        );
        (state, store, dir)
    }

    #[tokio::test]
    async fn command_preview_returns_stdout_without_persisting_an_input_definition() {
        let (state, _store, _dir) = test_state();

        let result = preview_command_input_core(
            &state,
            "computer-a",
            "echo".to_string(),
            vec!["preview-output".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(result.stdout, "preview-output");
        assert!(!result.truncated);
        assert!(list_inputs_core(&state, "computer-a").unwrap().is_empty());
    }

    #[tokio::test]
    async fn command_preview_reports_command_failures() {
        let (state, _store, _dir) = test_state();

        let error =
            preview_command_input_core(&state, "computer-a", "exit 7".to_string(), Vec::new())
                .await
                .unwrap_err();

        assert!(error.contains("exit code 7"));
        assert!(list_inputs_core(&state, "computer-a").unwrap().is_empty());
    }

    #[tokio::test]
    async fn command_preview_rejects_blank_commands_before_execution() {
        let (state, _store, _dir) = test_state();

        let error = preview_command_input_core(&state, "computer-a", "   ".to_string(), Vec::new())
            .await
            .unwrap_err();

        assert_eq!(error, "Command is required");
    }

    #[test]
    fn command_preview_truncation_preserves_utf8_boundaries() {
        let output = format!("{}界", "a".repeat(COMMAND_PREVIEW_OUTPUT_LIMIT_BYTES - 1));

        let (truncated, was_truncated) = truncate_command_preview_text(&output);

        assert!(was_truncated);
        assert_eq!(
            truncated,
            "a".repeat(COMMAND_PREVIEW_OUTPUT_LIMIT_BYTES - 1)
        );
    }

    #[test]
    fn list_inputs_projects_effective_local_definitions_for_value_management() {
        let (state, _store, _dir) = test_state();
        state
            .sdk_config
            .save(
                "computer-a",
                &a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc {
                    mcp_local: Some(
                        serde_json::json!({
                            "inputs": [{
                                "type": "PromptString",
                                "id": "local-token",
                                "description": "Local token",
                                "password": true
                            }]
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        assert!(matches!(
            list_inputs_core(&state, "computer-a").unwrap().as_slice(),
            [InputDefinition::PromptString {
                id,
                password: Some(true),
                ..
            }] if id == "local-token"
        ));
        assert!(list_input_values_core(&state, "computer-a")
            .unwrap()
            .contains_key("local-token"));
    }

    #[derive(Default)]
    struct FailOnceDeleteSecretStore {
        inner: InMemorySecretStore,
        fail_next_delete: AtomicBool,
    }

    impl SecretStore for FailOnceDeleteSecretStore {
        fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
            self.inner.set_secret(key, secret)
        }

        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            self.inner.get_secret(key)
        }

        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            if self.fail_next_delete.swap(false, Ordering::SeqCst) {
                return Err(KeychainError::Store("injected delete failure".to_string()));
            }
            self.inner.delete_secret(key)
        }
    }

    struct AlwaysFailSecretStore;

    impl SecretStore for AlwaysFailSecretStore {
        fn set_secret(&self, _key: &str, _secret: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Store("keychain unavailable".to_string()))
        }

        fn get_secret(&self, _key: &str) -> Result<Option<String>, KeychainError> {
            Err(KeychainError::Store("keychain unavailable".to_string()))
        }

        fn delete_secret(&self, _key: &str) -> Result<(), KeychainError> {
            Err(KeychainError::Store("keychain unavailable".to_string()))
        }
    }

    #[tokio::test]
    async fn plain_values_never_depend_on_the_keychain_across_their_lifecycle() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let state = AppState::new_with_secret_store(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            Arc::new(AlwaysFailSecretStore),
        );
        let definition = InputDefinition::PromptString {
            id: "plain".to_string(),
            label: None,
            description: Some("Plain value".to_string()),
            default: None,
            password: Some(false),
        };
        add_or_update_input_core(&state, "computer-a", definition.clone())
            .await
            .unwrap();

        set_input_value_core(
            &state,
            "computer-a",
            "plain".to_string(),
            serde_json::json!("first"),
        )
        .await
        .unwrap();
        assert_eq!(
            get_input_value_core(&state, "computer-a", "plain")
                .unwrap()
                .unwrap()
                .value,
            Some(serde_json::json!("first"))
        );
        remove_input_value_core(&state, "computer-a", "plain")
            .await
            .unwrap();
        set_input_value_core(
            &state,
            "computer-a",
            "plain".to_string(),
            serde_json::json!("second"),
        )
        .await
        .unwrap();
        clear_input_values_core(&state, "computer-a").await.unwrap();

        let runtime = state.computer_registry.runtime("computer-a").await.unwrap();
        runtime
            .add_or_update_input(input_definition_to_sdk(&definition))
            .await
            .unwrap();
        assert!(set_runtime_input_value_core(
            &state,
            "computer-a",
            "plain".to_string(),
            serde_json::json!("runtime"),
        )
        .await
        .unwrap());

        let snapshot = replace_input_definitions_config_only_locked(
            state.sdk_config.as_ref(),
            "computer-a",
            std::slice::from_ref(&definition),
        )
        .unwrap();
        restore_input_definitions_config_only_locked(
            state.sdk_config.as_ref(),
            "computer-a",
            &snapshot,
        )
        .unwrap();

        crate::commands::computer::delete_computer_instance_core(&state, "computer-a".to_string())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn password_input_uses_secret_namespace_and_list_never_returns_plaintext() {
        let (state, store, _dir) = test_state();
        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "api-key".to_string(),
                label: Some("API Key".to_string()),
                description: None,
                default: None,
                password: Some(true),
            },
        )
        .await
        .unwrap();

        set_input_value_core(
            &state,
            "computer-a",
            "api-key".to_string(),
            serde_json::json!("top-secret"),
        )
        .await
        .unwrap();

        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "api-key")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("api-key")
                .unwrap(),
            None
        );
        let view = list_input_values_core(&state, "computer-a").unwrap();
        assert_eq!(
            view.get("api-key"),
            Some(&InputValueView {
                configured: true,
                status: InputValueStatus::Configured,
                value: None,
            })
        );
        assert!(!serde_json::to_string(&view).unwrap().contains("top-secret"));
    }

    #[tokio::test]
    async fn definition_edit_does_not_change_the_independent_secret_entry() {
        let (state, store, _dir) = test_state();
        let definition = InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: None,
            description: Some("API key".to_string()),
            default: None,
            password: Some(true),
        };
        add_or_update_input_core(&state, "computer-a", definition.clone())
            .await
            .unwrap();
        upsert_input_entry_core(
            &state,
            "computer-a",
            "api-key",
            Some("top-secret".to_string()),
            true,
        )
        .await
        .unwrap();
        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "api-key".to_string(),
                label: Some("API Key".to_string()),
                description: Some("API key".to_string()),
                default: None,
                password: Some(true),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "api-key")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        assert!(
            !serde_json::to_string(&list_inputs_core(&state, "computer-a").unwrap())
                .unwrap()
                .contains("top-secret")
        );
    }

    #[tokio::test]
    async fn post_replace_definition_write_failure_restores_prompt_definition() {
        let (state, store, _dir) = test_state();
        state.sdk_config.inject_raw_restore_failure();

        let error = add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "token".to_string(),
                label: None,
                description: None,
                default: None,
                password: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error.contains("changes were reverted"));
        assert!(list_inputs_core(&state, "computer-a").unwrap().is_empty());
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("token")
                .unwrap(),
            None
        );
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "token").unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn recreated_password_definition_can_reuse_retained_secret() {
        let (state, store, _dir) = test_state();
        upsert_input_entry_core(
            &state,
            "computer-a",
            "api-key",
            Some("stale-secret".to_string()),
            true,
        )
        .await
        .unwrap();

        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "api-key".to_string(),
                label: None,
                description: None,
                default: None,
                password: Some(true),
            },
        )
        .await
        .unwrap();

        assert!(matches!(
            list_inputs_core(&state, "computer-a").unwrap().as_slice(),
            [InputDefinition::PromptString {
                id,
                password: Some(true),
                ..
            }] if id == "api-key"
        ));
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "api-key")
                .unwrap()
                .as_deref(),
            Some("stale-secret")
        );
    }

    #[tokio::test]
    async fn definition_edit_is_persisted_without_a_live_runtime() {
        let (state, _store, _dir) = test_state();
        state
            .computer_registry
            .remove_runtime("computer-a")
            .await
            .unwrap();

        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PickString {
                id: "first".to_string(),
                label: Some("First".to_string()),
                description: None,
                options: vec![PickOption {
                    label: "One".to_string(),
                    value: "one".to_string(),
                }],
                default: None,
            },
        )
        .await
        .unwrap();

        assert!(matches!(
            list_inputs_core(&state, "computer-a").unwrap().as_slice(),
            [InputDefinition::PickString { id, options, .. }]
                if id == "first" && options[0].value == "one"
        ));
        assert!(state
            .config
            .get_computer_instance("computer-a")
            .unwrap()
            .inputs
            .is_empty());
    }

    #[test]
    fn config_import_snapshot_restores_sdk_project_inputs() {
        let (state, _store, _dir) = test_state();
        let original = [InputDefinition::PromptString {
            id: "original".to_string(),
            label: None,
            description: Some("Original".to_string()),
            default: None,
            password: None,
        }];
        state
            .sdk_config
            .replace_input_definitions("computer-a", &original)
            .unwrap();
        let replacement = [InputDefinition::PromptString {
            id: "token".to_string(),
            label: None,
            description: None,
            default: None,
            password: None,
        }];

        let snapshot = replace_input_definitions_config_only_locked(
            state.sdk_config.as_ref(),
            "computer-a",
            &replacement,
        )
        .unwrap();
        restore_input_definitions_config_only_locked(
            state.sdk_config.as_ref(),
            "computer-a",
            &snapshot,
        )
        .unwrap();

        assert!(matches!(
            state.sdk_config.load_input_definitions("computer-a").as_slice(),
            [InputDefinition::PromptString { id, .. }] if id == "original"
        ));
    }

    #[tokio::test]
    async fn pick_persists_exact_values_while_command_remains_runtime_only() {
        let (state, store, _dir) = test_state();
        for input in [
            InputDefinition::PickString {
                id: "region".to_string(),
                label: None,
                description: None,
                options: vec![PickOption {
                    label: "US".to_string(),
                    value: "us".to_string(),
                }],
                default: None,
            },
            InputDefinition::Command {
                id: "whoami".to_string(),
                label: None,
                command: "whoami".to_string(),
                args: None,
            },
        ] {
            add_or_update_input_core(&state, "computer-a", input)
                .await
                .unwrap();
        }

        set_input_value_core(
            &state,
            "computer-a",
            "region".to_string(),
            serde_json::json!("us"),
        )
        .await
        .unwrap();
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("region")
                .unwrap(),
            Some(serde_json::json!("us"))
        );
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "region").unwrap(),
            None
        );
        assert!(set_input_value_core(
            &state,
            "computer-a",
            "region".to_string(),
            serde_json::json!("US"),
        )
        .await
        .unwrap_err()
        .contains("does not match any current option value"));
        remove_input_value_core(&state, "computer-a", "region")
            .await
            .unwrap();

        assert!(set_input_value_core(
            &state,
            "computer-a",
            "whoami".to_string(),
            serde_json::json!("value"),
        )
        .await
        .unwrap_err()
        .contains("do not support persistent values"));
        assert!(remove_input_value_core(&state, "computer-a", "whoami")
            .await
            .unwrap_err()
            .contains("InputEntry not found"));
    }

    #[test]
    fn pick_validation_allows_duplicates_and_requires_an_exact_default_value() {
        let empty = InputDefinition::PickString {
            id: "region".to_string(),
            label: Some("  ".to_string()),
            description: None,
            options: Vec::new(),
            default: None,
        };
        assert!(prepare_portable_input_definitions(&[empty])
            .unwrap_err()
            .contains("at least one option"));

        let duplicate = InputDefinition::PickString {
            id: "region".to_string(),
            label: None,
            description: None,
            options: vec![
                PickOption {
                    label: "US".to_string(),
                    value: "us".to_string(),
                },
                PickOption {
                    label: "US duplicate".to_string(),
                    value: "us".to_string(),
                },
            ],
            default: None,
        };
        assert!(prepare_portable_input_definitions(&[duplicate]).is_ok());

        let invalid_default = InputDefinition::PickString {
            id: "region".to_string(),
            label: None,
            description: None,
            options: vec![PickOption {
                label: "US".to_string(),
                value: "us".to_string(),
            }],
            default: Some("US".to_string()),
        };
        assert!(prepare_portable_input_definitions(&[invalid_default])
            .unwrap_err()
            .contains("default must match an option value"));
    }

    #[tokio::test]
    async fn plugin_runtime_definition_creates_a_managed_computer_input_entry() {
        let (state, store, _dir) = test_state();
        let instance = state.config.get_computer_instance("computer-a").unwrap();
        let runtime = state
            .computer_registry
            .upsert_runtime(instance)
            .await
            .unwrap();
        runtime
            .add_or_update_input(
                a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput::PromptString(
                    a2c_smcp::smcp_computer::mcp_clients::model::PromptStringInput {
                        id: "audit@acme/api-key".to_string(),
                        description: "Plugin API key".to_string(),
                        default: None,
                        password: Some(true),
                    },
                ),
            )
            .await
            .unwrap();

        assert!(set_runtime_input_value_core(
            &state,
            "computer-a",
            "audit@acme/api-key".to_string(),
            serde_json::json!("top-secret"),
        )
        .await
        .unwrap());

        assert!(list_inputs_core(&state, "computer-a").unwrap().is_empty());
        assert!(list_input_values_core(&state, "computer-a")
            .unwrap()
            .is_empty());
        assert_eq!(
            list_input_entries_core(&state, "computer-a").unwrap(),
            vec![InputEntryView {
                key: "audit@acme/api-key".to_string(),
                secret: true,
                value: None,
            }]
        );
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "audit@acme/api-key")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("audit@acme/api-key")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn runtime_only_value_rejects_unknown_definition_without_writing_storage() {
        let (state, store, _dir) = test_state();
        let instance = state.config.get_computer_instance("computer-a").unwrap();
        state
            .computer_registry
            .upsert_runtime(instance)
            .await
            .unwrap();

        assert!(!set_runtime_input_value_core(
            &state,
            "computer-a",
            "missing".to_string(),
            serde_json::json!("value"),
        )
        .await
        .unwrap());
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("missing")
                .unwrap(),
            None
        );
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "missing").unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn removing_a_definition_preserves_history_while_explicit_clear_removes_current_values() {
        let (state, store, _dir) = test_state();
        for id in ["remove-me", "clear-me"] {
            add_or_update_input_core(
                &state,
                "computer-a",
                InputDefinition::PromptString {
                    id: id.to_string(),
                    label: Some(id.to_string()),
                    description: None,
                    default: None,
                    password: Some(true),
                },
            )
            .await
            .unwrap();
            set_input_value_core(
                &state,
                "computer-a",
                id.to_string(),
                serde_json::json!(format!("{id}-secret")),
            )
            .await
            .unwrap();
        }

        remove_input_core(&state, "computer-a", "remove-me")
            .await
            .unwrap();
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "remove-me").unwrap(),
            Some("remove-me-secret".to_string())
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("remove-me")
                .unwrap(),
            None
        );

        clear_input_values_core(&state, "computer-a").await.unwrap();
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "clear-me").unwrap(),
            None
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("clear-me")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn changing_definition_password_does_not_move_a_plain_entry() {
        let (state, store, _dir) = test_state();
        let value_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: Some("Credential".to_string()),
            description: None,
            default: None,
            password: Some(false),
        };
        add_or_update_input_core(&state, "computer-a", value_definition.clone())
            .await
            .unwrap();
        set_input_value_core(
            &state,
            "computer-a",
            "credential".to_string(),
            serde_json::json!("legacy-secret"),
        )
        .await
        .unwrap();

        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "credential".to_string(),
                label: Some("Credential".to_string()),
                description: None,
                default: None,
                password: Some(true),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "credential").unwrap(),
            None
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("credential")
                .unwrap(),
            Some(serde_json::json!("legacy-secret"))
        );
        assert_eq!(
            get_input_value_core(&state, "computer-a", "credential")
                .unwrap()
                .unwrap(),
            InputValueView {
                configured: true,
                status: InputValueStatus::Configured,
                value: Some(serde_json::json!("legacy-secret")),
            }
        );

        add_or_update_input_core(&state, "computer-a", value_definition.clone())
            .await
            .unwrap();
        assert_eq!(
            get_input_value_core(&state, "computer-a", "credential")
                .unwrap()
                .unwrap(),
            InputValueView {
                configured: true,
                status: InputValueStatus::Configured,
                value: Some(serde_json::json!("legacy-secret")),
            }
        );
    }

    #[tokio::test]
    async fn changing_definition_password_does_not_move_a_secret_entry() {
        let (state, store, _dir) = test_state();
        let secret_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: Some("Credential".to_string()),
            description: None,
            default: None,
            password: Some(true),
        };
        add_or_update_input_core(&state, "computer-a", secret_definition.clone())
            .await
            .unwrap();
        set_input_value_core(
            &state,
            "computer-a",
            "credential".to_string(),
            serde_json::json!("top-secret"),
        )
        .await
        .unwrap();

        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "credential".to_string(),
                label: Some("Credential".to_string()),
                description: None,
                default: None,
                password: Some(false),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "credential").unwrap(),
            Some("top-secret".to_string())
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("credential")
                .unwrap(),
            None
        );
        assert_eq!(
            list_input_values_core(&state, "computer-a")
                .unwrap()
                .get("credential")
                .unwrap(),
            &InputValueView {
                configured: true,
                status: InputValueStatus::Configured,
                value: None,
            }
        );

        add_or_update_input_core(&state, "computer-a", secret_definition)
            .await
            .unwrap();
        let view = get_input_value_core(&state, "computer-a", "credential")
            .unwrap()
            .unwrap();
        assert!(view.configured);
        assert_eq!(view.status, InputValueStatus::Configured);
        assert_eq!(view.value, None);
    }

    #[tokio::test]
    async fn definition_type_change_does_not_touch_inactive_namespaces() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let store = Arc::new(FailOnceDeleteSecretStore::default());
        let state = AppState::new_with_secret_store(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            store.clone(),
        );
        let secret_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: Some("Credential".to_string()),
            description: None,
            default: None,
            password: Some(true),
        };
        add_or_update_input_core(&state, "computer-a", secret_definition.clone())
            .await
            .unwrap();
        set_input_value_core(
            &state,
            "computer-a",
            "credential".to_string(),
            serde_json::json!("top-secret"),
        )
        .await
        .unwrap();
        store.fail_next_delete.store(true, Ordering::SeqCst);

        let value_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: Some("Credential".to_string()),
            description: None,
            default: None,
            password: Some(false),
        };
        add_or_update_input_core(&state, "computer-a", value_definition.clone())
            .await
            .unwrap();

        assert!(matches!(
            list_inputs_core(&state, "computer-a").unwrap().as_slice(),
            [InputDefinition::PromptString {
                id,
                password: Some(false),
                ..
            }] if id == "credential"
        ));
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "credential")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("credential")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn management_list_migrates_singleton_secret_index_over_a_stale_plain_copy() {
        let (state, secrets, _dir) = test_state();
        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "credential".to_string(),
                label: Some("Credential".to_string()),
                description: None,
                default: None,
                password: Some(false),
            },
        )
        .await
        .unwrap();
        keychain::set_input_secret(
            secrets.as_ref(),
            "computer-a",
            "credential",
            "current-secret",
        )
        .unwrap();
        InputValueStore::for_computer(state.config.as_ref(), "computer-a")
            .set("credential", &serde_json::json!("stale-plain"))
            .unwrap();
        input_value_index::record(
            state.config.as_ref(),
            "computer-a",
            "credential",
            InputValueStorageKind::Secret,
        )
        .unwrap();

        assert_eq!(
            list_input_entries_core(&state, "computer-a").unwrap(),
            vec![InputEntryView {
                key: "credential".to_string(),
                secret: true,
                value: None,
            }]
        );
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("credential")
                .unwrap(),
            None
        );
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "credential")
                .unwrap()
                .as_deref(),
            Some("current-secret")
        );
    }

    #[tokio::test]
    async fn input_entry_crud_is_independent_of_sdk_definitions() {
        let (state, _secrets, _dir) = test_state();

        upsert_input_entry_core(
            &state,
            "computer-a",
            "name",
            Some("zhangsan".to_string()),
            false,
        )
        .await
        .unwrap();

        assert!(list_inputs_core(&state, "computer-a").unwrap().is_empty());
        assert_eq!(
            list_input_entries_core(&state, "computer-a").unwrap(),
            vec![InputEntryView {
                key: "name".to_string(),
                secret: false,
                value: Some(serde_json::json!("zhangsan")),
            }]
        );

        delete_input_entry_core(&state, "computer-a", "name")
            .await
            .unwrap();
        assert!(list_input_entries_core(&state, "computer-a")
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn input_entry_secret_switch_moves_value_and_redacts_list_projection() {
        let (state, secrets, _dir) = test_state();
        upsert_input_entry_core(
            &state,
            "computer-a",
            "credential",
            Some("top-secret".to_string()),
            false,
        )
        .await
        .unwrap();

        upsert_input_entry_core(&state, "computer-a", "credential", None, true)
            .await
            .unwrap();
        assert_eq!(
            InputValueStore::for_computer(state.config.as_ref(), "computer-a")
                .get("credential")
                .unwrap(),
            None
        );
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "credential")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        let entries = list_input_entries_core(&state, "computer-a").unwrap();
        assert_eq!(entries[0].value, None);
        assert!(!serde_json::to_string(&entries)
            .unwrap()
            .contains("top-secret"));

        upsert_input_entry_core(&state, "computer-a", "credential", None, false)
            .await
            .unwrap();
        assert_eq!(
            keychain::get_input_secret(secrets.as_ref(), "computer-a", "credential").unwrap(),
            None
        );
        assert_eq!(
            list_input_entries_core(&state, "computer-a").unwrap()[0].value,
            Some(serde_json::json!("top-secret"))
        );
    }
}
