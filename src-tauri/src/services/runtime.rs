use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Failed to get resource directory: {0}")]
    ResourceDir(String),
    #[error("Runtime not found: {0}")]
    NotFound(String),
}

pub struct RuntimePaths {
    pub node: PathBuf,
    pub python: PathBuf,
    pub uv: PathBuf,
    pub pnpm: PathBuf,
}

/// Resolves the root of resources declared by `tauri.conf.json` without relying on Tauri's
/// development-output-directory heuristic.
///
/// Tauri recognizes an unpackaged desktop binary as a development build only when the Cargo
/// output directory is named `target`. The signed development command intentionally uses
/// `target-signed-dev`, so `PathResolver::resource_dir` returns `UnknownPath` on macOS. Debug
/// builds can use the source resource tree directly; packaged builds must continue to use the
/// platform resource directory populated by Tauri bundling.
pub(crate) fn resolve_resource_root_for_build(
    packaged_resource_dir: Result<PathBuf, RuntimeError>,
    manifest_dir: &Path,
    debug_build: bool,
) -> Result<PathBuf, RuntimeError> {
    if debug_build {
        Ok(manifest_dir.join("resources"))
    } else {
        packaged_resource_dir.map(|path| path.join("resources"))
    }
}

pub(crate) fn resource_root_from_app(app: &AppHandle) -> Result<PathBuf, RuntimeError> {
    resolve_resource_root_for_build(
        app.path()
            .resource_dir()
            .map_err(|error| RuntimeError::ResourceDir(error.to_string())),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        cfg!(debug_assertions),
    )
}

impl RuntimePaths {
    pub fn from_app(app: &AppHandle) -> Result<Self, RuntimeError> {
        let resource_dir = resource_root_from_app(app)?;

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let (node_path, python_path) = (
            resource_dir.join("node/node-v20-darwin-arm64/bin/node"),
            resource_dir.join("python/python-3.11-darwin-arm64/bin/python3"),
        );

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        let (node_path, python_path) = (
            resource_dir.join("node/node-v20-darwin-x64/bin/node"),
            resource_dir.join("python/python-3.11-darwin-x64/bin/python3"),
        );

        #[cfg(target_os = "windows")]
        let (node_path, python_path) = (
            resource_dir.join("node/node-v20-win-x64/node.exe"),
            resource_dir.join("python/python-3.11-win-x64/python.exe"),
        );

        #[cfg(target_os = "linux")]
        let (node_path, python_path) = (
            resource_dir.join("node/node-v20-linux-x64/bin/node"),
            resource_dir.join("python/python-3.11-linux-x64/bin/python3"),
        );

        Ok(Self {
            node: node_path,
            python: python_path,
            uv: resource_dir.join("uv/uv"),
            pnpm: resource_dir.join("node/pnpm/bin/pnpm.cjs"),
        })
    }

    pub fn validate(&self) -> Result<(), RuntimeError> {
        if !self.node.exists() {
            return Err(RuntimeError::NotFound(format!(
                "Node.js not found at {:?}",
                self.node
            )));
        }
        if !self.python.exists() {
            return Err(RuntimeError::NotFound(format!(
                "Python not found at {:?}",
                self.python
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_resource_root_does_not_depend_on_tauri_target_directory_detection() {
        let manifest_dir = Path::new("/workspace/tfrobot-client/src-tauri");

        let resource_root = resolve_resource_root_for_build(
            Err(RuntimeError::ResourceDir("UnknownPath".to_string())),
            manifest_dir,
            true,
        )
        .expect("debug builds must resolve resources from the source tree");

        assert_eq!(resource_root, manifest_dir.join("resources"));
    }

    #[test]
    fn packaged_resource_root_uses_the_tauri_bundle_directory() {
        let packaged_resource_dir =
            PathBuf::from("/Applications/TFRobot Client.app/Contents/Resources");

        let resource_root = resolve_resource_root_for_build(
            Ok(packaged_resource_dir.clone()),
            Path::new("/workspace/tfrobot-client/src-tauri"),
            false,
        )
        .expect("packaged builds must use Tauri's resource directory");

        assert_eq!(resource_root, packaged_resource_dir.join("resources"));
    }

    #[test]
    fn packaged_resource_root_preserves_path_resolution_errors() {
        let error = resolve_resource_root_for_build(
            Err(RuntimeError::ResourceDir("UnknownPath".to_string())),
            Path::new("/workspace/tfrobot-client/src-tauri"),
            false,
        )
        .expect_err("packaged builds must not silently fall back to source resources");

        assert!(matches!(
            error,
            RuntimeError::ResourceDir(message) if message == "UnknownPath"
        ));
    }
}
