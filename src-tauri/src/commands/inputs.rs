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
pub async fn list_inputs(state: State<'_, AppState>) -> Result<Vec<InputDefinition>, String> {
    state.config.load_inputs().map_err(|e| e.to_string())
}

/// Get a single input definition by ID
#[tauri::command]
pub async fn get_input(state: State<'_, AppState>, id: String) -> Result<Option<InputDefinition>, String> {
    let inputs = state.config.load_inputs().map_err(|e| e.to_string())?;
    Ok(inputs.into_iter().find(|i| i.id() == id))
}

/// Add or update an input variable definition
#[tauri::command]
pub async fn add_or_update_input(
    state: State<'_, AppState>,
    input: InputDefinition,
) -> Result<(), String> {
    let id = input.id().to_string();
    log::info!("Adding/updating input: {}", id);

    let mut inputs = state.config.load_inputs().map_err(|e| e.to_string())?;
    inputs.retain(|i| i.id() != id);
    inputs.push(input);
    state.config.save_inputs(&inputs).map_err(|e| e.to_string())?;

    Ok(())
}

/// Remove an input variable definition
#[tauri::command]
pub async fn remove_input(state: State<'_, AppState>, id: String) -> Result<(), String> {
    log::info!("Removing input: {}", id);

    let mut inputs = state.config.load_inputs().map_err(|e| e.to_string())?;
    let original_len = inputs.len();
    inputs.retain(|i| i.id() != id);

    if inputs.len() == original_len {
        return Err(format!("Input not found: {}", id));
    }

    state.config.save_inputs(&inputs).map_err(|e| e.to_string())?;

    // Also remove cached value
    let mut values = state.config.load_input_values().map_err(|e| e.to_string())?;
    values.remove(&id);
    state.config.save_input_values(&values).map_err(|e| e.to_string())?;

    Ok(())
}

/// List all cached input values
#[tauri::command]
pub async fn list_input_values(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    state.config.load_input_values().map_err(|e| e.to_string())
}

/// Get a single cached input value
#[tauri::command]
pub async fn get_input_value(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    let values = state.config.load_input_values().map_err(|e| e.to_string())?;
    Ok(values.get(&id).cloned())
}

/// Set a cached input value
#[tauri::command]
pub async fn set_input_value(
    state: State<'_, AppState>,
    id: String,
    value: serde_json::Value,
) -> Result<(), String> {
    log::info!("Setting input value: {}", id);

    let mut values = state.config.load_input_values().map_err(|e| e.to_string())?;
    values.insert(id, value);
    state.config.save_input_values(&values).map_err(|e| e.to_string())?;

    Ok(())
}

/// Remove a cached input value
#[tauri::command]
pub async fn remove_input_value(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut values = state.config.load_input_values().map_err(|e| e.to_string())?;
    values.remove(&id);
    state.config.save_input_values(&values).map_err(|e| e.to_string())?;
    Ok(())
}

/// Clear all cached input values
#[tauri::command]
pub async fn clear_input_values(state: State<'_, AppState>) -> Result<(), String> {
    state
        .config
        .save_input_values(&std::collections::HashMap::new())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Import input definitions from a JSON file
#[tauri::command]
pub async fn import_inputs(state: State<'_, AppState>, path: String) -> Result<usize, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let imported: Vec<InputDefinition> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let count = imported.len();

    let mut inputs = state.config.load_inputs().map_err(|e| e.to_string())?;
    for input in imported {
        let id = input.id().to_string();
        inputs.retain(|i| i.id() != id);
        inputs.push(input);
    }
    state.config.save_inputs(&inputs).map_err(|e| e.to_string())?;

    Ok(count)
}
