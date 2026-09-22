//! Portable Computer configuration package: versioned format, export assembly, and
//! import parsing/preview.
//!
//! The package carries only client-owned and SDK-owned durable configuration. It
//! deliberately excludes secrets, Keychain content, Manager session state, logs,
//! caches, plugin materialization and any machine-local path. MCP `env`/`headers`
//! literals are preserved verbatim (CLI-native semantics), while password-type
//! Input defaults and plugin install locations are stripped.

use crate::services::built_in_tools::CommandLineToolPolicy;
use crate::services::client_control::RemoteControlPolicy;
use crate::services::computer::{
    ComputerProfile, ComputerProfileConnectionPolicy, COMPUTER_PROFILE_SCHEMA_VERSION,
    MAX_MCP_START_CONCURRENCY,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Stable package format version written by this client.
pub const PORTABLE_PACKAGE_FORMAT_VERSION: u32 = 1;

/// Package format versions this client can read (1..=1 today).
pub const SUPPORTED_PACKAGE_FORMAT_VERSIONS: std::ops::RangeInclusive<u32> = 1..=1;

/// Version of the SDK-owned configuration schema captured inside the package.
pub const PORTABLE_SDK_SCHEMA_VERSION: u32 = 1;

/// The `a2c-smcp` crate version this client was built against.
pub const A2C_SMCP_SDK_VERSION: &str = "0.4.1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PackageGroup {
    BasicProfile,
    McpAndInputs,
    NonSensitiveInputValues,
    SkillsAndPlugins,
}

impl PackageGroup {
    pub const ALL: [PackageGroup; 4] = [
        PackageGroup::BasicProfile,
        PackageGroup::McpAndInputs,
        PackageGroup::NonSensitiveInputValues,
        PackageGroup::SkillsAndPlugins,
    ];

    pub fn key(self) -> &'static str {
        match self {
            PackageGroup::BasicProfile => "basic_profile",
            PackageGroup::McpAndInputs => "mcp_and_inputs",
            PackageGroup::NonSensitiveInputValues => "non_sensitive_input_values",
            PackageGroup::SkillsAndPlugins => "skills_and_plugins",
        }
    }
}

/// The top-level portable package document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableComputerPackage {
    pub format_version: u32,
    pub manifest: PackageManifest,
    #[serde(default)]
    pub groups: Vec<PackageGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<PortableProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_config: Option<PortableSdkConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_values: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<PortableSkills>,
}

impl PortableComputerPackage {
    pub fn has_group(&self, group: PackageGroup) -> bool {
        self.groups.contains(&group)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageManifest {
    pub client_version: String,
    pub sdk_version: String,
    pub schema_version: u32,
    pub exported_at: String,
    pub source_computer_name: String,
}

/// Durable profile fields that travel with the package. `id` is regenerated on
/// import and `robot_binding` (Manager account state) is intentionally excluded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableProfile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub connection_policy: ComputerProfileConnectionPolicy,
    pub remote_control: RemoteControlPolicy,
    pub command_line: CommandLineToolPolicy,
    #[serde(default)]
    pub mcp_start_concurrency: usize,
}

impl PortableProfile {
    pub fn from_computer_profile(profile: &ComputerProfile) -> Self {
        Self {
            name: profile.name.clone(),
            description: profile.description.clone(),
            connection_policy: profile.connection_policy.clone(),
            remote_control: profile.remote_control.clone(),
            command_line: profile.command_line.clone(),
            mcp_start_concurrency: profile.mcp_start_concurrency,
        }
    }

    pub fn to_computer_profile(&self, id: impl Into<String>) -> ComputerProfile {
        ComputerProfile {
            schema_version: COMPUTER_PROFILE_SCHEMA_VERSION,
            id: id.into(),
            name: self.name.clone(),
            description: self.description.clone(),
            connection_policy: self.connection_policy.clone(),
            remote_control: self.remote_control.clone(),
            command_line: self.command_line.clone(),
            mcp_start_concurrency: self.mcp_start_concurrency,
            robot_binding: None,
        }
    }
}

/// Complete SDK-owned configuration scope. The client anchors `XDG_CONFIG_HOME`
/// at the project anchor, so the `user_*` maps capture the per-Computer user
/// scope (where `enabledPlugins` and other governance scalars live) in addition
/// to the project/local `.tfrobot` scope carried by `ProjectConfigDoc`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableSdkConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_local: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_local: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_settings: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_mcp: Option<Map<String, Value>>,
}

impl PortableSdkConfig {
    pub fn is_empty(&self) -> bool {
        self.settings.is_none()
            && self.settings_local.is_none()
            && self.mcp.is_none()
            && self.mcp_local.is_none()
            && self.user_settings.is_none()
            && self.user_mcp.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableSkills {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub user_files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marketplaces: Vec<PortableMarketplaceDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installed_plugins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableMarketplaceDeclaration {
    pub name: String,
    /// Recorded marketplace source (`{ "type": "git", "url": ... }` or a local path).
    pub source: Value,
    /// Informational commit SHA; preserved for display only and never consumed on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
}

/// Per-section import compatibility status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SectionStatus {
    Compatible,
    Migratable,
    Incompatible,
    Conflict,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SectionPreview {
    pub group: PackageGroup,
    pub status: SectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Lightweight result of inspecting a package before it is adopted by the create flow.
///
/// It contains only client-facing metadata used to prefill the create form; it does
/// not resolve name conflicts and never probes runtime dependencies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerPackageInspection {
    pub original_name: String,
    pub description: Option<String>,
    pub format_version: u32,
    pub groups: Vec<PackageGroup>,
}

impl ComputerPackageInspection {
    pub fn from_package(package: &PortableComputerPackage) -> Self {
        Self {
            original_name: package.manifest.source_computer_name.clone(),
            description: package
                .profile
                .as_ref()
                .and_then(|profile| profile.description.clone()),
            format_version: package.format_version,
            groups: package.groups.clone(),
        }
    }
}

fn preview_section(group: PackageGroup, package: &PortableComputerPackage) -> SectionPreview {
    let present = package.has_group(group);
    if !present {
        return SectionPreview {
            group,
            status: SectionStatus::Missing,
            message: None,
        };
    }
    let (status, message) = match group {
        PackageGroup::BasicProfile => match package.profile.as_ref() {
            None => (
                SectionStatus::Incompatible,
                Some("basic profile group is selected but missing profile data".to_string()),
            ),
            Some(profile) if profile.name.trim().is_empty() => (
                SectionStatus::Incompatible,
                Some("profile name is empty".to_string()),
            ),
            Some(profile)
                if profile.mcp_start_concurrency == 0
                    || profile.mcp_start_concurrency > MAX_MCP_START_CONCURRENCY =>
            {
                (
                    SectionStatus::Incompatible,
                    Some(format!(
                        "MCP start concurrency must be between 1 and {MAX_MCP_START_CONCURRENCY}"
                    )),
                )
            }
            Some(_) => (SectionStatus::Compatible, None),
        },
        PackageGroup::McpAndInputs => match package.sdk_config.as_ref() {
            None => (
                SectionStatus::Incompatible,
                Some("MCP/Input group is selected but missing SDK config".to_string()),
            ),
            Some(_) => (SectionStatus::Compatible, None),
        },
        PackageGroup::NonSensitiveInputValues => {
            if package.input_values.is_some() {
                (SectionStatus::Compatible, None)
            } else {
                (
                    SectionStatus::Incompatible,
                    Some("non-sensitive input values group is selected but missing".to_string()),
                )
            }
        }
        PackageGroup::SkillsAndPlugins => {
            if package.skills.is_some() {
                (SectionStatus::Compatible, None)
            } else {
                (
                    SectionStatus::Incompatible,
                    Some("skills/plugins group is selected but missing".to_string()),
                )
            }
        }
    };
    SectionPreview {
        group,
        status,
        message,
    }
}

/// Validates a package before any write; returns a structured error on the first
/// incompatible or unknown-required-field condition.
pub fn validate_package_for_import(
    package: &PortableComputerPackage,
) -> Result<(), PortableConfigError> {
    if !SUPPORTED_PACKAGE_FORMAT_VERSIONS.contains(&package.format_version) {
        return Err(PortableConfigError::UnsupportedFormatVersion {
            found: package.format_version,
            supported: format!(
                "{}-{}",
                SUPPORTED_PACKAGE_FORMAT_VERSIONS.start(),
                SUPPORTED_PACKAGE_FORMAT_VERSIONS.end()
            ),
        });
    }
    for section in PackageGroup::ALL
        .iter()
        .map(|group| preview_section(*group, package))
    {
        if section.status == SectionStatus::Incompatible {
            return Err(PortableConfigError::InvalidPackage(
                section
                    .message
                    .unwrap_or_else(|| format!("invalid section {:?}", section.group)),
            ));
        }
    }
    Ok(())
}

/// Structured error model shared by export and import boundaries.
#[derive(Debug, thiserror::Error)]
pub enum PortableConfigError {
    #[error("invalid portable package: {0}")]
    InvalidPackage(String),
    #[error("unsupported package format version {found}; supported: {supported}")]
    UnsupportedFormatVersion { found: u32, supported: String },
    #[error("portable package contains an unsupported or unknown field: {0}")]
    UnknownField(String),
    #[error("package manifest is missing required field: {0}")]
    MissingManifestField(String),
    #[error("package is corrupt or could not be parsed: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Deserializes a package from raw JSON with a version-compatibility gate.
pub fn parse_package(bytes: &[u8]) -> Result<PortableComputerPackage, PortableConfigError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        PortableConfigError::Corrupt(format!("package is not valid JSON: {error}"))
    })?;
    parse_package_value(&value)
}

pub fn parse_package_value(value: &Value) -> Result<PortableComputerPackage, PortableConfigError> {
    let root = value.as_object().ok_or_else(|| {
        PortableConfigError::InvalidPackage("package root must be an object".into())
    })?;
    let format_version = root
        .get("formatVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| PortableConfigError::MissingManifestField("formatVersion".into()))?
        as u32;
    if !SUPPORTED_PACKAGE_FORMAT_VERSIONS.contains(&format_version) {
        return Err(PortableConfigError::UnsupportedFormatVersion {
            found: format_version,
            supported: format!(
                "{}-{}",
                SUPPORTED_PACKAGE_FORMAT_VERSIONS.start(),
                SUPPORTED_PACKAGE_FORMAT_VERSIONS.end()
            ),
        });
    }
    serde_json::from_value(value.clone()).map_err(|error| {
        if error.to_string().contains("unknown field") {
            PortableConfigError::UnknownField(error.to_string())
        } else {
            PortableConfigError::InvalidPackage(error.to_string())
        }
    })
}

/// Serializes a package to pretty JSON.
pub fn serialize_package(
    package: &PortableComputerPackage,
) -> Result<Vec<u8>, PortableConfigError> {
    serde_json::to_vec_pretty(package)
        .map_err(|error| PortableConfigError::InvalidPackage(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_package() -> PortableComputerPackage {
        PortableComputerPackage {
            format_version: PORTABLE_PACKAGE_FORMAT_VERSION,
            manifest: PackageManifest {
                client_version: "0.2.6".into(),
                sdk_version: A2C_SMCP_SDK_VERSION.into(),
                schema_version: PORTABLE_SDK_SCHEMA_VERSION,
                exported_at: "2026-09-21T00:00:00Z".into(),
                source_computer_name: "One".into(),
            },
            groups: PackageGroup::ALL.to_vec(),
            profile: None,
            sdk_config: None,
            input_values: None,
            skills: None,
        }
    }

    #[test]
    fn package_round_trips_with_version_manifest() {
        let package = minimal_package();
        let bytes = serialize_package(&package).unwrap();
        let decoded = parse_package(&bytes).unwrap();
        assert_eq!(decoded, package);
    }

    #[test]
    fn unknown_format_version_is_blocked() {
        let mut package = minimal_package();
        package.format_version = 2;
        let bytes = serialize_package(&package).unwrap();
        let error = parse_package(&bytes).unwrap_err();
        assert!(matches!(
            error,
            PortableConfigError::UnsupportedFormatVersion { .. }
        ));
    }

    #[test]
    fn unknown_field_is_a_structured_error() {
        let package = minimal_package();
        let mut value = serde_json::to_value(&package).unwrap();
        value["unexpected"] = serde_json::json!(true);
        let error = parse_package_value(&value).unwrap_err();
        assert!(matches!(error, PortableConfigError::UnknownField(_)));
    }

    #[test]
    fn inspection_surfaces_original_name_description_and_groups() {
        let mut package = minimal_package();
        package.groups = vec![PackageGroup::BasicProfile, PackageGroup::McpAndInputs];
        package.profile = Some(PortableProfile {
            name: "One".into(),
            description: Some("source".into()),
            connection_policy: ComputerProfileConnectionPolicy::default(),
            remote_control: RemoteControlPolicy::default(),
            command_line: CommandLineToolPolicy::default(),
            mcp_start_concurrency: 3,
        });
        let inspection = ComputerPackageInspection::from_package(&package);
        assert_eq!(inspection.original_name, "One");
        assert_eq!(inspection.description.as_deref(), Some("source"));
        assert_eq!(
            inspection.groups,
            vec![PackageGroup::BasicProfile, PackageGroup::McpAndInputs]
        );
    }

    #[test]
    fn import_validation_blocks_incompatible_sections() {
        let mut package = minimal_package();
        package.groups = vec![PackageGroup::NonSensitiveInputValues];
        package.input_values = None;
        let error = validate_package_for_import(&package).unwrap_err();
        assert!(matches!(error, PortableConfigError::InvalidPackage(_)));
    }
}
