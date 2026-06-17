use crate::services::skills::{self, SkillsError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use smcp::A2CSkillRef;
use smcp_computer::computer::{Computer, SilentSession};
use smcp_computer::skills::SOURCE_USER;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSyncSummary {
    pub local_synced: usize,
    pub mcp_synced: usize,
    pub ignored_conflicts: Vec<SkillConflict>,
    pub skipped: Vec<SkillSkipped>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillConflict {
    pub skill_name: String,
    pub kept_source: String,
    pub kept_name: String,
    pub ignored_source: String,
    pub ignored_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSkipped {
    pub skill_name: String,
    pub source: String,
    pub reason: String,
}

pub fn summarize_user_skills(
    root: &str,
    active_refs: &[A2CSkillRef],
    mcp_synced: usize,
) -> Result<SkillSyncSummary, SkillsError> {
    let skills = skills::list_local_skill_candidates(root)?;
    let mut summary = SkillSyncSummary {
        mcp_synced,
        ..SkillSyncSummary::default()
    };

    for candidate in skills {
        let skill = candidate.info;
        if !skill.has_skill_md {
            continue;
        }
        if !skill.is_skill {
            summary.skipped.push(SkillSkipped {
                skill_name: skill.name,
                source: SOURCE_USER.to_string(),
                reason: skill
                    .invalid_reason
                    .unwrap_or_else(|| "invalid skill".to_string()),
            });
            continue;
        }

        let raw_path = PathBuf::from(&skill.path);
        let Some(canonical_path) = candidate.canonical_path else {
            summary.skipped.push(SkillSkipped {
                skill_name: skill.name,
                source: SOURCE_USER.to_string(),
                reason: "skill path cannot be canonicalized".to_string(),
            });
            continue;
        };

        match active_refs
            .iter()
            .find(|reference| reference.name == skill.name)
        {
            Some(existing)
                if existing.source == SOURCE_USER
                    && paths_equivalent(&existing.path, &raw_path, &canonical_path) =>
            {
                summary.local_synced += 1;
            }
            Some(existing) => {
                summary.ignored_conflicts.push(conflict_for_existing(
                    &skill.name,
                    existing,
                    SOURCE_USER,
                    "same full protocol skill name; keeping first loaded",
                ));
            }
            None => {
                summary.skipped.push(SkillSkipped {
                    skill_name: skill.name,
                    source: SOURCE_USER.to_string(),
                    reason: "not staged by runtime".to_string(),
                });
            }
        }
    }

    Ok(summary)
}

pub async fn stage_configured_user_skills(
    root: &str,
    computer: &Arc<Computer<SilentSession>>,
) -> Result<usize, SkillsError> {
    let refs = configured_user_skill_refs(root)?;
    let present_names: HashSet<String> = refs
        .iter()
        .map(|skill_ref| skill_ref.name.clone())
        .collect();
    let registry = computer.skill_registry_arc();
    let mut registry = registry.write().await;

    let existing_user_names: Vec<String> = registry
        .active_refs()
        .into_iter()
        .filter(|skill_ref| skill_ref.source == SOURCE_USER)
        .map(|skill_ref| skill_ref.name)
        .collect();
    for name in existing_user_names {
        if !present_names.contains(&name) {
            registry.mark_orphan(&name);
        }
    }

    let mut staged = 0;
    for skill_ref in refs {
        if registry.register_or_update(skill_ref) {
            staged += 1;
        }
    }
    Ok(staged)
}

fn configured_user_skill_refs(root: &str) -> Result<Vec<A2CSkillRef>, SkillsError> {
    let skills = skills::list_local_skill_candidates(root)?;
    let mut refs = Vec::new();

    for candidate in skills {
        if !candidate.info.is_skill {
            continue;
        }
        let Some(name) = candidate.protocol_name else {
            continue;
        };
        let Some(canonical_path) = candidate.canonical_path else {
            continue;
        };
        let Some(description) = candidate.full_description else {
            continue;
        };
        let Some(frontmatter) = candidate.frontmatter else {
            continue;
        };

        refs.push(build_user_ref(
            name,
            &canonical_path,
            description,
            &frontmatter,
        ));
    }

    Ok(refs)
}

fn build_user_ref(
    name: String,
    path: &std::path::Path,
    description: String,
    frontmatter: &Map<String, Value>,
) -> A2CSkillRef {
    let mut skill_ref = A2CSkillRef {
        name,
        source: SOURCE_USER.to_string(),
        uri: None,
        path: path.to_string_lossy().to_string(),
        description,
        license: None,
        compatibility: None,
        allowed_tools: None,
        version: None,
        skill_metadata: None,
    };
    apply_frontmatter_optional_fields(&mut skill_ref, frontmatter);
    skill_ref
}

fn apply_frontmatter_optional_fields(
    skill_ref: &mut A2CSkillRef,
    frontmatter: &Map<String, Value>,
) {
    if let Some(value) = frontmatter.get("license").filter(|value| !value.is_null()) {
        skill_ref.license = Some(scalar_to_string(value));
    }
    if let Some(value) = frontmatter
        .get("compatibility")
        .filter(|value| !value.is_null())
    {
        skill_ref.compatibility = Some(scalar_to_string(value));
    }
    if let Some(value) = frontmatter
        .get("allowed-tools")
        .or_else(|| frontmatter.get("allowed_tools"))
        .filter(|value| !value.is_null())
    {
        skill_ref.allowed_tools = Some(to_string_list(value));
    }
    if let Some(Value::Object(metadata)) = frontmatter.get("metadata") {
        skill_ref.skill_metadata = Some(Value::Object(metadata.clone()));
    }
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn to_string_list(value: &Value) -> Vec<String> {
    match value {
        Value::Array(values) => values.iter().map(scalar_to_string).collect(),
        other => vec![scalar_to_string(other)],
    }
}

fn paths_equivalent(
    existing: &str,
    raw_path: &std::path::Path,
    canonical_path: &std::path::Path,
) -> bool {
    let existing_path = PathBuf::from(existing);
    if existing_path == raw_path || existing_path == canonical_path {
        return true;
    }
    existing_path
        .canonicalize()
        .map(|path| path == canonical_path)
        .unwrap_or(false)
}

fn conflict_for_existing(
    name: &str,
    existing: &A2CSkillRef,
    ignored_source: &str,
    reason: &str,
) -> SkillConflict {
    SkillConflict {
        skill_name: name.to_string(),
        kept_source: existing.source.clone(),
        kept_name: existing.name.clone(),
        ignored_source: ignored_source.to_string(),
        ignored_name: name.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn skill_ref(name: &str, source: &str, path: &std::path::Path) -> A2CSkillRef {
        A2CSkillRef {
            name: name.to_string(),
            source: source.to_string(),
            uri: None,
            path: path.to_string_lossy().to_string(),
            description: String::new(),
            license: None,
            compatibility: None,
            allowed_tools: None,
            version: None,
            skill_metadata: None,
        }
    }

    #[test]
    fn summarize_user_skills_counts_synced_and_skipped_entries() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("skills-home");
        let valid = home.join("demo-skill");
        let missing = home.join("missing-md");
        std::fs::create_dir_all(&valid).unwrap();
        std::fs::create_dir_all(&missing).unwrap();
        std::fs::write(
            valid.join("SKILL.md"),
            "---\nname: published-demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();

        let active_refs = vec![skill_ref(
            "published-demo",
            SOURCE_USER,
            &valid.canonicalize().unwrap(),
        )];

        let summary =
            summarize_user_skills(&home.to_string_lossy(), &active_refs, 2).expect("summary");

        assert_eq!(summary.local_synced, 1);
        assert_eq!(summary.mcp_synced, 2);
        assert!(summary.ignored_conflicts.is_empty());
        assert!(summary.skipped.is_empty());
    }

    #[test]
    fn summarize_user_skills_reports_conflicts() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("skills-home");
        let local = home.join("demo-skill");
        let kept = tmp.path().join("kept-skill");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::write(
            local.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();

        let active_refs = vec![skill_ref("demo-skill", "marketplace", &kept)];

        let summary =
            summarize_user_skills(&home.to_string_lossy(), &active_refs, 0).expect("summary");

        assert_eq!(summary.local_synced, 0);
        assert_eq!(summary.ignored_conflicts.len(), 1);
        assert_eq!(summary.ignored_conflicts[0].skill_name, "demo-skill");
        assert_eq!(summary.ignored_conflicts[0].kept_source, "marketplace");
        assert!(summary.skipped.is_empty());
    }

    #[test]
    fn summarize_user_skills_reports_missing_frontmatter_description() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("skills-home");
        let body_only = home.join("body-only");
        std::fs::create_dir_all(&body_only).unwrap();
        std::fs::write(
            body_only.join("SKILL.md"),
            "---\nname: body-only\n---\n# Body Only\n\nBody description\n",
        )
        .unwrap();

        let summary = summarize_user_skills(&home.to_string_lossy(), &[], 0).expect("summary");

        assert_eq!(summary.local_synced, 0);
        assert_eq!(summary.skipped.len(), 1);
        assert_eq!(summary.skipped[0].skill_name, "body-only");
        assert_eq!(summary.skipped[0].reason, "missing frontmatter description");
    }

    #[test]
    fn summarize_user_skills_reports_missing_frontmatter_name() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("skills-home");
        let missing_name = home.join("missing-name");
        std::fs::create_dir_all(&missing_name).unwrap();
        std::fs::write(
            missing_name.join("SKILL.md"),
            "---\ndescription: Missing name\n---\n# Missing\n",
        )
        .unwrap();

        let summary = summarize_user_skills(&home.to_string_lossy(), &[], 0).expect("summary");

        assert_eq!(summary.local_synced, 0);
        assert_eq!(summary.skipped.len(), 1);
        assert_eq!(summary.skipped[0].skill_name, "missing-name");
        assert_eq!(summary.skipped[0].reason, "missing frontmatter name");
    }
}
