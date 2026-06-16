use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub path: String,
    pub skill_md_path: String,
    pub has_skill_md: bool,
    pub description: Option<String>,
    pub source: String,
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

pub fn list_skills(root: &str) -> Result<Vec<SkillInfo>, SkillsError> {
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
        let description = read_skill_description(&skill_md_path);
        let name = entry.file_name().to_string_lossy().to_string();
        skills.push(SkillInfo {
            name,
            path: path.to_string_lossy().to_string(),
            skill_md_path: skill_md_path.to_string_lossy().to_string(),
            has_skill_md: skill_md_path.is_file(),
            description,
            source: "local".to_string(),
        });
    }

    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(skills)
}

pub fn read_skill_markdown(root: &str, skill_path: &str) -> Result<String, SkillsError> {
    let skill_md_path = skill_markdown_path(root, skill_path)?;
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

fn read_skill_description(skill_md_path: &Path) -> Option<String> {
    let content = fs::read_to_string(skill_md_path).ok()?;
    if let Some(description) = read_frontmatter_description(&content) {
        return Some(truncate_description(&description));
    }

    let body = strip_frontmatter(&content);
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| !line.starts_with('#') && !line.starts_with("```"))
        .map(|line| truncate_description(line.trim_start_matches("- ").trim_start_matches("* ")))
}

fn read_frontmatter_description(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }

    let frontmatter: Vec<&str> = lines
        .by_ref()
        .take_while(|line| line.trim() != "---")
        .collect();

    let mut description_lines = Vec::new();
    let mut in_description_block = false;

    for line in frontmatter {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("description:") {
            let inline = rest.trim().trim_matches('"').trim_matches('\'');
            if !inline.is_empty() {
                return Some(inline.to_string());
            }
            in_description_block = true;
            continue;
        }

        if in_description_block {
            if trimmed.is_empty() {
                continue;
            }
            if !line.starts_with(' ') && !line.starts_with('\t') {
                break;
            }
            description_lines.push(trimmed);
        }
    }

    if description_lines.is_empty() {
        None
    } else {
        Some(description_lines.join(" "))
    }
}

fn strip_frontmatter(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("---") else {
        return content;
    };
    let Some((_frontmatter, body)) = rest.split_once("\n---") else {
        return content;
    };
    body
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
        fs::write(alpha.join("SKILL.md"), "# Alpha\n\nAlpha").unwrap();
        fs::write(tmp.path().join("README.md"), "ignored").unwrap();

        let skills = list_skills(tmp.path().to_str().unwrap()).unwrap();

        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha");
        assert!(skills[0].has_skill_md);
        assert!(skills[0].skill_md_path.ends_with("alpha/SKILL.md"));
        assert_eq!(skills[0].description.as_deref(), Some("Alpha"));
        assert_eq!(skills[0].source, "local");
        assert_eq!(skills[1].name, "beta");
        assert!(!skills[1].has_skill_md);
        assert!(skills[1].description.is_none());
        assert_eq!(skills[1].source, "local");
    }

    #[test]
    fn test_list_skills_reads_frontmatter_description() {
        let tmp = tempdir().unwrap();
        let skill = tmp.path().join("uat");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            r#"---
name: UAT
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
