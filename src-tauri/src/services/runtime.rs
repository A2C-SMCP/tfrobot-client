use std::path::PathBuf;
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

impl RuntimePaths {
    pub fn from_app(app: &AppHandle) -> Result<Self, RuntimeError> {
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| RuntimeError::ResourceDir(e.to_string()))?;

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
