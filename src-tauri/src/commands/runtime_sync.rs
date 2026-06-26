use crate::services::computer::{ComputerInstance, ComputerInstanceRuntime};
use crate::AppState;

pub async fn apply_updated_computer_instance(
    state: &AppState,
    previous: ComputerInstance,
    updated: ComputerInstance,
) -> Result<ComputerInstanceRuntime, String> {
    match state
        .computer_registry
        .update_runtime_instance(updated)
        .await
    {
        Ok(runtime) => Ok(runtime),
        Err(error) => {
            let restored = state
                .config
                .update_computer_instance(&previous.id, |instance| {
                    *instance = previous.clone();
                })
                .map_err(|restore_error| {
                    format!(
                        "Failed to apply Computer config to runtime: {error}; additionally failed to restore persisted config: {restore_error}"
                    )
                })?;
            if let Err(restore_runtime_error) = state
                .computer_registry
                .update_runtime_instance(restored)
                .await
            {
                return Err(format!(
                    "Failed to apply Computer config to runtime: {error}; persisted config was restored, but runtime restore failed: {restore_runtime_error}"
                ));
            }
            Err(format!(
                "Failed to apply Computer config to runtime; reverted persisted config: {error}"
            ))
        }
    }
}
