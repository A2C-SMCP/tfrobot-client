use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Input variable definition for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputDefinition {
    PromptString {
        id: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<bool>,
    },
    PickString {
        id: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        options: Vec<PickOption>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Command {
        id: String,
        label: String,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickOption {
    pub label: String,
    pub value: String,
}

impl InputDefinition {
    pub fn id(&self) -> &str {
        match self {
            InputDefinition::PromptString { id, .. } => id,
            InputDefinition::PickString { id, .. } => id,
            InputDefinition::Command { id, .. } => id,
        }
    }
}

/// List all input variable definitions
#[tauri::command]
pub async fn list_inputs(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<InputDefinition>, String> {
    state
        .config
        .load_inputs_for_instance(require_instance_id(&instance_id)?)
        .map_err(|e| e.to_string())
}

/// Get a single input definition by ID
#[tauri::command]
pub async fn get_input(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<InputDefinition>, String> {
    let inputs = state
        .config
        .load_inputs_for_instance(require_instance_id(&instance_id)?)
        .map_err(|e| e.to_string())?;
    Ok(inputs.into_iter().find(|i| i.id() == id))
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
    let instance_id = require_instance_id(instance_id)?;
    let id = input.id().to_string();
    log::info!("Adding/updating input for instance {}: {}", instance_id, id);
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    inputs.retain(|i| i.id() != id);
    inputs.push(input);
    let updated_instance = state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    apply_updated_computer_instance(&state, previous, updated_instance).await?;

    Ok(())
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
    log::info!("Removing input for instance {}: {}", instance_id, id);
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let original_len = inputs.len();
    inputs.retain(|i| i.id() != id);

    if inputs.len() == original_len {
        return Err(format!("Input not found: {}", id));
    }

    let updated_instance = state
        .config
        .update_computer_instance(instance_id, |instance| {
            instance.inputs = inputs;
            instance.input_values.remove(id);
        })
        .map_err(|e| e.to_string())?;
    apply_updated_computer_instance(&state, previous, updated_instance).await?;

    Ok(())
}

/// List all cached input values
#[tauri::command]
pub async fn list_input_values(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    state
        .config
        .load_input_values_for_instance(require_instance_id(&instance_id)?)
        .map_err(|e| e.to_string())
}

/// Get a single cached input value
#[tauri::command]
pub async fn get_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    let values = state
        .config
        .load_input_values_for_instance(require_instance_id(&instance_id)?)
        .map_err(|e| e.to_string())?;
    Ok(values.get(&id).cloned())
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
    log::info!("Setting input value: {}", id);
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;

    let mut values = state
        .config
        .load_input_values_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    values.insert(id, value);
    let updated_instance = state
        .config
        .save_input_values_for_instance(instance_id, &values)
        .map_err(|e| e.to_string())?;
    apply_updated_computer_instance(&state, previous, updated_instance).await?;

    Ok(())
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
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let mut values = state
        .config
        .load_input_values_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    values.remove(id);
    let updated_instance = state
        .config
        .save_input_values_for_instance(instance_id, &values)
        .map_err(|e| e.to_string())?;
    apply_updated_computer_instance(state, previous, updated_instance).await?;
    Ok(())
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
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let updated_instance = state
        .config
        .save_input_values_for_instance(instance_id, &std::collections::HashMap::new())
        .map_err(|e| e.to_string())?;
    apply_updated_computer_instance(state, previous, updated_instance).await?;
    Ok(())
}

/// Import input definitions from a JSON file
#[tauri::command]
pub async fn import_inputs(
    state: State<'_, AppState>,
    instance_id: String,
    path: String,
) -> Result<usize, String> {
    let instance_id = require_instance_id(&instance_id)?;
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let imported: Vec<InputDefinition> =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let count = imported.len();

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    for input in imported {
        let id = input.id().to_string();
        inputs.retain(|i| i.id() != id);
        inputs.push(input);
    }
    let updated_instance = state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    apply_updated_computer_instance(&state, previous, updated_instance).await?;

    Ok(count)
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}
