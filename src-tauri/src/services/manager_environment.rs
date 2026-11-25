use serde::{Deserialize, Serialize};

/// Canonical Manager deployments. Beta uses Production, never a separate credential scope.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ManagerEnvironment {
    Test,
    Staging,
    // Beta 当前就是正式环境，统一使用 prod 的服务地址和凭据作用域，不单独划分环境。
    #[default]
    #[serde(alias = "beta")]
    Prod,
}

impl ManagerEnvironment {
    pub const fn base_url(self) -> &'static str {
        match self {
            Self::Test => "https://rest.test.turingfocus.cn",
            Self::Staging => "https://api-staging.turingfocus.cn",
            Self::Prod => "https://api.turingfocus.cn",
        }
    }

    pub fn from_base_url(value: &str) -> Option<Self> {
        let normalized = value.trim().trim_end_matches('/');
        [Self::Test, Self::Staging, Self::Prod]
            .into_iter()
            .find(|environment| environment.base_url() == normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environments_have_canonical_manager_urls() {
        assert_eq!(
            ManagerEnvironment::Test.base_url(),
            "https://rest.test.turingfocus.cn"
        );
        assert_eq!(
            ManagerEnvironment::Staging.base_url(),
            "https://api-staging.turingfocus.cn"
        );
        assert_eq!(
            ManagerEnvironment::Prod.base_url(),
            "https://api.turingfocus.cn"
        );
        let beta: ManagerEnvironment = serde_json::from_str(r#""beta""#).unwrap();
        assert_eq!(beta, ManagerEnvironment::Prod);
        assert_eq!(serde_json::to_string(&beta).unwrap(), r#""prod""#);
        assert_eq!(
            ManagerEnvironment::from_base_url("https://api-beta.turingfocus.cn"),
            None
        );
    }

    #[test]
    fn legacy_urls_only_map_when_they_are_canonical() {
        assert_eq!(
            ManagerEnvironment::from_base_url("https://api-staging.turingfocus.cn/"),
            Some(ManagerEnvironment::Staging)
        );
        assert_eq!(
            ManagerEnvironment::from_base_url("https://rest.test.turingfocus.cn/"),
            Some(ManagerEnvironment::Test)
        );
        assert_eq!(
            ManagerEnvironment::from_base_url("https://example.com"),
            None
        );
    }

    #[test]
    fn development_config_preserves_the_installed_application_storage_identity() {
        let installed: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let development: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.dev.conf.json")).unwrap();
        assert_eq!(installed["identifier"], "com.tfrobot.client");
        // Tauri merges the dev overlay into the installed config. A different identifier would
        // silently select another data directory and keychain access identity on supported OSes.
        if let Some(identifier) = development.get("identifier") {
            assert_eq!(identifier, &installed["identifier"]);
        }
    }
}
