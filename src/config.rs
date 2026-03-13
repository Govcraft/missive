use acton_service::prelude::*;

use crate::jmap::JmapUrl;

fn default_page_size() -> usize {
    50
}

fn default_ping_interval() -> u32 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissiveConfig {
    pub jmap_url: JmapUrl,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub webhook: Option<WebhookConfig>,
}

impl Default for MissiveConfig {
    fn default() -> Self {
        Self {
            jmap_url: JmapUrl::default(),
            page_size: default_page_size(),
            webhook: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub jmap_username: String,
    #[serde(default)]
    pub jmap_password: String,
    #[serde(default)]
    pub include_body: bool,
    #[serde(default = "default_ping_interval")]
    pub ping_interval: u32,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::jmap::JmapUrl;

    #[test]
    fn toml_fallback_finds_jmap_url_at_root() {
        let raw = "jmap_url = \"https://mail.example.com\"\n\n[service]\nname = \"test\"\n";
        let table = raw.parse::<toml::Table>().unwrap();
        let url = table
            .get("jmap_url")
            .or_else(|| {
                table
                    .get("service")
                    .and_then(|v| v.as_table())
                    .and_then(|t| t.get("jmap_url"))
            })
            .and_then(|v| v.as_str());
        assert_eq!(url, Some("https://mail.example.com"));
    }

    #[test]
    fn toml_fallback_finds_jmap_url_under_service() {
        let raw = "[service]\nname = \"test\"\njmap_url = \"https://mail.example.com\"\n";
        let table = raw.parse::<toml::Table>().unwrap();
        let url = table
            .get("jmap_url")
            .or_else(|| {
                table
                    .get("service")
                    .and_then(|v| v.as_table())
                    .and_then(|t| t.get("jmap_url"))
            })
            .and_then(|v| v.as_str());
        assert_eq!(url, Some("https://mail.example.com"));
    }

    #[test]
    fn default_config_has_expected_values() {
        let config = MissiveConfig::default();
        assert!(config.jmap_url.is_empty());
        assert_eq!(config.page_size, 50);
    }

    #[test]
    fn jmap_url_from_str() {
        let url = JmapUrl::from("https://mail.example.com");
        assert_eq!(url.as_str(), "https://mail.example.com");
        assert!(!url.is_empty());
    }
}
