use std::fmt;
use std::path::Path;

use super::error::CliError;

/// Session storage backend selection for config generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStorageKind {
    /// In-memory session storage (lost on restart, single-instance only).
    Memory,
    /// Redis-backed session storage (persistent, multi-instance).
    Redis,
}

impl fmt::Display for SessionStorageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory => write!(f, "memory"),
            Self::Redis => write!(f, "redis"),
        }
    }
}

/// Parameters for generating a `config.toml` file.
#[derive(Debug, Clone)]
pub struct ConfigParams {
    /// JMAP server URL (e.g., "https://mail.example.com").
    pub jmap_url: String,
    /// HTTP port for the Missive web server.
    pub service_port: u16,
    /// Number of emails to fetch per page.
    pub page_size: usize,
    /// Session storage backend.
    pub session_storage: SessionStorageKind,
    /// Redis URL, required when `session_storage` is `Redis`.
    pub redis_url: Option<String>,
}

/// Generate a `config.toml` string from the given parameters.
///
/// This is a pure function with no I/O side effects.
///
/// # Examples
///
/// ```
/// use missive::cli::config_gen::{ConfigParams, SessionStorageKind, generate_config_toml};
///
/// let params = ConfigParams {
///     jmap_url: "https://mail.example.com".to_string(),
///     service_port: 8080,
///     page_size: 50,
///     session_storage: SessionStorageKind::Memory,
///     redis_url: None,
/// };
/// let toml = generate_config_toml(&params);
/// assert!(toml.contains("jmap_url"));
/// ```
pub fn generate_config_toml(params: &ConfigParams) -> String {
    let mut lines = Vec::new();

    lines.push(format!("jmap_url = \"{}\"", params.jmap_url));
    lines.push(format!("page_size = {}", params.page_size));
    lines.push(String::new());
    lines.push("[service]".to_string());
    lines.push("name = \"missive\"".to_string());
    lines.push(format!("port = {}", params.service_port));

    match params.session_storage {
        SessionStorageKind::Memory => {
            // No [session] section needed for memory storage (it's the default)
        }
        SessionStorageKind::Redis => {
            lines.push(String::new());
            lines.push("[session]".to_string());
            lines.push("storage = \"redis\"".to_string());
            if let Some(ref url) = params.redis_url {
                lines.push(format!("redis_url = \"{url}\""));
            }
        }
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Write the given config content to a file, creating parent directories as needed.
pub fn write_config_file(path: &Path, content: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Io {
            operation: format!("create directory '{}'", parent.display()),
            message: e.to_string(),
        })?;
    }

    std::fs::write(path, content).map_err(|e| CliError::Io {
        operation: format!("write config file '{}'", path.display()),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn memory_params() -> ConfigParams {
        ConfigParams {
            jmap_url: "https://mail.example.com".to_string(),
            service_port: 8080,
            page_size: 50,
            session_storage: SessionStorageKind::Memory,
            redis_url: None,
        }
    }

    fn redis_params() -> ConfigParams {
        ConfigParams {
            jmap_url: "https://jmap.fastmail.com".to_string(),
            service_port: 9090,
            page_size: 25,
            session_storage: SessionStorageKind::Redis,
            redis_url: Some("redis://localhost:6379".to_string()),
        }
    }

    #[test]
    fn memory_config_contains_jmap_url() {
        let toml = generate_config_toml(&memory_params());
        assert!(toml.contains("jmap_url = \"https://mail.example.com\""));
    }

    #[test]
    fn memory_config_contains_service_section() {
        let toml = generate_config_toml(&memory_params());
        assert!(toml.contains("[service]"));
        assert!(toml.contains("name = \"missive\""));
        assert!(toml.contains("port = 8080"));
    }

    #[test]
    fn memory_config_contains_page_size() {
        let toml = generate_config_toml(&memory_params());
        assert!(toml.contains("page_size = 50"));
    }

    #[test]
    fn memory_config_omits_session_section() {
        let toml = generate_config_toml(&memory_params());
        assert!(!toml.contains("[session]"));
        assert!(!toml.contains("redis_url"));
    }

    #[test]
    fn redis_config_includes_session_section() {
        let toml = generate_config_toml(&redis_params());
        assert!(toml.contains("[session]"));
        assert!(toml.contains("storage = \"redis\""));
        assert!(toml.contains("redis_url = \"redis://localhost:6379\""));
    }

    #[test]
    fn redis_config_contains_custom_port() {
        let toml = generate_config_toml(&redis_params());
        assert!(toml.contains("port = 9090"));
    }

    #[test]
    fn redis_config_contains_custom_page_size() {
        let toml = generate_config_toml(&redis_params());
        assert!(toml.contains("page_size = 25"));
    }

    #[test]
    fn generated_toml_parses_as_valid_toml() {
        let toml_str = generate_config_toml(&memory_params());
        let parsed: Result<toml::Table, _> = toml_str.parse();
        assert!(parsed.is_ok(), "Generated TOML should be valid");
    }

    #[test]
    fn generated_redis_toml_parses_as_valid_toml() {
        let toml_str = generate_config_toml(&redis_params());
        let parsed: Result<toml::Table, _> = toml_str.parse();
        assert!(parsed.is_ok(), "Generated Redis TOML should be valid");
    }

    #[test]
    fn round_trip_memory_config_values() {
        let toml_str = generate_config_toml(&memory_params());
        let table: toml::Table = toml_str.parse().unwrap();

        assert_eq!(
            table.get("jmap_url").and_then(|v| v.as_str()),
            Some("https://mail.example.com")
        );
        assert_eq!(
            table.get("page_size").and_then(|v| v.as_integer()),
            Some(50)
        );

        let service = table.get("service").and_then(|v| v.as_table()).unwrap();
        assert_eq!(
            service.get("name").and_then(|v| v.as_str()),
            Some("missive")
        );
        assert_eq!(service.get("port").and_then(|v| v.as_integer()), Some(8080));
    }

    #[test]
    fn round_trip_redis_config_values() {
        let toml_str = generate_config_toml(&redis_params());
        let table: toml::Table = toml_str.parse().unwrap();

        let session = table.get("session").and_then(|v| v.as_table()).unwrap();
        assert_eq!(
            session.get("storage").and_then(|v| v.as_str()),
            Some("redis")
        );
        assert_eq!(
            session.get("redis_url").and_then(|v| v.as_str()),
            Some("redis://localhost:6379")
        );
    }

    #[test]
    fn session_storage_kind_display() {
        assert_eq!(SessionStorageKind::Memory.to_string(), "memory");
        assert_eq!(SessionStorageKind::Redis.to_string(), "redis");
    }

    #[test]
    fn write_config_file_creates_file_in_temp_dir() {
        let dir = std::env::temp_dir().join("missive_test_config_gen");
        let path = dir.join("config.toml");

        // Clean up from any previous run
        let _ = std::fs::remove_dir_all(&dir);

        let content = generate_config_toml(&memory_params());
        write_config_file(&path, &content).unwrap();

        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, content);

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_config_file_returns_error_for_invalid_path() {
        let path = Path::new("/nonexistent_root_dir_zzzz/config.toml");
        let result = write_config_file(path, "test");
        assert!(result.is_err());
    }

    #[test]
    fn redis_without_url_omits_redis_url_line() {
        let params = ConfigParams {
            jmap_url: "https://mail.example.com".to_string(),
            service_port: 8080,
            page_size: 50,
            session_storage: SessionStorageKind::Redis,
            redis_url: None,
        };
        let toml = generate_config_toml(&params);
        assert!(toml.contains("[session]"));
        assert!(toml.contains("storage = \"redis\""));
        assert!(!toml.contains("redis_url"));
    }
}
