use crate::services::observability::DiagnosticLevel;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn set_diagnostic_log_level(
    state: State<'_, AppState>,
    level: DiagnosticLevel,
) -> Result<(), String> {
    state.diagnostics.set_level(level);
    Ok(())
}
