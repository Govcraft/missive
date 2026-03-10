use super::exit_code::ExitCode;

/// CLI-specific error type covering all failure modes for command-line operations.
#[derive(Debug)]
pub enum CliError {
    /// Configuration file could not be read or parsed.
    ConfigRead {
        /// Path to the configuration file that failed.
        path: String,
        /// Description of the read/parse failure.
        message: String,
    },

    /// Configuration validation failed.
    ConfigInvalid {
        /// Description of what is invalid.
        message: String,
    },

    /// File I/O operation failed.
    Io {
        /// What operation was attempted (e.g., "write config file").
        operation: String,
        /// Description of the I/O failure.
        message: String,
    },

    /// User cancelled the interactive setup wizard.
    SetupCancelled,

    /// The web server failed to start.
    ServeFailed {
        /// Description of the server startup failure.
        message: String,
    },
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigRead { path, message } => {
                write!(f, "failed to read config at '{path}': {message}")
            }
            Self::ConfigInvalid { message } => {
                write!(f, "invalid configuration: {message}")
            }
            Self::Io { operation, message } => {
                write!(f, "I/O error during {operation}: {message}")
            }
            Self::SetupCancelled => {
                write!(f, "setup cancelled by user")
            }
            Self::ServeFailed { message } => {
                write!(f, "server failed to start: {message}")
            }
        }
    }
}

impl std::error::Error for CliError {}

impl CliError {
    /// Maps each error variant to its corresponding process exit code.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::ConfigRead { .. } | Self::ConfigInvalid { .. } => ExitCode::CONFIG_ERROR,
            Self::Io { .. } => ExitCode::IO_ERROR,
            Self::SetupCancelled => ExitCode::SETUP_CANCELLED,
            Self::ServeFailed { .. } => ExitCode::SERVE_FAILED,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn display_config_read() {
        let err = CliError::ConfigRead {
            path: "/etc/missive/config.toml".to_string(),
            message: "file not found".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "failed to read config at '/etc/missive/config.toml': file not found"
        );
    }

    #[test]
    fn display_config_invalid() {
        let err = CliError::ConfigInvalid {
            message: "jmap_url is empty".to_string(),
        };
        assert_eq!(err.to_string(), "invalid configuration: jmap_url is empty");
    }

    #[test]
    fn display_io_error() {
        let err = CliError::Io {
            operation: "write config file".to_string(),
            message: "permission denied".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "I/O error during write config file: permission denied"
        );
    }

    #[test]
    fn display_setup_cancelled() {
        let err = CliError::SetupCancelled;
        assert_eq!(err.to_string(), "setup cancelled by user");
    }

    #[test]
    fn display_serve_failed() {
        let err = CliError::ServeFailed {
            message: "port 8080 already in use".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "server failed to start: port 8080 already in use"
        );
    }

    #[test]
    fn exit_code_config_read() {
        let err = CliError::ConfigRead {
            path: "config.toml".to_string(),
            message: "parse error".to_string(),
        };
        assert_eq!(err.exit_code(), ExitCode::CONFIG_ERROR);
    }

    #[test]
    fn exit_code_config_invalid() {
        let err = CliError::ConfigInvalid {
            message: "bad url".to_string(),
        };
        assert_eq!(err.exit_code(), ExitCode::CONFIG_ERROR);
    }

    #[test]
    fn exit_code_io() {
        let err = CliError::Io {
            operation: "create dir".to_string(),
            message: "denied".to_string(),
        };
        assert_eq!(err.exit_code(), ExitCode::IO_ERROR);
    }

    #[test]
    fn exit_code_setup_cancelled() {
        let err = CliError::SetupCancelled;
        assert_eq!(err.exit_code(), ExitCode::SETUP_CANCELLED);
    }

    #[test]
    fn exit_code_serve_failed() {
        let err = CliError::ServeFailed {
            message: "bind error".to_string(),
        };
        assert_eq!(err.exit_code(), ExitCode::SERVE_FAILED);
    }

    #[test]
    fn error_trait_source_returns_none() {
        use std::error::Error;

        let variants: Vec<CliError> = vec![
            CliError::ConfigRead {
                path: "x".to_string(),
                message: "y".to_string(),
            },
            CliError::ConfigInvalid {
                message: "z".to_string(),
            },
            CliError::Io {
                operation: "a".to_string(),
                message: "b".to_string(),
            },
            CliError::SetupCancelled,
            CliError::ServeFailed {
                message: "c".to_string(),
            },
        ];

        for err in &variants {
            assert!(
                err.source().is_none(),
                "source() should return None for {err}"
            );
        }
    }

    #[test]
    fn debug_output_is_non_empty() {
        let err = CliError::SetupCancelled;
        let debug = format!("{err:?}");
        assert!(!debug.is_empty());
    }
}
