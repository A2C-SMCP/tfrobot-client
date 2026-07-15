use crate::services::config::ConfigService;
use a2c_smcp::smcp_computer::settings::config::{
    delete_config, duplicate_config, export_config, import_config, init_config, load_config,
    migrate_config, save_config, update_config, validate_config, ComputerConfigSnapshot,
    ConfigContext, ConfigCrudError, ConfigEdit, ProjectConfigDoc, ValidationReport,
};
use a2c_smcp::smcp_computer::settings::{
    resolve_mcp_config, EnvMap, ResolveMcpConfigArgs, ResolvedMcpConfig, SettingsValidationError,
    MANAGED_MCP_FILENAME, XDG_CONFIG_HOME_ENV,
};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The client-side boundary for SDK-owned Computer configuration.
///
/// `ConfigService` continues to own client profile/connection data. MCP, skill,
/// marketplace, plugin, and runtime configuration crosses this adapter only.
#[derive(Clone)]
pub struct SdkConfigService {
    config: Arc<ConfigService>,
}

#[derive(Debug, thiserror::Error)]
pub enum SdkConfigExportError {
    #[error("Cannot export invalid SDK MCP configuration: {details}")]
    InvalidSource {
        errors: Vec<SettingsValidationError>,
        details: String,
    },
    #[error(transparent)]
    Crud(#[from] ConfigCrudError),
}

impl SdkConfigExportError {
    fn invalid_source(errors: Vec<SettingsValidationError>) -> Self {
        let details = errors
            .iter()
            .map(|error| {
                format!(
                    "{}:{}: {}",
                    error.source_path.as_deref().unwrap_or("SDK MCP config"),
                    error.field,
                    error.reason
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Self::InvalidSource { errors, details }
    }
}

/// Resolve only configuration owned by the Computer instance for a portable backup.
///
/// Policy is an ambient, read-only constraint of the machine performing the export. Pointing the
/// SDK resolver at a guaranteed-absent managed file removes that layer before precedence is
/// applied. Filtering Policy winners after resolution would also discard an instance-owned server
/// hidden by a same-name Policy declaration.
fn resolve_portable_mcp_without_policy(
    context: &InstanceConfigContext,
    staging_root: &Path,
) -> ResolvedMcpConfig {
    let absent_policy_path = staging_root
        .join("excluded-policy")
        .join(MANAGED_MCP_FILENAME);
    resolve_mcp_config(ResolveMcpConfigArgs {
        cwd: Some(context.project_anchor()),
        env: Some(context.env()),
        managed_mcp_path: Some(&absent_policy_path),
        ..Default::default()
    })
}

#[derive(Clone)]
pub(crate) struct InstanceConfigContext {
    project_anchor: PathBuf,
    skill_home: PathBuf,
    env: EnvMap,
}

impl InstanceConfigContext {
    pub(crate) fn new(project_anchor: PathBuf, skill_home: PathBuf) -> Self {
        let mut env = EnvMap::new();
        env.insert(
            XDG_CONFIG_HOME_ENV.to_string(),
            project_anchor.to_string_lossy().into_owned(),
        );
        Self {
            project_anchor,
            skill_home,
            env,
        }
    }

    pub(crate) fn sdk_context(&self) -> ConfigContext<'_> {
        let mut context = ConfigContext::new(&self.project_anchor);
        context.env = Some(&self.env);
        context.home = Some(&self.skill_home);
        context
    }

    pub(crate) fn load(&self) -> ComputerConfigSnapshot {
        load_config(&self.sdk_context())
    }

    pub(crate) fn project_anchor(&self) -> &Path {
        &self.project_anchor
    }

    pub(crate) fn skill_home(&self) -> &Path {
        &self.skill_home
    }

    pub(crate) fn env(&self) -> &EnvMap {
        &self.env
    }
}

impl SdkConfigService {
    pub fn new(config: Arc<ConfigService>) -> Self {
        Self { config }
    }

    pub fn project_anchor(&self, instance_id: &str) -> PathBuf {
        self.config
            .computer_instance_storage_root(instance_id)
            .join("sdk_config")
    }

    pub fn skill_home(&self, instance_id: &str) -> PathBuf {
        self.config
            .get_computer_instance(instance_id)
            .ok()
            .and_then(|instance| instance.local_skills_root)
            .unwrap_or_else(|| self.config.default_local_skills_root(instance_id))
    }

    pub fn env(&self, instance_id: &str) -> EnvMap {
        self.context(instance_id).env
    }

    fn context(&self, instance_id: &str) -> InstanceConfigContext {
        InstanceConfigContext::new(
            self.project_anchor(instance_id),
            self.skill_home(instance_id),
        )
    }

    pub fn init(&self, instance_id: &str) -> Result<(), ConfigCrudError> {
        init_config(&self.project_anchor(instance_id))
    }

    pub fn load(&self, instance_id: &str) -> ComputerConfigSnapshot {
        self.context(instance_id).load()
    }

    pub fn save(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<(), ConfigCrudError> {
        save_config(&self.project_anchor(instance_id), document)
    }

    pub fn update(
        &self,
        instance_id: &str,
        edits: &[ConfigEdit],
    ) -> Result<ComputerConfigSnapshot, ConfigCrudError> {
        let context = self.context(instance_id);
        update_config(&context.sdk_context(), edits)
    }

    pub fn validate(&self, document: &ProjectConfigDoc) -> ValidationReport {
        validate_config(document)
    }

    pub fn migrate(&self, instance_id: &str) -> Result<bool, ConfigCrudError> {
        migrate_config(&self.project_anchor(instance_id))
    }

    pub fn delete(&self, instance_id: &str) -> Result<(), ConfigCrudError> {
        delete_config(&self.project_anchor(instance_id))
    }

    pub fn duplicate(&self, source_id: &str, target_id: &str) -> Result<(), ConfigCrudError> {
        duplicate_config(
            &self.project_anchor(source_id),
            &self.project_anchor(target_id),
        )
    }

    pub fn export(&self, instance_id: &str) -> Result<ProjectConfigDoc, ConfigCrudError> {
        export_config(&self.project_anchor(instance_id))
    }

    /// Export every reconciled instance-owned MCP declaration through the SDK sanitizer.
    ///
    /// SDK shareable export intentionally omits local scopes. The client's CLI-native export is
    /// a full backup, so it resolves User/Project/Local declarations without ambient Policy,
    /// stages them in an isolated project document, and delegates redaction to the SDK.
    pub fn export_cli_native_mcp(
        &self,
        instance_id: &str,
    ) -> Result<ProjectConfigDoc, SdkConfigExportError> {
        let context = self.context(instance_id);
        let source_anchor = context.project_anchor.clone();
        let staging = tempfile::tempdir().map_err(|error| ConfigCrudError::Io {
            path: source_anchor.clone(),
            reason: format!("failed to create CLI-native export staging directory: {error}"),
        })?;
        let mut resolved = resolve_portable_mcp_without_policy(&context, staging.path());
        let export_errors = std::mem::take(&mut resolved.errors)
            .into_iter()
            .filter(|error| error.field != "inputs" && !error.field.starts_with("inputs."))
            .collect::<Vec<_>>();
        if !export_errors.is_empty() {
            return Err(SdkConfigExportError::invalid_source(export_errors));
        }

        let mut servers = Map::new();
        for (name, server) in resolved.servers {
            let value =
                serde_json::to_value(server.config).map_err(|error| ConfigCrudError::Io {
                    path: source_anchor.clone(),
                    reason: format!(
                        "failed to serialize MCP server '{}' for CLI-native export: {error}",
                        name
                    ),
                })?;
            let mut body = value
                .as_object()
                .cloned()
                .ok_or_else(|| ConfigCrudError::Io {
                    path: source_anchor.clone(),
                    reason: format!("serialized MCP server '{}' is not a JSON object", name),
                })?;
            body.remove("name");
            servers.insert(name, Value::Object(body));
        }

        let mut mcp = Map::new();
        mcp.insert("servers".to_string(), Value::Object(servers));
        save_config(
            staging.path(),
            &ProjectConfigDoc {
                mcp: Some(mcp),
                ..Default::default()
            },
        )?;
        Ok(export_config(staging.path())?)
    }

    pub fn import(
        &self,
        instance_id: &str,
        document: &ProjectConfigDoc,
    ) -> Result<ValidationReport, ConfigCrudError> {
        import_config(&self.project_anchor(instance_id), document)
    }

    pub fn owns_path(&self, instance_id: &str, path: &Path) -> bool {
        path.starts_with(self.project_anchor(instance_id))
            || path.starts_with(self.skill_home(instance_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2c_smcp::smcp_computer::settings::config::{ConfigEntity, EditIntent, ProjectConfigDoc};
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn lifecycle_is_scoped_to_one_computer_instance() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("computer-a").unwrap();

        let anchor = sdk_config.project_anchor("computer-a");
        assert!(anchor.join(".tfrobot/settings.json").is_file());
        assert!(anchor.join(".tfrobot/mcp.json").is_file());
        assert!(!sdk_config
            .project_anchor("computer-b")
            .join(".tfrobot/mcp.json")
            .exists());
        let snapshot = sdk_config.load("computer-a");
        assert!(!snapshot.revision.0.is_empty());
    }

    #[test]
    fn duplicate_and_delete_do_not_cross_instance_boundaries() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        sdk_config.duplicate("source", "target").unwrap();
        sdk_config.delete("source").unwrap();

        assert!(!sdk_config
            .project_anchor("source")
            .join(".tfrobot/mcp.json")
            .exists());
        assert!(sdk_config
            .project_anchor("target")
            .join(".tfrobot/mcp.json")
            .is_file());
    }

    #[test]
    fn duplicate_propagates_corrupt_source_error_without_mutating_target() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        sdk_config.init("target").unwrap();
        let source_mcp = sdk_config
            .project_anchor("source")
            .join(".tfrobot/mcp.json");
        let target_mcp = sdk_config
            .project_anchor("target")
            .join(".tfrobot/mcp.json");
        std::fs::write(&source_mcp, "{not-json").unwrap();
        let target_before = std::fs::read(&target_mcp).unwrap();

        let error = sdk_config.duplicate("source", "target").unwrap_err();

        assert!(matches!(error, ConfigCrudError::Io { ref path, .. } if path == &source_mcp));
        assert_eq!(std::fs::read(&target_mcp).unwrap(), target_before);
    }

    #[test]
    fn cli_native_export_rejects_corrupt_source_config() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        let source_mcp = sdk_config
            .project_anchor("source")
            .join(".tfrobot/mcp.json");
        std::fs::write(&source_mcp, "{not-json").unwrap();

        let error = sdk_config.export_cli_native_mcp("source").unwrap_err();

        match error {
            SdkConfigExportError::InvalidSource { errors, .. } => {
                assert!(errors.iter().any(|error| {
                    error.field == "<file>" && error.source_path.as_deref() == source_mcp.to_str()
                }));
            }
            other => panic!("expected invalid source error, got {other:?}"),
        }
    }

    #[test]
    fn cli_native_export_rejects_partial_backup_when_one_server_is_invalid() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "valid": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "node"}
                                },
                                "broken": {"type": "carrier-pigeon"}
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let error = sdk_config.export_cli_native_mcp("source").unwrap_err();

        match error {
            SdkConfigExportError::InvalidSource { errors, .. } => {
                assert!(errors.iter().any(|error| error.field == "servers.broken"));
            }
            other => panic!("expected invalid source error, got {other:?}"),
        }
    }

    #[test]
    fn cli_native_export_ignores_sdk_input_diagnostics_owned_by_the_client() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "valid": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "node"}
                                }
                            },
                            "inputs": [{"id": 7}]
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let exported = sdk_config.export_cli_native_mcp("source").unwrap();

        assert!(exported
            .mcp
            .as_ref()
            .and_then(|mcp| mcp.get("servers"))
            .and_then(Value::as_object)
            .is_some_and(|servers| servers.contains_key("valid")));
    }

    #[test]
    fn portable_mcp_resolution_excludes_policy_and_recovers_shadowed_instance_server() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "shared-name": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "instance-command"}
                                }
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let policy_directory = tempdir().unwrap();
        let policy_path = policy_directory.path().join(MANAGED_MCP_FILENAME);
        std::fs::write(
            &policy_path,
            serde_json::to_vec(&json!({
                "servers": {
                    "policy-only": {
                        "type": "stdio",
                        "server_parameters": {"command": "policy-only-command"}
                    },
                    "shared-name": {
                        "type": "stdio",
                        "server_parameters": {"command": "policy-command"}
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let context = sdk_config.context("source");
        let resolved_with_policy = resolve_mcp_config(ResolveMcpConfigArgs {
            cwd: Some(context.project_anchor()),
            env: Some(context.env()),
            managed_mcp_path: Some(&policy_path),
            ..Default::default()
        });
        assert!(resolved_with_policy.servers.contains_key("policy-only"));
        assert_eq!(
            serde_json::to_value(&resolved_with_policy.servers["shared-name"].config).unwrap()
                ["server_parameters"]["command"],
            "policy-command"
        );

        let staging = tempdir().unwrap();
        let portable = resolve_portable_mcp_without_policy(&context, staging.path());

        assert!(!portable.servers.contains_key("policy-only"));
        assert_eq!(
            serde_json::to_value(&portable.servers["shared-name"].config).unwrap()
                ["server_parameters"]["command"],
            "instance-command"
        );
    }

    #[test]
    fn portable_mcp_resolution_ignores_malformed_policy_diagnostics() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);
        sdk_config
            .save(
                "source",
                &ProjectConfigDoc {
                    mcp: Some(
                        json!({
                            "servers": {
                                "instance-server": {
                                    "type": "stdio",
                                    "server_parameters": {"command": "instance-command"}
                                }
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let policy_directory = tempdir().unwrap();
        let policy_path = policy_directory.path().join(MANAGED_MCP_FILENAME);
        std::fs::write(&policy_path, "{not-json").unwrap();
        let context = sdk_config.context("source");
        let resolved_with_policy = resolve_mcp_config(ResolveMcpConfigArgs {
            cwd: Some(context.project_anchor()),
            env: Some(context.env()),
            managed_mcp_path: Some(&policy_path),
            ..Default::default()
        });
        assert!(resolved_with_policy
            .errors
            .iter()
            .any(|error| error.source_path.as_deref() == policy_path.to_str()));

        let staging = tempdir().unwrap();
        let portable = resolve_portable_mcp_without_policy(&context, staging.path());

        assert!(portable.errors.is_empty());
        assert!(portable.servers.contains_key("instance-server"));
    }

    #[test]
    fn crud_and_portability_round_trip_through_the_adapter() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        sdk_config.init("source").unwrap();
        let before = sdk_config.load("source").revision;
        let after = sdk_config
            .update(
                "source",
                &[ConfigEdit::new(
                    ConfigEntity::McpServer("audit".to_string()),
                    EditIntent::Upsert(json!({
                        "type": "stdio",
                        "server_parameters": {"command": "node"}
                    })),
                )],
            )
            .unwrap();
        assert_ne!(before, after.revision);
        assert_eq!(after.mcp.servers[0].name, "audit");

        let exported = sdk_config.export("source").unwrap();
        assert!(sdk_config.validate(&exported).is_valid());

        sdk_config.save("saved", &exported).unwrap();
        assert_eq!(sdk_config.load("saved").mcp.servers[0].name, "audit");

        let report = sdk_config.import("imported", &exported).unwrap();
        assert!(report.is_valid());
        assert_eq!(sdk_config.load("imported").mcp.servers[0].name, "audit");
    }

    #[test]
    fn validate_migrate_and_path_ownership_are_instance_scoped() {
        let directory = tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let sdk_config = SdkConfigService::new(config);

        let invalid = ProjectConfigDoc {
            mcp: Some(
                json!({"servers": {"broken": {"type": "carrier-pigeon"}}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            ..Default::default()
        };
        assert!(!sdk_config.validate(&invalid).is_valid());

        sdk_config.init("computer-a").unwrap();
        assert!(!sdk_config.migrate("computer-a").unwrap());
        assert!(sdk_config.owns_path(
            "computer-a",
            &sdk_config
                .project_anchor("computer-a")
                .join(".tfrobot/mcp.json")
        ));
        assert!(!sdk_config.owns_path(
            "computer-a",
            &sdk_config
                .project_anchor("computer-b")
                .join(".tfrobot/mcp.json")
        ));
    }
}
