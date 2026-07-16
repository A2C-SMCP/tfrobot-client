use crate::services::keychain;
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Input variable definition for the frontend
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    let id = input.id().to_string();
    log::info!("Adding/updating input for instance {}: {}", instance_id, id);
    require_existing_instance(state, instance_id)?;

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let previous_inputs = inputs.clone();
    inputs.retain(|i| i.id() != id);
    inputs.push(input);
    state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    if let Err(error) = sync_all_computer_runtimes(state).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            &[],
            error,
        )
        .await);
    }

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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    log::info!("Removing input for instance {}: {}", instance_id, id);
    require_existing_instance(state, instance_id)?;

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let previous_inputs = inputs.clone();
    let previous_value =
        keychain::get_input_value(state.secret_store.as_ref(), id).map_err(|e| e.to_string())?;
    let original_len = inputs.len();
    inputs.retain(|i| i.id() != id);

    if inputs.len() == original_len {
        return Err(format!("Input not found: {}", id));
    }

    state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    if let Err(error) = keychain::delete_input_value(state.secret_store.as_ref(), id) {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            &[(id.to_string(), previous_value)],
            error.to_string(),
        )
        .await);
    }
    if let Err(error) = sync_all_computer_runtimes(state).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            &[(id.to_string(), previous_value)],
            error,
        )
        .await);
    }

    Ok(())
}

/// List all cached input values
#[tauri::command]
pub async fn list_input_values(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    list_input_values_core(&state, require_instance_id(&instance_id)?)
}

/// Get a single cached input value
#[tauri::command]
pub async fn get_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    let instance_id = require_instance_id(&instance_id)?;
    require_existing_instance(&state, instance_id)?;
    keychain::get_input_value(state.secret_store.as_ref(), &id).map_err(|e| e.to_string())
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    log::info!("Setting input value: {}", id);
    require_existing_instance(state, instance_id)?;
    let inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    if !inputs.iter().any(|input| input.id() == id) {
        return Err(format!("Input not found: {id}"));
    }
    let previous_value =
        keychain::get_input_value(state.secret_store.as_ref(), &id).map_err(|e| e.to_string())?;
    keychain::set_input_value(state.secret_store.as_ref(), &id, &value)
        .map_err(|e| e.to_string())?;
    if let Err(error) = sync_all_computer_runtimes(state).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            None,
            &[(id, previous_value)],
            error,
        )
        .await);
    }

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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    require_existing_instance(state, instance_id)?;
    let previous_value =
        keychain::get_input_value(state.secret_store.as_ref(), id).map_err(|e| e.to_string())?;
    keychain::delete_input_value(state.secret_store.as_ref(), id).map_err(|e| e.to_string())?;
    if let Err(error) = sync_all_computer_runtimes(state).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            None,
            &[(id.to_string(), previous_value)],
            error,
        )
        .await);
    }
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    require_existing_instance(state, instance_id)?;
    let inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let previous_values = inputs
        .iter()
        .map(|input| {
            keychain::get_input_value(state.secret_store.as_ref(), input.id())
                .map(|value| (input.id().to_string(), value))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for input in &inputs {
        if let Err(error) = keychain::delete_input_value(state.secret_store.as_ref(), input.id()) {
            return Err(rollback_input_mutation(
                state,
                instance_id,
                None,
                &previous_values,
                error.to_string(),
            )
            .await);
        }
    }
    if let Err(error) = sync_all_computer_runtimes(state).await {
        return Err(
            rollback_input_mutation(state, instance_id, None, &previous_values, error).await,
        );
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
    let instance_id = require_instance_id(&instance_id)?;
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    require_existing_instance(&state, instance_id)?;
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let imported: Vec<InputDefinition> =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let count = imported.len();

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let previous_inputs = inputs.clone();
    for input in imported {
        let id = input.id().to_string();
        inputs.retain(|i| i.id() != id);
        inputs.push(input);
    }
    state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    if let Err(error) = sync_all_computer_runtimes(&state).await {
        return Err(rollback_input_mutation(
            &state,
            instance_id,
            Some(&previous_inputs),
            &[],
            error,
        )
        .await);
    }

    Ok(count)
}

async fn sync_all_computer_runtimes(state: &AppState) -> Result<(), String> {
    sync_all_computer_runtimes_with_parts(
        state.config.as_ref(),
        state.computer_registry.as_ref(),
        state.secret_store.as_ref(),
    )
    .await
}

async fn sync_all_computer_runtimes_with_parts(
    config: &crate::services::config::ConfigService,
    registry: &crate::services::computer::ComputerRegistry,
    secret_store: &dyn crate::services::keychain::SecretStore,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for mut instance in config
        .load_computer_instances()
        .map_err(|error| error.to_string())?
        .instances
    {
        for input in &instance.inputs {
            if let Some(value) = keychain::get_input_value(secret_store, input.id())
                .map_err(|error| error.to_string())?
            {
                instance.input_values.insert(input.id().to_string(), value);
            }
        }
        if let Err(error) = registry.update_runtime_instance(instance).await {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub(crate) async fn replace_global_input_definitions_with_parts_locked(
    config: &crate::services::config::ConfigService,
    registry: &crate::services::computer::ComputerRegistry,
    secret_store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    definitions: &[InputDefinition],
) -> Result<(), String> {
    let previous = config
        .load_inputs_for_instance(instance_id)
        .map_err(|error| error.to_string())?;
    config
        .save_inputs_for_instance(instance_id, definitions)
        .map_err(|error| error.to_string())?;
    if let Err(primary_error) =
        sync_all_computer_runtimes_with_parts(config, registry, secret_store).await
    {
        let mut rollback_errors = Vec::new();
        if let Err(error) = config.save_inputs_for_instance(instance_id, &previous) {
            rollback_errors.push(format!("restore global input definitions: {error}"));
        }
        if let Err(error) =
            sync_all_computer_runtimes_with_parts(config, registry, secret_store).await
        {
            rollback_errors.push(format!("restore Computer runtimes: {error}"));
        }
        return if rollback_errors.is_empty() {
            Err(format!(
                "Failed to synchronize global input definitions; changes were reverted: {primary_error}"
            ))
        } else {
            Err(format!(
                "Failed to synchronize global input definitions: {primary_error}; rollback also failed: {}",
                rollback_errors.join("; ")
            ))
        };
    }
    Ok(())
}

async fn rollback_input_mutation(
    state: &AppState,
    instance_id: &str,
    previous_inputs: Option<&[InputDefinition]>,
    previous_values: &[(String, Option<serde_json::Value>)],
    primary_error: String,
) -> String {
    let mut rollback_errors = Vec::new();
    if let Some(inputs) = previous_inputs {
        if let Err(error) = state.config.save_inputs_for_instance(instance_id, inputs) {
            rollback_errors.push(format!("restore global input definitions: {error}"));
        }
    }
    for (id, value) in previous_values {
        let result = match value {
            Some(value) => keychain::set_input_value(state.secret_store.as_ref(), id, value),
            None => keychain::delete_input_value(state.secret_store.as_ref(), id),
        };
        if let Err(error) = result {
            rollback_errors.push(format!("restore Keychain input '{id}': {error}"));
        }
    }
    if let Err(error) = sync_all_computer_runtimes(state).await {
        rollback_errors.push(format!("restore Computer runtimes: {error}"));
    }
    if rollback_errors.is_empty() {
        format!(
            "Failed to synchronize global input mutation; changes were reverted: {primary_error}"
        )
    } else {
        format!(
            "Failed to synchronize global input mutation: {primary_error}; rollback also failed: {}",
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
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    require_existing_instance(state, instance_id)?;
    let mut values = std::collections::HashMap::new();
    for input in state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|error| error.to_string())?
    {
        if let Some(value) = keychain::get_input_value(state.secret_store.as_ref(), input.id())
            .map_err(|error| error.to_string())?
        {
            values.insert(input.id().to_string(), value);
        }
    }
    Ok(values)
}
