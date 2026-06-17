use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use smcp::A2CSkillRef;
use smcp_computer::skills::{parse_skill_frontmatter, synthesize_user_name};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub path: String,
    pub skill_md_path: String,
    pub has_skill_md: bool,
    pub is_skill: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    pub description: Option<String>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LocalSkillCandidate {
    pub info: SkillInfo,
    pub canonical_path: Option<PathBuf>,
    pub protocol_name: Option<String>,
    pub full_description: Option<String>,
    pub frontmatter: Option<Map<String, Value>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillsError {
    #[error("Skills root directory does not exist: {0}")]
    RootNotFound(String),

    #[error("Skills root path is not a directory: {0}")]
    RootNotDirectory(String),

    #[error("Skill path is outside the configured skills root")]
    PathOutsideRoot,

    #[error("Skill path is not a directory: {0}")]
    SkillNotDirectory(String),

    #[error("SKILL.md not found: {0}")]
    SkillMarkdownNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(path))
    } else if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    }
}

pub fn runtime_skill_home(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("runtime-skill-home")
}

pub fn list_skills(root: &str) -> Result<Vec<SkillInfo>, SkillsError> {
    Ok(list_local_skill_candidates(root)?
        .into_iter()
        .filter(|candidate| candidate.info.is_skill)
        .map(|candidate| candidate.info)
        .collect())
}

pub fn list_local_skill_candidates(root: &str) -> Result<Vec<LocalSkillCandidate>, SkillsError> {
    let root_path = ensure_skills_root(root)?;

    let mut skills = Vec::new();
    for entry in fs::read_dir(root_path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }

        let path = entry.path();
        let skill_md_path = path.join("SKILL.md");
        let dir_name = entry.file_name().to_string_lossy().to_string();
        skills.push(build_local_candidate(dir_name, path, skill_md_path));
    }

    skills.sort_by(|a, b| a.info.name.to_lowercase().cmp(&b.info.name.to_lowercase()));
    Ok(skills)
}

pub fn skill_info_from_ref(skill_ref: &A2CSkillRef) -> SkillInfo {
    let path = PathBuf::from(&skill_ref.path);
    SkillInfo {
        name: skill_ref.name.clone(),
        path: skill_ref.path.clone(),
        skill_md_path: path.join("SKILL.md").to_string_lossy().to_string(),
        has_skill_md: path.join("SKILL.md").is_file(),
        is_skill: true,
        invalid_reason: None,
        description: Some(truncate_description(&skill_ref.description)),
        source: skill_ref.source.clone(),
        uri: skill_ref.uri.clone(),
    }
}

pub fn read_skill_markdown(root: &str, skill_path: &str) -> Result<String, SkillsError> {
    let skill_md_path = skill_markdown_path(root, skill_path)?;
    fs::read_to_string(skill_md_path).map_err(Into::into)
}

pub fn read_skill_markdown_file(skill_path: &str) -> Result<String, SkillsError> {
    let skill_path = PathBuf::from(skill_path);
    if !skill_path.is_dir() {
        return Err(SkillsError::SkillNotDirectory(
            skill_path.to_string_lossy().to_string(),
        ));
    }
    let skill_md_path = skill_path.join("SKILL.md");
    if !skill_md_path.is_file() {
        return Err(SkillsError::SkillMarkdownNotFound(
            skill_md_path.to_string_lossy().to_string(),
        ));
    }
    fs::read_to_string(skill_md_path).map_err(Into::into)
}

pub fn skill_markdown_path(root: &str, skill_path: &str) -> Result<PathBuf, SkillsError> {
    let root_path = expand_home(root);
    ensure_existing_root(&root_path)?;

    let root_canonical = root_path.canonicalize()?;
    let skill_path = PathBuf::from(skill_path);
    let skill_canonical = skill_path.canonicalize()?;

    if !skill_canonical.starts_with(&root_canonical) {
        return Err(SkillsError::PathOutsideRoot);
    }

    if !skill_canonical.is_dir() {
        return Err(SkillsError::SkillNotDirectory(
            skill_canonical.to_string_lossy().to_string(),
        ));
    }

    let skill_md_path = skill_canonical.join("SKILL.md");
    if !skill_md_path.is_file() {
        return Err(SkillsError::SkillMarkdownNotFound(
            skill_md_path.to_string_lossy().to_string(),
        ));
    }

    Ok(skill_md_path)
}

pub fn open_skill_folder(root: &str, skill_path: &str) -> Result<PathBuf, SkillsError> {
    let root_path = ensure_skills_root(root)?;
    let root_canonical = root_path.canonicalize()?;
    let skill_path = PathBuf::from(skill_path);
    let skill_canonical = skill_path.canonicalize()?;

    if !skill_canonical.starts_with(&root_canonical) {
        return Err(SkillsError::PathOutsideRoot);
    }

    if !skill_canonical.is_dir() {
        return Err(SkillsError::SkillNotDirectory(
            skill_canonical.to_string_lossy().to_string(),
        ));
    }

    Ok(skill_canonical)
}

pub fn ensure_skills_root(root: &str) -> Result<PathBuf, SkillsError> {
    let root_path = expand_home(root);
    if root_path.exists() {
        ensure_existing_root(&root_path)?;
        return Ok(root_path);
    }

    fs::create_dir_all(&root_path)?;
    ensure_existing_root(&root_path)?;
    Ok(root_path)
}

fn ensure_existing_root(root_path: &Path) -> Result<(), SkillsError> {
    if !root_path.exists() {
        return Err(SkillsError::RootNotFound(
            root_path.to_string_lossy().to_string(),
        ));
    }
    if !root_path.is_dir() {
        return Err(SkillsError::RootNotDirectory(
            root_path.to_string_lossy().to_string(),
        ));
    }
    Ok(())
}

fn build_local_candidate(
    dir_name: String,
    path: PathBuf,
    skill_md_path: PathBuf,
) -> LocalSkillCandidate {
    let path_string = path.to_string_lossy().to_string();
    let skill_md_path_string = skill_md_path.to_string_lossy().to_string();
    let has_skill_md = skill_md_path.is_file();
    let canonical_path = path.canonicalize().ok();

    let mut info = SkillInfo {
        name: dir_name.clone(),
        path: path_string,
        skill_md_path: skill_md_path_string,
        has_skill_md,
        is_skill: false,
        invalid_reason: None,
        description: None,
        source: "local".to_string(),
        uri: None,
    };

    if !has_skill_md {
        info.invalid_reason = Some("missing SKILL.md".to_string());
        return LocalSkillCandidate {
            info,
            canonical_path,
            protocol_name: None,
            full_description: None,
            frontmatter: None,
        };
    }

    let Ok(content) = fs::read_to_string(&skill_md_path) else {
        info.invalid_reason = Some("SKILL.md cannot be read".to_string());
        return LocalSkillCandidate {
            info,
            canonical_path,
            protocol_name: None,
            full_description: None,
            frontmatter: None,
        };
    };

    let frontmatter = parse_skill_frontmatter(&content);
    let Some(raw_name) = frontmatter_required_string(&frontmatter, "name") else {
        info.invalid_reason = Some("missing frontmatter name".to_string());
        return LocalSkillCandidate {
            info,
            canonical_path,
            protocol_name: None,
            full_description: None,
            frontmatter: Some(frontmatter),
        };
    };

    info.name = raw_name.clone();
    let protocol_name = match synthesize_user_name(&raw_name) {
        Ok(name) => name,
        Err(error) => {
            info.invalid_reason = Some(format!("invalid frontmatter name: {error}"));
            return LocalSkillCandidate {
                info,
                canonical_path,
                protocol_name: None,
                full_description: None,
                frontmatter: Some(frontmatter),
            };
        }
    };
    info.name = protocol_name.clone();

    let Some(description) = frontmatter_required_string(&frontmatter, "description") else {
        info.invalid_reason = Some("missing frontmatter description".to_string());
        return LocalSkillCandidate {
            info,
            canonical_path,
            protocol_name: Some(protocol_name),
            full_description: None,
            frontmatter: Some(frontmatter),
        };
    };

    info.description = Some(truncate_description(&description));
    info.is_skill = true;
    LocalSkillCandidate {
        info,
        canonical_path,
        protocol_name: Some(protocol_name),
        full_description: Some(description),
        frontmatter: Some(frontmatter),
    }
}

fn frontmatter_required_string(frontmatter: &Map<String, Value>, field: &str) -> Option<String> {
    frontmatter
        .get(field)
        .filter(|value| !value.is_null())
        .map(scalar_to_string)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn truncate_description(description: &str) -> String {
    const MAX_DESCRIPTION_CHARS: usize = 50;

    let mut chars = description.chars();
    let truncated: String = chars.by_ref().take(MAX_DESCRIPTION_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_expand_home_keeps_non_home_path() {
        assert_eq!(expand_home("/tmp/skills"), PathBuf::from("/tmp/skills"));
    }

    #[test]
    fn test_list_skills_returns_empty_for_empty_directory() {
        let tmp = tempdir().unwrap();
        let skills = list_skills(tmp.path().to_str().unwrap()).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn test_list_skills_returns_skill_metadata() {
        let tmp = tempdir().unwrap();
        let alpha = tmp.path().join("alpha");
        let beta = tmp.path().join("beta");
        fs::create_dir_all(&beta).unwrap();
        fs::create_dir_all(&alpha).unwrap();
        fs::write(
            alpha.join("SKILL.md"),
            "---\nname: frontmatter-alpha\ndescription: Alpha skill description\n---\n# Alpha\n",
        )
        .unwrap();
        fs::write(tmp.path().join("README.md"), "ignored").unwrap();

        let skills = list_skills(tmp.path().to_str().unwrap()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "frontmatter-alpha");
        assert!(skills[0].has_skill_md);
        assert!(skills[0].is_skill);
        assert!(skills[0].skill_md_path.ends_with("alpha/SKILL.md"));
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Alpha skill description")
        );
        assert_eq!(skills[0].source, "local");

        let candidates = list_local_skill_candidates(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(candidates.len(), 2);
        let invalid = candidates
            .iter()
            .find(|candidate| candidate.info.name == "beta")
            .expect("missing beta candidate");
        assert!(!invalid.info.has_skill_md);
        assert!(!invalid.info.is_skill);
        assert_eq!(
            invalid.info.invalid_reason.as_deref(),
            Some("missing SKILL.md")
        );
    }

    #[test]
    fn test_list_skills_reads_frontmatter_description() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("uat");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            r#"---
name: uat
description:
  tfrobot-client 用户验收测试（协同 UAT）。不同于 Web 项目能用 Playwright 自动驱动，Tauri 桌面端无
  E2E 能力，所以本 skill 是 Claude + 用户协同执行。
argument-hint: "[场景名]"
---

# UAT
"#,
        )
        .unwrap();

        let skills = list_skills(tmp.path().to_str().unwrap()).unwrap();

        assert_eq!(
            skills[0].description.as_deref(),
            Some("tfrobot-client 用户验收测试（协同 UAT）。不同于 Web 项目能用 Playwri...")
        );
    }

    #[test]
    fn test_list_skills_requires_frontmatter_name_and_description() {
        let tmp = tempdir().unwrap();
        let missing_name = tmp.path().join("missing-name");
        let missing_description = tmp.path().join("missing-description");
        let invalid_name = tmp.path().join("invalid-name");
        fs::create_dir_all(&missing_name).unwrap();
        fs::create_dir_all(&missing_description).unwrap();
        fs::create_dir_all(&invalid_name).unwrap();
        fs::write(
            missing_name.join("SKILL.md"),
            "---\ndescription: Missing name\n---\n# Missing\n",
        )
        .unwrap();
        fs::write(
            missing_description.join("SKILL.md"),
            "---\nname: missing-description\n---\n# Missing\n",
        )
        .unwrap();
        fs::write(
            invalid_name.join("SKILL.md"),
            "---\nname: Invalid Name\ndescription: Invalid\n---\n# Invalid\n",
        )
        .unwrap();

        let skills = list_skills(tmp.path().to_str().unwrap()).unwrap();
        assert!(skills.is_empty());

        let candidates = list_local_skill_candidates(tmp.path().to_str().unwrap()).unwrap();
        let reasons: Vec<_> = candidates
            .into_iter()
            .map(|candidate| candidate.info.invalid_reason.unwrap())
            .collect();
        assert!(reasons.contains(&"missing frontmatter name".to_string()));
        assert!(reasons.contains(&"missing frontmatter description".to_string()));
        assert!(reasons
            .iter()
            .any(|reason| reason.starts_with("invalid frontmatter name:")));
    }

    #[test]
    fn test_list_skills_creates_missing_root() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing");

        let skills = list_skills(missing.to_str().unwrap()).unwrap();

        assert!(missing.is_dir());
        assert!(skills.is_empty());
    }

    #[test]
    fn test_list_skills_errors_when_root_is_file() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("skills");
        fs::write(&file, "not a directory").unwrap();

        let err = list_skills(file.to_str().unwrap()).unwrap_err();

        assert!(matches!(err, SkillsError::RootNotDirectory(_)));
    }

    #[test]
    fn test_read_skill_markdown_reads_main_file() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("alpha");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# Alpha\n").unwrap();

        let content =
            read_skill_markdown(tmp.path().to_str().unwrap(), skill.to_str().unwrap()).unwrap();

        assert_eq!(content, "# Alpha\n");
    }

    #[test]
    fn test_read_skill_markdown_errors_when_missing() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("alpha");
        fs::create_dir_all(&skill).unwrap();

        let err =
            read_skill_markdown(tmp.path().to_str().unwrap(), skill.to_str().unwrap()).unwrap_err();

        assert!(matches!(err, SkillsError::SkillMarkdownNotFound(_)));
    }

    #[test]
    fn test_read_skill_markdown_rejects_path_outside_root() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let skill = outside.path().join("alpha");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# Alpha").unwrap();

        let err = read_skill_markdown(root.path().to_str().unwrap(), skill.to_str().unwrap())
            .unwrap_err();

        assert!(matches!(err, SkillsError::PathOutsideRoot));
    }

    #[test]
    fn test_open_skill_folder_rejects_path_outside_root() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let skill = outside.path().join("alpha");
        fs::create_dir_all(&skill).unwrap();

        let err =
            open_skill_folder(root.path().to_str().unwrap(), skill.to_str().unwrap()).unwrap_err();

        assert!(matches!(err, SkillsError::PathOutsideRoot));
    }

    #[test]
    fn test_ensure_skills_root_creates_missing_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");

        let created = ensure_skills_root(root.to_str().unwrap()).unwrap();

        assert!(created.is_dir());
    }
}
