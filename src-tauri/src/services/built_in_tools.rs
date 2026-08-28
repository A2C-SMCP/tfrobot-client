use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const COMMAND_LINE_BUNDLE_ID: &str = "tfrobot_tfbash_mcp";
pub const COMMAND_LINE_PROVIDER: &str = "command_line";
pub const BUILT_IN_RESOURCE_ROOT_ENV: &str = "TFROBOT_BUILT_IN_RESOURCE_ROOT";

pub(crate) fn configure_resource_root(resource_root: Option<&Path>) {
    match resource_root {
        Some(path) => std::env::set_var(BUILT_IN_RESOURCE_ROOT_ENV, path),
        None => std::env::remove_var(BUILT_IN_RESOURCE_ROOT_ENV),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CommandLineToolPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,
}

impl CommandLineToolPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(path) = self.workspace_root.as_ref() {
            if path.as_os_str().is_empty() {
                return Err("command line workspace must not be empty".to_string());
            }
            if !path.is_absolute() {
                return Err("command line workspace must be an absolute path".to_string());
            }
        }
        Ok(())
    }

    pub fn validate_existing_workspace(&self) -> Result<(), String> {
        self.validate()?;
        if let Some(path) = self.workspace_root.as_ref() {
            if !path.is_dir() {
                return Err(format!(
                    "command line workspace must be an existing directory: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    pub fn effective_workspace(&self, instance_storage_root: &Path) -> PathBuf {
        self.workspace_root
            .clone()
            .unwrap_or_else(|| instance_storage_root.join("workspace"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineRuntimeAssets {
    pub python: PathBuf,
    pub powershell: Option<PathBuf>,
}

impl CommandLineRuntimeAssets {
    pub fn discover() -> Result<Self, String> {
        let resource_root = std::env::var_os(BUILT_IN_RESOURCE_ROOT_ENV)
            .map(PathBuf::from)
            .ok_or_else(|| "built-in tool resource root is not configured".to_string())?;
        Self::from_resource_root(&resource_root)
    }

    pub fn from_resource_root(resource_root: &Path) -> Result<Self, String> {
        let runtime_root = resource_root.join("tfbash").join(target_runtime_name());
        #[cfg(target_os = "windows")]
        let python = runtime_root.join("python").join("python.exe");
        #[cfg(not(target_os = "windows"))]
        let python = runtime_root.join("python").join("bin").join("python3");

        #[cfg(target_os = "windows")]
        let powershell = Some(runtime_root.join("powershell").join("pwsh.exe"));
        #[cfg(not(target_os = "windows"))]
        let powershell = None;

        let assets = Self { python, powershell };
        assets.validate()?;
        Ok(assets)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.python.is_file() {
            return Err(format!(
                "bundled tfbash Python runtime is missing: {}",
                self.python.display()
            ));
        }
        if let Some(powershell) = self.powershell.as_ref() {
            if !powershell.is_file() {
                return Err(format!(
                    "bundled tfbash PowerShell runtime is missing: {}",
                    powershell.display()
                ));
            }
        }
        Ok(())
    }
}

pub fn command_line_server_config(
    policy: &CommandLineToolPolicy,
    instance_storage_root: &Path,
) -> Result<MCPServerConfig, String> {
    policy.validate_existing_workspace()?;
    let assets = CommandLineRuntimeAssets::discover()?;
    command_line_server_config_with_assets(policy, instance_storage_root, &assets)
}

pub fn command_line_server_config_with_assets(
    policy: &CommandLineToolPolicy,
    instance_storage_root: &Path,
    assets: &CommandLineRuntimeAssets,
) -> Result<MCPServerConfig, String> {
    policy.validate_existing_workspace()?;
    assets.validate()?;
    let workspace = policy.effective_workspace(instance_storage_root);
    std::fs::create_dir_all(&workspace).map_err(|error| {
        format!(
            "failed to create command line workspace {}: {error}",
            workspace.display()
        )
    })?;

    let mut arguments = vec![
        "-m".to_string(),
        "tfbash_mcp".to_string(),
        "--transport".to_string(),
        "stdio".to_string(),
        "--runtime-profile".to_string(),
        "auto".to_string(),
        "--host-profile".to_string(),
        "ide".to_string(),
        "--workspace-root".to_string(),
        workspace.to_string_lossy().into_owned(),
    ];
    if let Some(powershell) = assets.powershell.as_ref() {
        arguments.push("--shell".to_string());
        arguments.push(powershell.to_string_lossy().into_owned());
    }

    serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "TFRobot command line",
        "bundle_id": COMMAND_LINE_BUNDLE_ID,
        "disabled": false,
        "server_parameters": {
            "command": assets.python,
            "args": arguments,
            "env": { "PYTHONDONTWRITEBYTECODE": "1" }
        }
    }))
    .map_err(|error| format!("failed to build command line MCP config: {error}"))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const fn target_runtime_name() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const fn target_runtime_name() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const fn target_runtime_name() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const fn target_runtime_name() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
compile_error!("TFRC-125 does not define a bundled tfbash runtime for this target");

#[cfg(test)]
mod tests {
    use super::*;
    use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static RESOURCE_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn restore_resource_root(previous: Option<OsString>) {
        match previous {
            Some(value) => std::env::set_var(BUILT_IN_RESOURCE_ROOT_ENV, value),
            None => std::env::remove_var(BUILT_IN_RESOURCE_ROOT_ENV),
        }
    }

    #[test]
    fn resource_root_configuration_replaces_and_clears_stale_process_state() {
        let _guard = RESOURCE_ROOT_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os(BUILT_IN_RESOURCE_ROOT_ENV);
        std::env::set_var(BUILT_IN_RESOURCE_ROOT_ENV, "/stale/resources");

        configure_resource_root(None);
        let cleared = std::env::var_os(BUILT_IN_RESOURCE_ROOT_ENV);

        let resolved = Path::new("/resolved/resources");
        configure_resource_root(Some(resolved));
        let configured = std::env::var_os(BUILT_IN_RESOURCE_ROOT_ENV);

        restore_resource_root(previous);

        assert!(cleared.is_none());
        assert_eq!(configured, Some(resolved.as_os_str().to_os_string()));
    }

    #[test]
    fn custom_workspace_must_be_absolute() {
        let policy = CommandLineToolPolicy {
            enabled: true,
            workspace_root: Some(PathBuf::from("relative")),
        };
        assert!(policy.validate_existing_workspace().is_err());
    }

    #[test]
    fn custom_workspace_must_already_be_a_directory() {
        let temp = TempDir::new().unwrap();
        let policy = CommandLineToolPolicy {
            enabled: true,
            workspace_root: Some(temp.path().join("missing")),
        };
        assert!(policy.validate().is_ok());
        assert!(policy.validate_existing_workspace().is_err());

        let policy = CommandLineToolPolicy {
            enabled: true,
            workspace_root: Some(temp.path().to_path_buf()),
        };
        assert!(policy.validate_existing_workspace().is_ok());
    }

    #[test]
    fn builds_reserved_stdio_config_with_default_workspace() {
        let temp = TempDir::new().unwrap();
        let python = temp.path().join("python");
        std::fs::write(&python, "probe").unwrap();
        let assets = CommandLineRuntimeAssets {
            python: python.clone(),
            powershell: None,
        };
        let instance_root = temp.path().join("instance");
        let config = command_line_server_config_with_assets(
            &CommandLineToolPolicy {
                enabled: true,
                workspace_root: None,
            },
            &instance_root,
            &assets,
        )
        .unwrap();
        assert_eq!(resolve_bundle_id(&config).as_str(), COMMAND_LINE_BUNDLE_ID);
        let json = serde_json::to_value(config).unwrap();
        assert_eq!(
            json["server_parameters"]["command"],
            python.to_string_lossy().as_ref()
        );
        assert!(json["server_parameters"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == instance_root.join("workspace").to_string_lossy().as_ref()));
    }
}
