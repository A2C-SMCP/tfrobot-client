use serde::{Deserialize, Serialize};

/// Canonical Manager deployments. Beta uses Production, never a separate credential scope.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ManagerEnvironment {
    // Retain the old Test identity for saved sessions and Computer bindings.
    Test,
    Staging,
    // Beta 当前就是正式环境，统一使用 prod 的服务地址和凭据作用域，不单独划分环境。
    #[default]
    #[serde(alias = "beta")]
    Prod,
    Custom(ManagerBaseUrl),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String")]
pub struct ManagerBaseUrl(String);

impl TryFrom<String> for ManagerBaseUrl {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let input = value.trim();
        let error =
            "Use an absolute HTTP or HTTPS Manager URL without credentials, a query, or a fragment";
        let url = url::Url::parse(input).map_err(|_| error)?;
        if !input
            .split_once("://")
            .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(url.scheme()))
            || !matches!(url.scheme(), "http" | "https")
            || !url.has_host()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(error);
        }
        Ok(Self(url.as_str().trim_end_matches('/').to_owned()))
    }
}

impl ManagerEnvironment {
    pub(crate) fn preserve_legacy_test_identity(self) -> Self {
        if matches!(&self, Self::Custom(url) if url.0 == Self::Test.base_url()) {
            return Self::Test;
        }
        self
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Test | Self::Custom(_) => "custom",
            Self::Staging => "staging",
            Self::Prod => "prod",
        }
    }

    pub fn base_url(&self) -> &str {
        match self {
            Self::Test => "https://rest.test.turingfocus.cn",
            Self::Staging => "https://api-staging.turingfocus.cn",
            Self::Prod => "https://api.turingfocus.cn",
            Self::Custom(url) => &url.0,
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
    fn custom_environment_normalizes_urls_and_rejects_invalid_addresses() {
        let custom = serde_json::json!({ "custom": " HTTPS://MANAGER.EXAMPLE.COM:443/gateway/ " });
        let environment: ManagerEnvironment = serde_json::from_value(custom).unwrap();
        assert_eq!(
            environment.base_url(),
            "https://manager.example.com/gateway"
        );
        assert_eq!(
            serde_json::to_value(environment).unwrap(),
            serde_json::json!({ "custom": "https://manager.example.com/gateway" })
        );

        for invalid in [
            "",
            "manager.example.com",
            "ftp://manager.example.com",
            "https://user:password@manager.example.com",
            "https://manager.example.com?",
            "https://manager.example.com#",
        ] {
            let value = serde_json::json!({ "custom": invalid });
            assert!(serde_json::from_value::<ManagerEnvironment>(value).is_err());
        }
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
