use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobalConfigFile {
    ChatPreferences,
    ManualTargets,
    ManagerSession,
}

pub const COMPUTER_PROFILE_FILE_NAME: &str = "profile.json";
pub const COMPUTER_INPUTS_FILE_NAME: &str = "inputs.json";
pub const SDK_CONTEXT_FILE_NAME: &str = "sdk_context.json";
pub const MIGRATION_STATE_FILE_NAME: &str = "migration_state.json";

impl GlobalConfigFile {
    fn file_name(self) -> &'static str {
        match self {
            Self::ChatPreferences => "chat_preferences.json",
            Self::ManualTargets => "manual_targets.json",
            Self::ManagerSession => "manager_session.json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ClientComputersPathError {
    #[error("invalid Computer instance directory id: {0}")]
    InvalidInstanceId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientComputersPaths {
    root: PathBuf,
}

impl ClientComputersPaths {
    pub fn from_app_data_dir(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join("client_computers"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn instances_root(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub(crate) fn instance_root(
        &self,
        instance_id: &str,
    ) -> Result<PathBuf, ClientComputersPathError> {
        validate_instance_directory_id(instance_id)?;
        Ok(self.instances_root().join(instance_id))
    }

    pub(crate) fn computer_profile(
        &self,
        instance_id: &str,
    ) -> Result<PathBuf, ClientComputersPathError> {
        validate_instance_directory_id(instance_id)?;
        Ok(self
            .instance_root(instance_id)?
            .join(COMPUTER_PROFILE_FILE_NAME))
    }

    pub(crate) fn sdk_context(
        &self,
        instance_id: &str,
    ) -> Result<PathBuf, ClientComputersPathError> {
        Ok(self.instance_root(instance_id)?.join(SDK_CONTEXT_FILE_NAME))
    }

    pub(crate) fn computer_inputs(
        &self,
        instance_id: &str,
    ) -> Result<PathBuf, ClientComputersPathError> {
        Ok(self
            .instance_root(instance_id)?
            .join(COMPUTER_INPUTS_FILE_NAME))
    }

    pub(crate) fn global_config(&self, artifact: GlobalConfigFile) -> PathBuf {
        self.root.join("global").join(artifact.file_name())
    }

    pub(crate) fn migration_state(&self) -> PathBuf {
        self.root.join(MIGRATION_STATE_FILE_NAME)
    }
}

fn validate_instance_directory_id(instance_id: &str) -> Result<(), ClientComputersPathError> {
    if instance_id.is_empty()
        || matches!(instance_id, "." | "..")
        || !instance_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(ClientComputersPathError::InvalidInstanceId(
            instance_id.to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_all_client_owned_paths_from_one_root() {
        let paths = ClientComputersPaths::from_app_data_dir(Path::new("/app-data"));

        assert_eq!(paths.root(), Path::new("/app-data/client_computers"));
        assert_eq!(
            paths.computer_profile("computer-a").unwrap(),
            Path::new("/app-data/client_computers/instances/computer-a/profile.json")
        );
        assert_eq!(
            paths.computer_inputs("computer-a").unwrap(),
            Path::new("/app-data/client_computers/instances/computer-a/inputs.json")
        );
    }

    #[test]
    fn rejects_instance_ids_that_can_escape_the_instances_root() {
        let paths = ClientComputersPaths::from_app_data_dir(Path::new("/app-data"));

        for instance_id in ["", ".", "..", "../outside", "nested/computer"] {
            assert!(matches!(
                paths.computer_profile(instance_id),
                Err(ClientComputersPathError::InvalidInstanceId(id)) if id == instance_id
            ));
        }
    }
}
