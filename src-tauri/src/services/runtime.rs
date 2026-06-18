use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};
use tauri::{AppHandle, Manager};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration};

use crate::commands::connection::{ConnectionProfile, ConnectionStatusInfo};
use crate::services::settings::AppSettings;
use crate::services::skill_manager::{self, SkillSyncSummary};
use crate::services::skills;
use smcp_computer::computer::{Computer, SilentSession};
use smcp_computer::mcp_clients::MCPServerConfig;
use smcp_computer::socketio_client::SmcpComputerClient;

pub type AppComputer = Computer<SilentSession>;
const SMCP_CONNECTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

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

#[derive(Debug, Clone)]
pub struct ConnectionMeta {
    pub profile_name: String,
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
}

pub struct ComputerRuntime {
    computer: StdRwLock<Arc<AppComputer>>,
    booted: StdRwLock<bool>,
    settings: StdRwLock<AppSettings>,
    configs: StdRwLock<Vec<MCPServerConfig>>,
    runtime_skill_home: PathBuf,
    connection: Arc<RwLock<Option<ConnectionMeta>>>,
    skill_sync_summary: Arc<RwLock<SkillSyncSummary>>,
    connect_lock: Mutex<()>,
}

impl ComputerRuntime {
    pub fn new(
        _computer_name: impl Into<String>,
        settings: &AppSettings,
        configs: Vec<MCPServerConfig>,
        runtime_skill_home: PathBuf,
    ) -> Self {
        let computer = Self::build_computer(
            settings.computer_name.clone(),
            "tfrobot-client-runtime".to_string(),
            configs.clone(),
            &runtime_skill_home,
            true,
            true,
        );

        Self {
            computer: StdRwLock::new(computer),
            booted: StdRwLock::new(false),
            settings: StdRwLock::new(settings.clone()),
            configs: StdRwLock::new(configs),
            runtime_skill_home,
            connection: Arc::new(RwLock::new(None)),
            skill_sync_summary: Arc::new(RwLock::new(SkillSyncSummary::default())),
            connect_lock: Mutex::new(()),
        }
    }

    fn build_computer(
        computer_name: impl Into<String>,
        session_id: String,
        configs: Vec<MCPServerConfig>,
        runtime_skill_home: &std::path::Path,
        auto_connect: bool,
        auto_reconnect: bool,
    ) -> Arc<AppComputer> {
        let computer_name = computer_name.into();
        let config_map = configs
            .into_iter()
            .map(|config| (config.name().to_string(), config))
            .collect();
        Arc::new(
            Computer::new(
                computer_name,
                SilentSession::new(session_id),
                None,
                Some(config_map),
                auto_connect,
                auto_reconnect,
            )
            .with_skill_home(runtime_skill_home.to_path_buf()),
        )
    }

    pub fn computer(&self) -> Arc<AppComputer> {
        Arc::clone(
            &self
                .computer
                .read()
                .expect("runtime computer lock poisoned"),
        )
    }

    fn stored_runtime_config(&self) -> (AppSettings, Vec<MCPServerConfig>) {
        let settings = self
            .settings
            .read()
            .expect("runtime settings lock poisoned")
            .clone();
        let configs = self
            .configs
            .read()
            .expect("runtime configs lock poisoned")
            .clone();
        (settings, configs)
    }

    fn store_runtime_config(&self, settings: &AppSettings, configs: Vec<MCPServerConfig>) {
        *self
            .settings
            .write()
            .expect("runtime settings lock poisoned") = settings.clone();
        self.store_configs(configs);
    }

    pub fn store_configs(&self, configs: Vec<MCPServerConfig>) {
        *self.configs.write().expect("runtime configs lock poisoned") = configs;
    }

    pub fn store_settings(&self, settings: &AppSettings) {
        *self
            .settings
            .write()
            .expect("runtime settings lock poisoned") = settings.clone();
    }

    #[cfg(test)]
    pub fn stored_config_count(&self) -> usize {
        self.configs
            .read()
            .expect("runtime configs lock poisoned")
            .len()
    }

    fn swap_computer(&self, computer: Arc<AppComputer>, booted: bool) {
        {
            let mut current = self
                .computer
                .write()
                .expect("runtime computer lock poisoned");
            *current = computer;
        }
        *self.booted.write().expect("runtime boot lock poisoned") = booted;
    }

    pub async fn boot(&self) -> Result<(), String> {
        let _guard = self.connect_lock.lock().await;
        self.boot_current_unlocked().await
    }

    pub async fn reconfigure(
        &self,
        settings: &AppSettings,
        configs: Vec<MCPServerConfig>,
    ) -> Result<SkillSyncSummary, String> {
        let _guard = self.connect_lock.lock().await;

        let previous = self.computer();
        let computer = Self::build_computer(
            settings.computer_name.clone(),
            "tfrobot-client-runtime".to_string(),
            configs.clone(),
            &self.runtime_skill_home,
            true,
            true,
        );
        computer.boot_up().await.map_err(|e| e.to_string())?;

        let summary = match self
            .stage_and_summarize_current_skills(settings, &computer)
            .await
        {
            Ok(summary) => summary,
            Err(e) => {
                Self::shutdown_computer(&computer).await;
                return Err(e);
            }
        };

        let previous_was_connected = self.connection.read().await.is_some();
        if previous_was_connected {
            Self::leave_office(&previous).await;
        }
        Self::shutdown_computer(&previous).await;
        self.swap_computer(Arc::clone(&computer), true);
        self.store_runtime_config(settings, configs);
        if previous_was_connected {
            let mut connection = self.connection.write().await;
            *connection = None;
        }
        Ok(summary)
    }

    async fn boot_current_unlocked(&self) -> Result<(), String> {
        if *self.booted.read().expect("runtime boot lock poisoned") {
            return Ok(());
        }
        let computer = self.computer();
        if !computer.is_mcp_manager_initialized().await {
            computer.boot_up().await.map_err(|e| e.to_string())?;
        }
        let (settings, _) = self.stored_runtime_config();
        self.stage_and_summarize_current_skills(&settings, &computer)
            .await?;
        *self.booted.write().expect("runtime boot lock poisoned") = true;
        Ok(())
    }

    pub async fn connect(
        &self,
        profile: &ConnectionProfile,
        api_key: &Option<String>,
        settings: &AppSettings,
        configs: Vec<MCPServerConfig>,
    ) -> Result<SkillSyncSummary, String> {
        let _guard = self.connect_lock.lock().await;

        self.boot_current_unlocked().await?;
        self.store_runtime_config(settings, configs);
        let computer = self.computer();
        let mcp_synced = match self.stage_current_skills(settings, &computer).await {
            Ok(mcp_synced) => mcp_synced,
            Err(e) => return Err(e),
        };

        let headers = encode_headers(&profile.headers);
        let previous_client = Self::current_socket_client(&computer).await;

        if let Err(e) = computer
            .connect_socketio(&profile.url, &profile.namespace, api_key, &headers)
            .await
        {
            return Err(e.to_string());
        }

        if let Err(e) = computer
            .join_office(&profile.office_id, &profile.computer_name)
            .await
        {
            Self::disconnect_socket(&computer).await;
            if let Some(previous_client) = previous_client {
                computer.set_socketio_client(previous_client).await;
            }
            return Err(e.to_string());
        }

        let summary = match self
            .summarize_and_store_current_skills(settings, mcp_synced, &computer)
            .await
        {
            Ok(summary) => summary,
            Err(e) => {
                Self::disconnect_socket(&computer).await;
                if let Some(previous_client) = previous_client {
                    computer.set_socketio_client(previous_client).await;
                }
                return Err(e);
            }
        };

        let _ = computer.emit_update_skills_now().await;

        if let Some(previous_client) = previous_client {
            Self::leave_and_close_smcp_client(previous_client, "previous SMCP profile").await;
        }

        {
            let mut connection = self.connection.write().await;
            *connection = Some(ConnectionMeta {
                profile_name: profile.name.clone(),
                url: profile.url.clone(),
                office_id: profile.office_id.clone(),
                computer_name: profile.computer_name.clone(),
                connected_at: chrono::Utc::now(),
            });
        }
        Ok(summary)
    }

    pub async fn refresh_skill_sync_summary(
        &self,
        settings: &AppSettings,
    ) -> Result<SkillSyncSummary, String> {
        let _guard = self.connect_lock.lock().await;
        let computer = self.computer();
        let summary = self
            .stage_and_summarize_current_skills(settings, &computer)
            .await?;
        computer.emit_update_skills_now().await;
        Ok(summary)
    }

    async fn stage_and_summarize_current_skills(
        &self,
        settings: &AppSettings,
        computer: &Arc<AppComputer>,
    ) -> Result<SkillSyncSummary, String> {
        let mcp_synced = self.stage_current_skills(settings, computer).await?;
        self.summarize_and_store_current_skills(settings, mcp_synced, computer)
            .await
    }

    async fn stage_current_skills(
        &self,
        settings: &AppSettings,
        computer: &Arc<AppComputer>,
    ) -> Result<usize, String> {
        skill_manager::stage_configured_user_skills(&settings.skills_root_dir, computer)
            .await
            .map_err(|e| e.to_string())?;
        Ok(computer.restage_mcp_skills(None).await.len())
    }

    async fn summarize_and_store_current_skills(
        &self,
        settings: &AppSettings,
        mcp_synced: usize,
        computer: &Arc<AppComputer>,
    ) -> Result<SkillSyncSummary, String> {
        let active_refs = computer.get_skills().await;
        let summary = skill_manager::summarize_user_skills(
            &settings.skills_root_dir,
            &active_refs,
            mcp_synced,
        )
        .map_err(|e| e.to_string())?;
        let mut latest = self.skill_sync_summary.write().await;
        *latest = summary.clone();
        Ok(summary)
    }

    pub async fn disconnect(&self) {
        let _guard = self.connect_lock.lock().await;
        self.disconnect_current_unlocked().await;
    }

    async fn disconnect_current_unlocked(&self) {
        let computer = self.computer();
        let was_connected = self.connection.read().await.is_some();
        if was_connected {
            Self::leave_office(&computer).await;
        }
        Self::disconnect_socket(&computer).await;
        let mut connection = self.connection.write().await;
        *connection = None;
    }

    async fn leave_office(computer: &Arc<AppComputer>) {
        match timeout(SMCP_CONNECTION_CLOSE_TIMEOUT, computer.leave_office()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("Error leaving SMCP office: {}", e),
            Err(_) => log::warn!(
                "Timed out leaving SMCP office after {:?}",
                SMCP_CONNECTION_CLOSE_TIMEOUT
            ),
        }
    }

    async fn disconnect_socket(computer: &Arc<AppComputer>) {
        let client = {
            let socketio_client = computer.get_socketio_client();
            let mut guard = socketio_client.write().await;
            guard.take()
        };

        let Some(client) = client else {
            if let Err(e) = computer.disconnect_socketio().await {
                log::warn!("Error clearing SMCP socket reference: {}", e);
            }
            return;
        };

        Self::close_smcp_client(client, "from SMCP server").await;
    }

    async fn current_socket_client(computer: &Arc<AppComputer>) -> Option<Arc<SmcpComputerClient>> {
        let socketio_client = computer.get_socketio_client();
        let guard = socketio_client.read().await;
        guard.clone()
    }

    async fn leave_and_close_smcp_client(client: Arc<SmcpComputerClient>, context: &str) {
        match client.get_current_office_id().await {
            Ok(office_id) => {
                if let Err(e) = client.leave_office(&office_id).await {
                    log::warn!("Error leaving SMCP office for {}: {}", context, e);
                }
            }
            Err(e) => log::warn!("Error reading SMCP office for {}: {}", context, e),
        }
        Self::close_smcp_client(client, context).await;
    }

    async fn close_smcp_client(client: Arc<SmcpComputerClient>, context: &str) {
        match Arc::try_unwrap(client) {
            Ok(client) => match timeout(SMCP_CONNECTION_CLOSE_TIMEOUT, client.disconnect()).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("Error disconnecting {}: {}", context, e),
                Err(_) => {
                    log::warn!(
                        "Timed out disconnecting {} after {:?}",
                        context,
                        SMCP_CONNECTION_CLOSE_TIMEOUT,
                    );
                }
            },
            Err(client) => {
                log::warn!(
                    "Cannot actively disconnect SMCP socket; {} references remain",
                    Arc::strong_count(&client)
                );
            }
        }
    }

    async fn shutdown_computer(computer: &Arc<AppComputer>) {
        Self::disconnect_socket(computer).await;
        if let Err(e) = computer.shutdown().await {
            log::warn!("Error shutting down app computer: {}", e);
        }
    }

    pub async fn shutdown(&self) {
        self.disconnect().await;
        Self::shutdown_computer(&self.computer()).await;
        *self.booted.write().expect("runtime boot lock poisoned") = false;
    }

    pub async fn connection_status(&self) -> ConnectionStatusInfo {
        let connection = self.connection.read().await;
        match connection.as_ref() {
            Some(c) => ConnectionStatusInfo {
                connected: true,
                url: Some(c.url.clone()),
                office_id: Some(c.office_id.clone()),
                computer_name: Some(c.computer_name.clone()),
                connected_at: Some(c.connected_at.to_rfc3339()),
                profile_name: Some(c.profile_name.clone()),
            },
            None => ConnectionStatusInfo {
                connected: false,
                url: None,
                office_id: None,
                computer_name: None,
                connected_at: None,
                profile_name: None,
            },
        }
    }

    pub async fn skill_sync_summary(&self) -> SkillSyncSummary {
        self.skill_sync_summary.read().await.clone()
    }

    pub fn local_skill_root(&self) -> PathBuf {
        skills::expand_home(
            &self
                .settings
                .read()
                .expect("runtime settings lock poisoned")
                .skills_root_dir,
        )
    }

    pub fn computer_name(&self) -> String {
        self.computer().name().to_string()
    }
}

fn encode_headers(headers: &std::collections::HashMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }

    let mut pairs: Vec<_> = headers.iter().collect();
    pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
    Some(
        pairs
            .into_iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect::<Vec<_>>()
            .join(","),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn settings_for_skill_home(home: &std::path::Path) -> AppSettings {
        AppSettings {
            skills_root_dir: home.to_string_lossy().to_string(),
            ..AppSettings::default()
        }
    }

    fn write_user_skill(home: &std::path::Path, name: &str) {
        let skill_dir = home.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name}\n---\n# {name}\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn reconfigure_rebuilds_computer_with_updated_skill_home() {
        let tmp = TempDir::new().unwrap();
        let old_home = tmp.path().join("old-home");
        let new_home = tmp.path().join("new-home");
        let runtime_home = tmp.path().join("runtime-skill-home");
        write_user_skill(&old_home, "old-skill");
        write_user_skill(&new_home, "new-skill");

        let old_settings = settings_for_skill_home(&old_home);
        let runtime = ComputerRuntime::new(
            "tfrobot-client",
            &old_settings,
            Vec::new(),
            runtime_home.clone(),
        );
        runtime.boot().await.unwrap();
        assert_eq!(runtime.computer().skill_home(), runtime_home);
        let old_summary = runtime
            .refresh_skill_sync_summary(&old_settings)
            .await
            .unwrap();
        assert_eq!(old_summary.local_synced, 1);
        assert!(runtime
            .computer()
            .get_skills()
            .await
            .iter()
            .any(|skill| skill.name == "old-skill"));

        let new_settings = settings_for_skill_home(&new_home);
        let new_summary = runtime
            .reconfigure(&new_settings, Vec::new())
            .await
            .unwrap();

        assert_eq!(runtime.local_skill_root(), new_home);
        assert_eq!(runtime.computer().skill_home(), runtime_home);
        assert_eq!(new_summary.local_synced, 1);
        assert!(new_summary.skipped.is_empty());
        let active_names: Vec<_> = runtime
            .computer()
            .get_skills()
            .await
            .into_iter()
            .map(|skill| skill.name)
            .collect();
        assert!(active_names.contains(&"new-skill".to_string()));
        assert!(!active_names.contains(&"old-skill".to_string()));

        runtime.shutdown().await;
    }
}
