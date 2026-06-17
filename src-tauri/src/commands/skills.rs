use crate::services::skill_manager::SkillSyncSummary;
use crate::services::skills::{self, SkillInfo};
use crate::AppState;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use tauri_plugin_shell::ShellExt;

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<SkillInfo>, String> {
    let settings = state.settings_service.load();
    let mut skill_infos =
        skills::list_skills(&settings.skills_root_dir).map_err(|e| e.to_string())?;

    let mut mcp_infos: Vec<SkillInfo> = state
        .runtime
        .computer()
        .get_skills()
        .await
        .into_iter()
        .filter(|skill_ref| skill_ref.source.starts_with("mcp:"))
        .map(|skill_ref| skills::skill_info_from_ref(&skill_ref))
        .collect();

    skill_infos.append(&mut mcp_infos);
    skill_infos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(skill_infos)
}

#[tauri::command]
pub async fn get_skill_sync_summary(
    state: State<'_, AppState>,
) -> Result<SkillSyncSummary, String> {
    Ok(state.runtime.skill_sync_summary().await)
}

#[tauri::command]
pub async fn refresh_skill_sync_summary(
    state: State<'_, AppState>,
) -> Result<SkillSyncSummary, String> {
    let settings = state.settings_service.load();
    state.runtime.refresh_skill_sync_summary(&settings).await
}

#[tauri::command]
pub async fn read_skill_markdown(
    state: State<'_, AppState>,
    skill_path: String,
) -> Result<String, String> {
    let settings = state.settings_service.load();
    if is_active_mcp_skill_path(&state, &skill_path).await {
        return skills::read_skill_markdown_file(&skill_path).map_err(|e| e.to_string());
    }
    skills::read_skill_markdown(&settings.skills_root_dir, &skill_path).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(deprecated)]
pub async fn open_skills_root(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings_service.load();
    let root = skills::ensure_skills_root(&settings.skills_root_dir).map_err(|e| e.to_string())?;
    let root_path = root.to_string_lossy().to_string();
    app.shell()
        .open(root_path.clone(), None)
        .map_err(|e| e.to_string())?;
    Ok(root_path)
}

#[tauri::command]
#[allow(deprecated)]
pub async fn open_skill_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    skill_path: String,
) -> Result<String, String> {
    let settings = state.settings_service.load();
    let skill_folder = skills::open_skill_folder(&settings.skills_root_dir, &skill_path)
        .map_err(|e| e.to_string())?;
    let folder_path = skill_folder.to_string_lossy().to_string();
    app.shell()
        .open(folder_path.clone(), None)
        .map_err(|e| e.to_string())?;
    Ok(folder_path)
}

#[tauri::command]
#[allow(deprecated)]
pub async fn open_skill_markdown_file(
    app: AppHandle,
    state: State<'_, AppState>,
    skill_path: String,
) -> Result<String, String> {
    let settings = state.settings_service.load();
    let skill_md_path = skills::skill_markdown_path(&settings.skills_root_dir, &skill_path)
        .map_err(|e| e.to_string())?;
    let file_path = skill_md_path.to_string_lossy().to_string();
    app.shell()
        .open(file_path.clone(), None)
        .map_err(|e| e.to_string())?;
    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_info_serializes_frontend_contract_fields() {
        let info = SkillInfo {
            name: "demo".to_string(),
            path: "/tmp/skills/demo".to_string(),
            skill_md_path: "/tmp/skills/demo/SKILL.md".to_string(),
            has_skill_md: true,
            is_skill: true,
            invalid_reason: None,
            description: Some("Demo skill".to_string()),
            source: "local".to_string(),
            uri: None,
        };

        let value = serde_json::to_value(info).unwrap();

        assert_eq!(value["name"], "demo");
        assert_eq!(value["path"], "/tmp/skills/demo");
        assert_eq!(value["skill_md_path"], "/tmp/skills/demo/SKILL.md");
        assert_eq!(value["has_skill_md"], true);
        assert_eq!(value["is_skill"], true);
        assert_eq!(value["description"], "Demo skill");
        assert_eq!(value["source"], "local");
    }
}

async fn is_active_mcp_skill_path(state: &AppState, skill_path: &str) -> bool {
    let requested = PathBuf::from(skill_path);
    let requested_canonical = requested.canonicalize().ok();

    state
        .runtime
        .computer()
        .get_skills()
        .await
        .into_iter()
        .filter(|skill_ref| skill_ref.source.starts_with("mcp:"))
        .any(|skill_ref| {
            let active_path = Path::new(&skill_ref.path);
            if active_path == requested {
                return true;
            }
            match (&requested_canonical, active_path.canonicalize().ok()) {
                (Some(requested), Some(active)) => requested == &active,
                _ => false,
            }
        })
}
