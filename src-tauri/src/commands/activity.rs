use crate::services::observability::{ActivityPage, ActivityQuery, ActivityScopeFilter};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_activity(
    state: State<'_, AppState>,
    query: Option<ActivityQuery>,
) -> Result<ActivityPage, String> {
    let query = query.unwrap_or_default();
    state.observability.query_activity_async(query).await
}

#[tauri::command]
pub async fn export_activity(
    state: State<'_, AppState>,
    path: String,
    query: Option<ActivityQuery>,
) -> Result<(), String> {
    let service = state.observability.as_ref().clone();
    let query = query.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let json = service.export_activity(&query)?;
        std::fs::write(path, json).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn clear_activity(
    state: State<'_, AppState>,
    scope: Option<ActivityScopeFilter>,
) -> Result<u64, String> {
    let service = state.observability.as_ref().clone();
    let scope = scope.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || service.clear_activity(&scope))
        .await
        .map_err(|error| error.to_string())?
}
