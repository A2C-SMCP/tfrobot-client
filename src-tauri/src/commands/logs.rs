use crate::services::logger::LogFilter;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_logs(
    state: State<'_, AppState>,
    filter: Option<LogFilter>,
) -> Result<Vec<crate::services::logger::LogEntry>, String> {
    let f = filter.unwrap_or_default();
    state.log_service.query(&f)
}

#[tauri::command]
pub async fn export_logs(
    state: State<'_, AppState>,
    path: String,
    filter: Option<LogFilter>,
) -> Result<(), String> {
    let f = filter.unwrap_or_default();
    let json = state.log_service.export(&f)?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn clear_logs(
    state: State<'_, AppState>,
    before_days: Option<i64>,
) -> Result<u64, String> {
    match before_days {
        Some(days) => state.log_service.cleanup(days),
        None => {
            state.log_service.clear_all()?;
            Ok(0)
        }
    }
}
