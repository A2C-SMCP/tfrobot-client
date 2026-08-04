use crate::services::config::ConfigError;
use crate::AppState;
use a2c_smcp::smcp_computer::skills::SkillResourceView;
use a2c_smcp::A2CSkillRef;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

const MAX_INLINE_TEXT_BODY_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillResourceResponse {
    pub name: String,
    pub rel_path: String,
    pub mime_type: String,
    pub total_size: u64,
    pub sha256: String,
    pub is_entry: bool,
    pub is_text: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum SkillCommandError {
    InvalidRequest {
        message: String,
    },
    InstanceNotFound {
        instance_id: String,
    },
    RuntimeUnavailable {
        message: String,
    },
    ConfigurationUnavailable {
        message: String,
    },
    SkillNotFound {
        name: String,
    },
    ResourceNotAccessible {
        reason: String,
        rel_path: String,
        message: String,
    },
    ResourceReadFailed {
        rel_path: String,
        message: String,
    },
    InvalidUtf8 {
        rel_path: String,
        message: String,
    },
    OpenFailed {
        path: String,
        message: String,
    },
}

#[tauri::command]
pub async fn list_skills(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<A2CSkillRef>, SkillCommandError> {
    list_skills_core(&state, &instance_id).await
}

pub async fn list_skills_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<A2CSkillRef>, SkillCommandError> {
    let runtime = runtime_for_instance(state, instance_id).await?;
    let reader = runtime
        .sdk_skill_reader()
        .map_err(|message| SkillCommandError::RuntimeUnavailable { message })?;
    Ok(reader.skills().await)
}

#[tauri::command]
pub async fn get_skill(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
    rel_path: Option<String>,
) -> Result<SkillResourceResponse, SkillCommandError> {
    get_skill_core(&state, &instance_id, &name, rel_path.as_deref()).await
}

pub async fn get_skill_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
    rel_path: Option<&str>,
) -> Result<SkillResourceResponse, SkillCommandError> {
    let runtime = runtime_for_instance(state, instance_id).await?;
    let reader = runtime
        .sdk_skill_reader()
        .map_err(|message| SkillCommandError::RuntimeUnavailable { message })?;
    let name = require_non_empty("skill name", name)?;
    let skill_ref =
        reader
            .skill_ref(name)
            .await
            .ok_or_else(|| SkillCommandError::SkillNotFound {
                name: name.to_string(),
            })?;
    let view = reader
        .read_skill_resource(&skill_ref, rel_path)
        .await
        .map_err(|error| SkillCommandError::ResourceNotAccessible {
            reason: error.reason.to_string(),
            rel_path: error.rel_path.clone(),
            message: format!(
                "Skill resource not accessible: reason={}, rel_path={}",
                error.reason, error.rel_path
            ),
        })?;

    skill_resource_response(name, view)
}

#[tauri::command]
pub async fn refresh_skills(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), SkillCommandError> {
    refresh_skills_core(&state, &instance_id).await
}

pub async fn refresh_skills_core(
    state: &AppState,
    instance_id: &str,
) -> Result<(), SkillCommandError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let runtime = runtime_for_instance(state, instance_id).await?;
    runtime.mark_sdk_skills_dirty().await;
    Ok(())
}

#[tauri::command]
pub async fn open_local_skills_root(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), SkillCommandError> {
    open_local_skills_root_core(&state, &instance_id, |path| {
        app.opener()
            .open_path(path.to_string_lossy().to_string(), None::<&str>)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn open_configured_local_skills_root(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), SkillCommandError> {
    open_configured_local_skills_root_core(&state, &instance_id, |path| {
        app.opener()
            .open_path(path.to_string_lossy().to_string(), None::<&str>)
            .map_err(|error| error.to_string())
    })
}

pub fn open_configured_local_skills_root_core<F>(
    state: &AppState,
    instance_id: &str,
    open_path: F,
) -> Result<(), SkillCommandError>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let root = configured_local_user_skills_root(state, instance_id)?;
    std::fs::create_dir_all(&root).map_err(|error| SkillCommandError::OpenFailed {
        path: root.to_string_lossy().to_string(),
        message: format!("Failed to create configured local skills root: {error}"),
    })?;
    open_path(&root).map_err(|message| SkillCommandError::OpenFailed {
        path: root.to_string_lossy().to_string(),
        message,
    })
}

pub fn configured_local_user_skills_root(
    state: &AppState,
    instance_id: &str,
) -> Result<PathBuf, SkillCommandError> {
    let instance_id = require_non_empty("instance_id", instance_id)?;
    let instance =
        state
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| match error {
                ConfigError::NotFound(_) => SkillCommandError::InstanceNotFound {
                    instance_id: instance_id.to_string(),
                },
                other => SkillCommandError::ConfigurationUnavailable {
                    message: other.to_string(),
                },
            })?;
    Ok(instance
        .local_skills_root
        .unwrap_or_else(|| state.config.default_local_skills_root(instance_id))
        .join("user"))
}

pub async fn open_local_skills_root_core<F>(
    state: &AppState,
    instance_id: &str,
    open_path: F,
) -> Result<(), SkillCommandError>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let root = local_user_skills_root(state, instance_id).await?;
    std::fs::create_dir_all(&root).map_err(|error| SkillCommandError::OpenFailed {
        path: root.to_string_lossy().to_string(),
        message: format!("Failed to create local skills root: {error}"),
    })?;
    open_path(&root).map_err(|message| SkillCommandError::OpenFailed {
        path: root.to_string_lossy().to_string(),
        message,
    })
}

pub async fn local_user_skills_root(
    state: &AppState,
    instance_id: &str,
) -> Result<PathBuf, SkillCommandError> {
    let runtime = runtime_for_instance(state, instance_id).await?;
    let reader = runtime
        .sdk_skill_reader()
        .map_err(|message| SkillCommandError::RuntimeUnavailable { message })?;
    Ok(reader.skill_home().await.join("user"))
}

async fn runtime_for_instance(
    state: &AppState,
    instance_id: &str,
) -> Result<crate::services::computer::ComputerInstanceRuntime, SkillCommandError> {
    let instance_id = require_non_empty("instance_id", instance_id)?;
    state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| SkillCommandError::InstanceNotFound {
            instance_id: instance_id.to_string(),
        })
}

fn skill_resource_response(
    name: &str,
    view: SkillResourceView,
) -> Result<SkillResourceResponse, SkillCommandError> {
    let body = if view.is_text && view.total_size <= MAX_INLINE_TEXT_BODY_BYTES {
        let bytes = view
            .read_all()
            .map_err(|error| SkillCommandError::ResourceReadFailed {
                rel_path: view.rel_path.clone(),
                message: format!("Failed to read skill resource {}: {}", view.rel_path, error),
            })?;
        Some(
            String::from_utf8(bytes).map_err(|error| SkillCommandError::InvalidUtf8 {
                rel_path: view.rel_path.clone(),
                message: format!(
                    "Skill resource {} is marked as text but is not valid UTF-8: {}",
                    view.rel_path, error
                ),
            })?,
        )
    } else {
        None
    };

    Ok(SkillResourceResponse {
        name: name.to_string(),
        rel_path: view.rel_path,
        mime_type: view.mime,
        total_size: view.total_size,
        sha256: view.sha256,
        is_entry: view.is_entry,
        is_text: view.is_text,
        body,
    })
}

fn require_non_empty<'a>(field: &str, value: &'a str) -> Result<&'a str, SkillCommandError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(SkillCommandError::InvalidRequest {
            message: format!("{field} is required"),
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::observability::ObservabilityService;
    use crate::services::settings::SettingsService;
    use tempfile::TempDir;

    fn test_state() -> (AppState, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let skill_dir = config
            .default_local_skills_root("computer-a")
            .join("user")
            .join("example-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: example-skill\ndescription: Example skill\n---\nBody\n",
        )
        .unwrap();
        let log_service = ObservabilityService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());
        (AppState::new(config, log_service, settings_service), dir)
    }

    fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
        let skill_dir = root.join("user").join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn list_skills_returns_sdk_refs_without_source_remap() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let skills = list_skills_core(&state, "computer-a").await.unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "example-skill")
            .expect("example skill should be listed");

        assert_eq!(skill.source, "user");
        assert!(skill.path.ends_with("example-skill"));
    }

    #[tokio::test]
    async fn list_and_get_skills_are_scoped_to_each_instance_skill_home() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        let custom_a = dir.path().join("custom-a-skill-home");
        let custom_b = dir.path().join("custom-b-skill-home");
        let mut computer_a = ComputerInstance::new("computer-a", "Computer A");
        computer_a.local_skills_root = Some(custom_a.clone());
        let mut computer_b = ComputerInstance::new("computer-b", "Computer B");
        computer_b.local_skills_root = Some(custom_b.clone());
        config.add_computer_instance(computer_a).unwrap();
        config.add_computer_instance(computer_b).unwrap();
        write_skill(&custom_a, "a-only", "A helper", "A body");
        write_skill(&custom_b, "b-only", "B helper", "B body");
        let log_service = ObservabilityService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());
        let state = AppState::new(config, log_service, settings_service);
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();
        state
            .computer_registry
            .start_runtime("computer-b")
            .await
            .unwrap();

        let skills_a = list_skills_core(&state, "computer-a").await.unwrap();
        let skills_b = list_skills_core(&state, "computer-b").await.unwrap();

        assert!(skills_a.iter().any(|skill| skill.name == "a-only"));
        assert!(skills_a.iter().all(|skill| skill.name != "b-only"));
        assert!(skills_b.iter().any(|skill| skill.name == "b-only"));
        assert!(skills_b.iter().all(|skill| skill.name != "a-only"));
        let body_a = get_skill_core(&state, "computer-a", "a-only", None)
            .await
            .unwrap();
        let body_b = get_skill_core(&state, "computer-b", "b-only", None)
            .await
            .unwrap();
        assert_eq!(body_a.body.as_deref(), Some("A body\n"));
        assert_eq!(body_b.body.as_deref(), Some("B body\n"));
        assert!(matches!(
            get_skill_core(&state, "computer-a", "b-only", None).await,
            Err(SkillCommandError::SkillNotFound { .. })
        ));
    }

    #[tokio::test]
    async fn get_skill_reads_entry_body_through_sdk_resource_api() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let response = get_skill_core(&state, "computer-a", "example-skill", None)
            .await
            .unwrap();

        assert_eq!(response.name, "example-skill");
        assert_eq!(response.rel_path, "SKILL.md");
        assert!(response.is_entry);
        assert!(response.is_text);
        assert_eq!(response.body.as_deref(), Some("Body\n"));
    }

    #[tokio::test]
    async fn get_skill_reads_relative_resource_through_sdk_resource_api() {
        let (state, _dir) = test_state();
        let docs_dir = state
            .config
            .default_local_skills_root("computer-a")
            .join("user")
            .join("example-skill")
            .join("docs");
        std::fs::create_dir_all(&docs_dir).unwrap();
        std::fs::write(docs_dir.join("usage.md"), "Usage\n").unwrap();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let response = get_skill_core(&state, "computer-a", "example-skill", Some("docs/usage.md"))
            .await
            .unwrap();

        assert_eq!(response.rel_path, "docs/usage.md");
        assert!(!response.is_entry);
        assert!(response.is_text);
        assert_eq!(response.body.as_deref(), Some("Usage\n"));
    }

    #[tokio::test]
    async fn get_skill_reports_sandbox_error_for_invalid_relative_resource() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let error = get_skill_core(&state, "computer-a", "example-skill", Some("../outside.md"))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            SkillCommandError::ResourceNotAccessible {
                reason: "traversal".to_string(),
                rel_path: "../outside.md".to_string(),
                message: "Skill resource not accessible: reason=traversal, rel_path=../outside.md"
                    .to_string(),
            }
        );
    }

    #[tokio::test]
    async fn get_skill_omits_large_text_body_from_inline_response() {
        let (state, _dir) = test_state();
        let docs_dir = state
            .config
            .default_local_skills_root("computer-a")
            .join("user")
            .join("example-skill")
            .join("docs");
        std::fs::create_dir_all(&docs_dir).unwrap();
        std::fs::write(
            docs_dir.join("large.md"),
            "x".repeat((MAX_INLINE_TEXT_BODY_BYTES + 1) as usize),
        )
        .unwrap();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let response = get_skill_core(&state, "computer-a", "example-skill", Some("docs/large.md"))
            .await
            .unwrap();

        assert_eq!(response.total_size, MAX_INLINE_TEXT_BODY_BYTES + 1);
        assert!(response.is_text);
        assert_eq!(response.body, None);
    }

    #[tokio::test]
    async fn get_skill_reports_missing_skill() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let error = get_skill_core(&state, "computer-a", "missing-skill", None)
            .await
            .unwrap_err();

        assert_eq!(
            error,
            SkillCommandError::SkillNotFound {
                name: "missing-skill".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn get_skill_preserves_empty_text_body() {
        let (state, _dir) = test_state();
        let docs_dir = state
            .config
            .default_local_skills_root("computer-a")
            .join("user")
            .join("example-skill")
            .join("docs");
        std::fs::create_dir_all(&docs_dir).unwrap();
        std::fs::write(docs_dir.join("empty.md"), "").unwrap();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        let response = get_skill_core(&state, "computer-a", "example-skill", Some("docs/empty.md"))
            .await
            .unwrap();

        assert_eq!(response.total_size, 0);
        assert!(response.is_text);
        assert_eq!(response.body.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn open_local_skills_root_creates_and_opens_user_source_root() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();
        let expected_root = state
            .computer_registry
            .runtime("computer-a")
            .await
            .unwrap()
            .sdk_skill_home()
            .await
            .join("user");
        std::fs::remove_dir_all(&expected_root).unwrap();

        let mut opened = None;
        open_local_skills_root_core(&state, "computer-a", |path| {
            opened = Some(path.to_path_buf());
            Ok(())
        })
        .await
        .unwrap();

        assert_eq!(opened.as_deref(), Some(expected_root.as_path()));
        assert!(expected_root.is_dir());
    }

    #[tokio::test]
    async fn open_local_skills_root_reports_missing_instance() {
        let (state, _dir) = test_state();

        let error = open_local_skills_root_core(&state, "missing", |_| Ok(()))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            SkillCommandError::InstanceNotFound {
                instance_id: "missing".to_string(),
            }
        );
    }

    #[test]
    fn open_configured_local_skills_root_uses_saved_root_without_runtime_restart() {
        let (state, dir) = test_state();
        let configured_root = dir.path().join("saved-skill-home");
        let runtime_root = state.config.default_local_skills_root("computer-a");
        state
            .config
            .update_computer_instance("computer-a", |instance| {
                instance.local_skills_root = Some(configured_root.clone());
            })
            .unwrap();

        let mut opened = None;
        open_configured_local_skills_root_core(&state, "computer-a", |path| {
            opened = Some(path.to_path_buf());
            Ok(())
        })
        .unwrap();

        let expected = configured_root.join("user");
        assert_eq!(opened.as_deref(), Some(expected.as_path()));
        assert!(expected.is_dir());
        assert_ne!(expected, runtime_root.join("user"));
    }

    #[tokio::test]
    async fn refresh_skills_marks_registry_dirty_without_client_scan() {
        let (state, _dir) = test_state();
        state
            .computer_registry
            .start_runtime("computer-a")
            .await
            .unwrap();

        refresh_skills_core(&state, "computer-a").await.unwrap();
    }
}
