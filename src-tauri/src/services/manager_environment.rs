use serde::{Deserialize, Serialize};

/// TFRSManager deployment selected by the user.
///
/// URLs are intentionally owned by the Rust trust boundary. The webview only sends this enum,
/// so it cannot turn Manager login into an arbitrary outbound HTTP request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ManagerEnvironment {
    Staging,
    Beta,
    Prod,
}

impl ManagerEnvironment {
    pub const fn base_url(self) -> &'static str {
        match self {
            Self::Staging => "https://api-staging.turingfocus.cn",
            Self::Beta => "https://api-beta.turingfocus.cn",
            Self::Prod => "https://api.turingfocus.cn",
        }
    }

    pub fn from_base_url(value: &str) -> Option<Self> {
        let normalized = value.trim().trim_end_matches('/');
        [Self::Staging, Self::Beta, Self::Prod]
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
            ManagerEnvironment::Staging.base_url(),
            "https://api-staging.turingfocus.cn"
        );
        assert_eq!(
            ManagerEnvironment::Beta.base_url(),
            "https://api-beta.turingfocus.cn"
        );
        assert_eq!(
            ManagerEnvironment::Prod.base_url(),
            "https://api.turingfocus.cn"
        );
    }

    #[test]
    fn legacy_urls_only_map_when_they_are_canonical() {
        assert_eq!(
            ManagerEnvironment::from_base_url("https://api-staging.turingfocus.cn/"),
            Some(ManagerEnvironment::Staging)
        );
        assert_eq!(
            ManagerEnvironment::from_base_url("https://example.com"),
            None
        );
    }
}
