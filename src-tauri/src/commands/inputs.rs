use crate::services::keychain;
use crate::services::sdk_config::ensure_portable_cli_arguments;
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

    pub fn is_secret(&self) -> bool {
        matches!(
            self,
            InputDefinition::PromptString {
                password: Some(true),
                ..
            }
        )
    }
}

/// Applies the single client-owned portability boundary for input definitions.
/// Command arguments are checked before persistence and password defaults are always removed so
/// secret values can exist only in the Keychain namespace, never in definitions or API responses.
pub(crate) fn prepare_portable_input_definitions(
    inputs: &[InputDefinition],
) -> Result<Vec<InputDefinition>, String> {
    for input in inputs {
        if let InputDefinition::Command {
            id,
            args: Some(args),
            ..
        } = input
        {
            ensure_portable_cli_arguments(&format!("inputs.{id}.args"), args)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(inputs
        .iter()
        .cloned()
        .map(|mut input| {
            if let InputDefinition::PromptString {
                default, password, ..
            } = &mut input
            {
                if *password == Some(true) {
                    *default = None;
                }
            }
            input
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputValueView {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct StoredInputSnapshot {
    id: String,
    value: Option<serde_json::Value>,
    secret: Option<String>,
}

pub(crate) struct InputDefinitionsConfigSnapshot {
    definitions: Vec<InputDefinition>,
    stored_values: Vec<StoredInputSnapshot>,
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
    let inputs = state
        .config
        .load_inputs_for_instance(require_instance_id(instance_id)?)
        .map_err(|e| e.to_string())?;
    prepare_portable_input_definitions(&inputs)
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
    let inputs = state
        .config
        .load_inputs_for_instance(require_instance_id(instance_id)?)
        .map_err(|e| e.to_string())?;
    Ok(prepare_portable_input_definitions(&inputs)?
        .into_iter()
        .find(|i| i.id() == id))
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
    let previous_definition = inputs.iter().find(|item| item.id() == id).cloned();
    let previous_value = snapshot_input_storage(state, instance_id, &id)?;
    inputs.retain(|i| i.id() != id);
    inputs.push(input);
    let inputs = prepare_portable_input_definitions(&inputs)?;
    state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    let new_definition = inputs.iter().find(|item| item.id() == id);
    if let Err(error) = reconcile_definition_storage(
        state.secret_store.as_ref(),
        instance_id,
        previous_definition.as_ref(),
        new_definition,
    ) {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            std::slice::from_ref(&previous_value),
            error,
        )
        .await);
    }
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            &[previous_value],
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
    let previous_value = snapshot_input_storage(state, instance_id, id)?;
    let original_len = inputs.len();
    inputs.retain(|i| i.id() != id);

    if inputs.len() == original_len {
        return Err(format!("Input not found: {}", id));
    }

    let inputs = prepare_portable_input_definitions(&inputs)?;
    state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    if let Err(error) = delete_input_storage(state, instance_id, id) {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            std::slice::from_ref(&previous_value),
            error.to_string(),
        )
        .await);
    }
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            &[previous_value],
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
) -> Result<std::collections::HashMap<String, InputValueView>, String> {
    list_input_values_core(&state, require_instance_id(&instance_id)?)
}

/// Get a single cached input value
#[tauri::command]
pub async fn get_input_value(
    state: State<'_, AppState>,
    instance_id: String,
    id: String,
) -> Result<Option<InputValueView>, String> {
    let instance_id = require_instance_id(&instance_id)?;
    require_existing_instance(&state, instance_id)?;
    let definition = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|input| input.id() == id)
        .ok_or_else(|| format!("Input not found: {id}"))?;
    input_value_view(state.secret_store.as_ref(), instance_id, &definition)
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
    let definition = inputs
        .iter()
        .find(|input| input.id() == id)
        .ok_or_else(|| format!("Input not found: {id}"))?;
    let previous_value = snapshot_input_storage(state, instance_id, &id)?;
    let mutation = if definition.is_secret() {
        let secret = value
            .as_str()
            .ok_or_else(|| format!("Secret input '{id}' must be a string"))?;
        keychain::set_input_secret(state.secret_store.as_ref(), instance_id, &id, secret).and_then(
            |_| keychain::delete_input_value(state.secret_store.as_ref(), instance_id, &id),
        )
    } else {
        keychain::set_input_value(state.secret_store.as_ref(), instance_id, &id, &value).and_then(
            |_| keychain::delete_input_secret(state.secret_store.as_ref(), instance_id, &id),
        )
    };
    if let Err(error) = mutation {
        let primary_error = error.to_string();
        return match restore_input_storage(state, instance_id, &previous_value) {
            Ok(()) => Err(format!(
                "Failed to store input value; changes were reverted: {primary_error}"
            )),
            Err(rollback_error) => Err(format!(
                "Failed to store input value: {primary_error}; rollback also failed: {rollback_error}"
            )),
        };
    }
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
        return Err(
            rollback_input_mutation(state, instance_id, None, &[previous_value], error).await,
        );
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
    let previous_value = snapshot_input_storage(state, instance_id, id)?;
    if let Err(error) = delete_input_storage(state, instance_id, id) {
        return Err(
            rollback_input_mutation(state, instance_id, None, &[previous_value], error).await,
        );
    }
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
        return Err(
            rollback_input_mutation(state, instance_id, None, &[previous_value], error).await,
        );
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
        .map(|input| snapshot_input_storage(state, instance_id, input.id()))
        .collect::<Result<Vec<_>, _>>()?;
    for input in &inputs {
        if let Err(error) = delete_input_storage(state, instance_id, input.id()) {
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
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
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
    import_inputs_core(&state, &instance_id, &path).await
}

pub async fn import_inputs_core(
    state: &AppState,
    instance_id: &str,
    path: &str,
) -> Result<usize, String> {
    let instance_id = require_instance_id(instance_id)?;
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    require_existing_instance(state, instance_id)?;
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let imported: Vec<InputDefinition> =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let imported = prepare_portable_input_definitions(&imported)?;
    let count = imported.len();

    let mut inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let previous_inputs = inputs.clone();
    let imported_ids = imported
        .iter()
        .map(|input| input.id().to_string())
        .collect::<std::collections::HashSet<_>>();
    let previous_values = imported_ids
        .iter()
        .map(|id| snapshot_input_storage(state, instance_id, id))
        .collect::<Result<Vec<_>, _>>()?;
    for input in imported {
        let id = input.id().to_string();
        inputs.retain(|i| i.id() != id);
        inputs.push(input);
    }
    let inputs = prepare_portable_input_definitions(&inputs)?;
    state
        .config
        .save_inputs_for_instance(instance_id, &inputs)
        .map_err(|e| e.to_string())?;
    for id in &imported_ids {
        let previous_definition = previous_inputs.iter().find(|input| input.id() == id);
        let new_definition = inputs.iter().find(|input| input.id() == id);
        if let Err(error) = reconcile_definition_storage(
            state.secret_store.as_ref(),
            instance_id,
            previous_definition,
            new_definition,
        ) {
            return Err(rollback_input_mutation(
                state,
                instance_id,
                Some(&previous_inputs),
                &previous_values,
                error,
            )
            .await);
        }
    }
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
        return Err(rollback_input_mutation(
            state,
            instance_id,
            Some(&previous_inputs),
            &previous_values,
            error,
        )
        .await);
    }

    Ok(count)
}

async fn sync_computer_runtime(state: &AppState, instance_id: &str) -> Result<(), String> {
    sync_computer_runtime_with_parts(
        state.config.as_ref(),
        state.computer_registry.as_ref(),
        instance_id,
    )
    .await
}

async fn sync_computer_runtime_with_parts(
    config: &crate::services::config::ConfigService,
    registry: &crate::services::computer::ComputerRegistry,
    instance_id: &str,
) -> Result<(), String> {
    let instance = config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    registry.update_runtime_instance(instance).await.map(|_| ())
}

/// Replaces client-owned input definitions without rebuilding or reloading any runtime.
///
/// Configuration import uses this boundary so definition persistence remains independent from
/// runtime availability. The returned snapshot can roll the change back if the paired SDK config
/// mutation fails.
pub(crate) fn replace_input_definitions_config_only_locked(
    config: &crate::services::config::ConfigService,
    secret_store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    definitions: &[InputDefinition],
) -> Result<InputDefinitionsConfigSnapshot, InputDefinitionsConfigMutationError> {
    let definitions = prepare_portable_input_definitions(definitions)
        .map_err(InputDefinitionsConfigMutationError::Unchanged)?;
    let previous = config
        .load_inputs_for_instance(instance_id)
        .map_err(|error| InputDefinitionsConfigMutationError::Unchanged(error.to_string()))?;
    let affected_ids = previous
        .iter()
        .chain(definitions.iter())
        .map(|definition| definition.id().to_string())
        .collect::<std::collections::HashSet<_>>();
    let stored_values = affected_ids
        .iter()
        .map(|id| snapshot_input_storage_with_store(secret_store, instance_id, id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(InputDefinitionsConfigMutationError::Unchanged)?;
    let snapshot = InputDefinitionsConfigSnapshot {
        definitions: previous,
        stored_values,
    };

    if let Err(primary_error) = config.save_inputs_for_instance(instance_id, &definitions) {
        return Err(
            match restore_input_definitions_config_only_locked(
                config,
                secret_store,
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
    let reconcile_result = affected_ids.iter().try_for_each(|id| {
        reconcile_definition_storage(
            secret_store,
            instance_id,
            snapshot
                .definitions
                .iter()
                .find(|definition| definition.id() == id),
            definitions.iter().find(|definition| definition.id() == id),
        )
    });
    if let Err(primary_error) = reconcile_result {
        let rollback_error = restore_input_definitions_config_only_locked(
            config,
            secret_store,
            instance_id,
            &snapshot,
        )
        .err();
        return Err(match rollback_error {
            Some(rollback_error) => InputDefinitionsConfigMutationError::OutcomeUncertain(
                format!(
                    "Failed to update input definition storage: {primary_error}; rollback also failed: {rollback_error}"
                ),
            ),
            None => InputDefinitionsConfigMutationError::Reverted(format!(
                "Failed to update input definition storage; changes were reverted: {primary_error}"
            )),
        });
    }
    Ok(snapshot)
}

pub(crate) fn restore_input_definitions_config_only_locked(
    config: &crate::services::config::ConfigService,
    secret_store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    snapshot: &InputDefinitionsConfigSnapshot,
) -> Result<(), String> {
    let mut errors = Vec::new();
    let definitions = prepare_portable_input_definitions(&snapshot.definitions)?;
    if let Err(error) = config.save_inputs_for_instance(instance_id, &definitions) {
        errors.push(format!("restore input definitions: {error}"));
    }
    for stored in &snapshot.stored_values {
        if let Err(error) = restore_input_storage_with_store(secret_store, instance_id, stored) {
            errors.push(format!("restore input '{}': {error}", stored.id));
        }
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
    previous_inputs: Option<&[InputDefinition]>,
    previous_values: &[StoredInputSnapshot],
    primary_error: String,
) -> String {
    let mut rollback_errors = Vec::new();
    if let Some(inputs) = previous_inputs {
        match prepare_portable_input_definitions(inputs) {
            Ok(inputs) => {
                if let Err(error) = state.config.save_inputs_for_instance(instance_id, &inputs) {
                    rollback_errors.push(format!("restore Computer input definitions: {error}"));
                }
            }
            Err(error) => {
                rollback_errors.push(format!("sanitize Computer input definitions: {error}"));
            }
        }
    }
    for snapshot in previous_values {
        let result = restore_input_storage(state, instance_id, snapshot);
        if let Err(error) = result {
            rollback_errors.push(format!("restore Keychain input '{}': {error}", snapshot.id));
        }
    }
    if let Err(error) = sync_computer_runtime(state, instance_id).await {
        rollback_errors.push(format!("restore Computer runtimes: {error}"));
    }
    if rollback_errors.is_empty() {
        format!(
            "Failed to synchronize Computer input mutation; changes were reverted: {primary_error}"
        )
    } else {
        format!(
            "Failed to synchronize Computer input mutation: {primary_error}; rollback also failed: {}",
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
    for input in state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|error| error.to_string())?
    {
        if let Some(value) = input_value_view(state.secret_store.as_ref(), instance_id, &input)? {
            values.insert(input.id().to_string(), value);
        }
    }
    Ok(values)
}

fn input_value_view(
    store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    definition: &InputDefinition,
) -> Result<Option<InputValueView>, String> {
    if definition.is_secret() {
        let configured = keychain::get_input_secret(store, instance_id, definition.id())
            .map_err(|error| error.to_string())?
            .is_some();
        return Ok(configured.then_some(InputValueView {
            configured: true,
            value: None,
        }));
    }

    Ok(
        keychain::get_input_value(store, instance_id, definition.id())
            .map_err(|error| error.to_string())?
            .map(|value| InputValueView {
                configured: true,
                value: Some(value),
            }),
    )
}

fn snapshot_input_storage(
    state: &AppState,
    instance_id: &str,
    id: &str,
) -> Result<StoredInputSnapshot, String> {
    snapshot_input_storage_with_store(state.secret_store.as_ref(), instance_id, id)
}

fn snapshot_input_storage_with_store(
    store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    id: &str,
) -> Result<StoredInputSnapshot, String> {
    Ok(StoredInputSnapshot {
        id: id.to_string(),
        value: keychain::get_input_value(store, instance_id, id)
            .map_err(|error| error.to_string())?,
        secret: keychain::get_input_secret(store, instance_id, id)
            .map_err(|error| error.to_string())?,
    })
}

fn delete_input_storage(state: &AppState, instance_id: &str, id: &str) -> Result<(), String> {
    keychain::delete_input_value(state.secret_store.as_ref(), instance_id, id)
        .map_err(|error| error.to_string())?;
    keychain::delete_input_secret(state.secret_store.as_ref(), instance_id, id)
        .map_err(|error| error.to_string())
}

fn restore_input_storage(
    state: &AppState,
    instance_id: &str,
    snapshot: &StoredInputSnapshot,
) -> Result<(), String> {
    restore_input_storage_with_store(state.secret_store.as_ref(), instance_id, snapshot)
}

fn restore_input_storage_with_store(
    store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    snapshot: &StoredInputSnapshot,
) -> Result<(), String> {
    match &snapshot.value {
        Some(value) => keychain::set_input_value(store, instance_id, &snapshot.id, value),
        None => keychain::delete_input_value(store, instance_id, &snapshot.id),
    }
    .map_err(|error| error.to_string())?;
    match &snapshot.secret {
        Some(secret) => keychain::set_input_secret(store, instance_id, &snapshot.id, secret),
        None => keychain::delete_input_secret(store, instance_id, &snapshot.id),
    }
    .map_err(|error| error.to_string())
}

fn reconcile_definition_storage(
    store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
    previous: Option<&InputDefinition>,
    next: Option<&InputDefinition>,
) -> Result<(), String> {
    let Some(next) = next else {
        let Some(previous) = previous else {
            return Ok(());
        };
        keychain::delete_input_value(store, instance_id, previous.id())
            .map_err(|error| error.to_string())?;
        return keychain::delete_input_secret(store, instance_id, previous.id())
            .map_err(|error| error.to_string());
    };

    if next.is_secret() {
        return keychain::delete_input_value(store, instance_id, next.id())
            .map_err(|error| error.to_string());
    }

    if previous.is_some_and(InputDefinition::is_secret) {
        keychain::delete_input_value(store, instance_id, next.id())
            .map_err(|error| error.to_string())?;
    }
    keychain::delete_input_secret(store, instance_id, next.id()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::keychain::{self, InMemorySecretStore, KeychainError, SecretStore};
    use crate::services::logger::LogService;
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
            LogService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            store.clone(),
        );
        (state, store, dir)
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

    #[tokio::test]
    async fn password_input_uses_secret_namespace_and_list_never_returns_plaintext() {
        let (state, store, _dir) = test_state();
        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "api-key".to_string(),
                label: "API Key".to_string(),
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
            keychain::get_input_value(store.as_ref(), "computer-a", "api-key").unwrap(),
            None
        );
        let view = list_input_values_core(&state, "computer-a").unwrap();
        assert_eq!(
            view.get("api-key"),
            Some(&InputValueView {
                configured: true,
                value: None,
            })
        );
        assert!(!serde_json::to_string(&view).unwrap().contains("top-secret"));
    }

    #[tokio::test]
    async fn removing_and_clearing_password_inputs_delete_both_namespaces() {
        let (state, store, _dir) = test_state();
        for id in ["remove-me", "clear-me"] {
            add_or_update_input_core(
                &state,
                "computer-a",
                InputDefinition::PromptString {
                    id: id.to_string(),
                    label: id.to_string(),
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
            None
        );
        assert_eq!(
            keychain::get_input_value(store.as_ref(), "computer-a", "remove-me").unwrap(),
            None
        );

        clear_input_values_core(&state, "computer-a").await.unwrap();
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "clear-me").unwrap(),
            None
        );
        assert_eq!(
            keychain::get_input_value(store.as_ref(), "computer-a", "clear-me").unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn changing_value_definition_to_secret_drops_value_and_requires_reentry() {
        let (state, store, _dir) = test_state();
        let value_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: "Credential".to_string(),
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
                label: "Credential".to_string(),
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
            keychain::get_input_value(store.as_ref(), "computer-a", "credential").unwrap(),
            None
        );

        add_or_update_input_core(&state, "computer-a", value_definition.clone())
            .await
            .unwrap();
        keychain::set_input_value(
            store.as_ref(),
            "computer-a",
            "credential",
            &serde_json::json!({"nested": true}),
        )
        .unwrap();
        add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "credential".to_string(),
                label: "Credential".to_string(),
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
            keychain::get_input_value(store.as_ref(), "computer-a", "credential").unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn changing_secret_definition_to_value_drops_secret_and_requires_reentry() {
        let (state, store, _dir) = test_state();
        let secret_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: "Credential".to_string(),
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
                label: "Credential".to_string(),
                description: None,
                default: None,
                password: Some(false),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "credential").unwrap(),
            None
        );
        assert_eq!(
            keychain::get_input_value(store.as_ref(), "computer-a", "credential").unwrap(),
            None
        );
        assert!(!list_input_values_core(&state, "computer-a")
            .unwrap()
            .contains_key("credential"));
    }

    #[tokio::test]
    async fn definition_type_change_rolls_back_definition_and_both_namespaces_on_failure() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let store = Arc::new(FailOnceDeleteSecretStore::default());
        let state = AppState::new_with_secret_store(
            config,
            LogService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            store.clone(),
        );
        let secret_definition = InputDefinition::PromptString {
            id: "credential".to_string(),
            label: "Credential".to_string(),
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

        let error = add_or_update_input_core(
            &state,
            "computer-a",
            InputDefinition::PromptString {
                id: "credential".to_string(),
                label: "Credential".to_string(),
                description: None,
                default: None,
                password: Some(false),
            },
        )
        .await
        .unwrap_err();

        assert!(error.contains("changes were reverted"));
        assert_eq!(
            state.config.load_inputs_for_instance("computer-a").unwrap(),
            vec![secret_definition]
        );
        assert_eq!(
            keychain::get_input_secret(store.as_ref(), "computer-a", "credential")
                .unwrap()
                .as_deref(),
            Some("top-secret")
        );
        assert_eq!(
            keychain::get_input_value(store.as_ref(), "computer-a", "credential").unwrap(),
            None
        );
    }
}
