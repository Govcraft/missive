//! Command-line interface for Missive.
//!
//! Provides subcommands for running the server, interactive setup,
//! configuration validation, and config file generation.

use std::path::PathBuf;

use acton_service::prelude::Config;
use clap::{Parser, Subcommand};

pub mod config_gen;
pub mod error;
pub mod exit_code;
pub mod sanity;
pub mod setup;

use error::CliError;
use exit_code::ExitCode;

/// Missive -- a web-based email client using the JMAP protocol.
#[derive(Parser, Debug)]
#[command(name = "missive", version, about)]
pub struct Cli {
    /// Path to configuration file.
    #[arg(long, short, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Subcommand to run. Defaults to `serve` when omitted.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the Missive web server (default when no subcommand is given).
    Serve,

    /// Interactive first-run setup wizard.
    Setup,

    /// Validate configuration without starting the server.
    Sanity,

    /// Generate a starter config file.
    Config {
        /// Output file path. Prints to stdout if omitted.
        #[arg(short, long)]
        output: Option<String>,
    },
}

/// Dispatch the CLI command and return the appropriate exit code.
pub async fn dispatch(cli: Cli) -> ExitCode {
    let command = cli.command.unwrap_or(Command::Serve);

    let result = match command {
        Command::Serve => run_serve(cli.config).await,
        Command::Setup => run_setup().await,
        Command::Sanity => run_sanity(cli.config),
        Command::Config { output } => run_config(output),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            e.exit_code()
        }
    }
}

/// Start the Missive web server.
async fn run_serve(config_path: Option<PathBuf>) -> Result<(), CliError> {
    crate::serve(config_path).await
}

/// Run the interactive setup wizard.
async fn run_setup() -> Result<(), CliError> {
    setup::run_setup().await
}

/// Validate the current configuration.
fn run_sanity(config_path: Option<PathBuf>) -> Result<(), CliError> {
    let config = load_config(config_path)?;

    let report = sanity::check_config(&config);
    sanity::print_report(&report);

    if report.all_passed() {
        Ok(())
    } else {
        Err(CliError::ConfigInvalid {
            message: format!("{} check(s) failed", report.failure_count()),
        })
    }
}

/// Generate a starter config file.
fn run_config(output: Option<String>) -> Result<(), CliError> {
    let params = config_gen::ConfigParams {
        jmap_url: "https://mail.example.com".to_string(),
        service_port: 8080,
        page_size: 50,
        session_storage: config_gen::SessionStorageKind::Memory,
        redis_url: None,
    };

    let content = config_gen::generate_config_toml(&params);

    match output {
        Some(path) => {
            let path = std::path::PathBuf::from(&path);
            config_gen::write_config_file(&path, &content)?;
            println!("Starter config written to {}", path.display());
        }
        None => {
            print!("{content}");
        }
    }

    Ok(())
}

/// Load configuration, optionally from a specific path.
fn load_config(
    config_path: Option<PathBuf>,
) -> Result<Config<crate::config::MissiveConfig>, CliError> {
    match config_path {
        Some(path) => {
            let raw = std::fs::read_to_string(&path).map_err(|e| CliError::ConfigRead {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
            let table: toml::Table = raw.parse().map_err(|e: toml::de::Error| CliError::ConfigRead {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
            let mut config = Config::<crate::config::MissiveConfig>::default();

            if let Some(url) = table.get("jmap_url").and_then(|v| v.as_str()) {
                config.custom.jmap_url = crate::jmap::JmapUrl::from(url);
            }
            if let Some(ps) = table.get("page_size").and_then(|v| v.as_integer()) {
                config.custom.page_size = ps as usize;
            }
            if let Some(service) = table.get("service").and_then(|v| v.as_table()) {
                if let Some(name) = service.get("name").and_then(|v| v.as_str()) {
                    config.service.name = name.to_string();
                }
                if let Some(port) = service.get("port").and_then(|v| v.as_integer()) {
                    config.service.port = port as u16;
                }
            }

            Ok(config)
        }
        None => Config::<crate::config::MissiveConfig>::load().map_err(|e| CliError::ConfigRead {
            path: "config.toml".to_string(),
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn cli_parses_no_subcommand() {
        let cli = Cli::parse_from(["missive"]);
        assert!(cli.command.is_none());
        assert!(cli.config.is_none());
    }

    #[test]
    fn cli_parses_serve() {
        let cli = Cli::parse_from(["missive", "serve"]);
        assert!(matches!(cli.command, Some(Command::Serve)));
    }

    #[test]
    fn cli_parses_setup() {
        let cli = Cli::parse_from(["missive", "setup"]);
        assert!(matches!(cli.command, Some(Command::Setup)));
    }

    #[test]
    fn cli_parses_sanity() {
        let cli = Cli::parse_from(["missive", "sanity"]);
        assert!(matches!(cli.command, Some(Command::Sanity)));
    }

    #[test]
    fn cli_parses_config_no_output() {
        let cli = Cli::parse_from(["missive", "config"]);
        assert!(matches!(
            cli.command,
            Some(Command::Config { output: None })
        ));
    }

    #[test]
    fn cli_parses_config_with_output() {
        let cli = Cli::parse_from(["missive", "config", "--output", "/tmp/config.toml"]);
        match cli.command {
            Some(Command::Config { output }) => {
                assert_eq!(output.as_deref(), Some("/tmp/config.toml"));
            }
            other => panic!("expected Config command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_config_with_short_output() {
        let cli = Cli::parse_from(["missive", "config", "-o", "/tmp/config.toml"]);
        match cli.command {
            Some(Command::Config { output }) => {
                assert_eq!(output.as_deref(), Some("/tmp/config.toml"));
            }
            other => panic!("expected Config command, got {other:?}"),
        }
    }

    #[test]
    fn cli_parses_global_config_flag() {
        let cli = Cli::parse_from(["missive", "--config", "/etc/missive/config.toml", "serve"]);
        assert_eq!(
            cli.config,
            Some(PathBuf::from("/etc/missive/config.toml"))
        );
        assert!(matches!(cli.command, Some(Command::Serve)));
    }

    #[test]
    fn cli_parses_short_config_flag() {
        let cli = Cli::parse_from(["missive", "-c", "/tmp/config.toml", "sanity"]);
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/config.toml")));
        assert!(matches!(cli.command, Some(Command::Sanity)));
    }

    #[test]
    fn cli_parses_version_flag() {
        let result = Cli::try_parse_from(["missive", "--version"]);
        // --version causes clap to exit with an error (it's a special flag)
        assert!(result.is_err());
    }

    #[test]
    fn cli_rejects_unknown_subcommand() {
        let result = Cli::try_parse_from(["missive", "unknown"]);
        assert!(result.is_err());
    }
}
