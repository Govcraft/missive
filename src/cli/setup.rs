use std::io::IsTerminal;
use std::path::PathBuf;

use dialoguer::{Confirm, Input, Password, Select};
use secrecy::{ExposeSecret, SecretString};

use super::config_gen::{
    ConfigParams, SessionStorageKind, generate_config_toml, write_config_file,
};
use super::error::CliError;
use super::sanity;
use crate::jmap::JmapUrl;

/// Run the interactive setup wizard.
///
/// Prompts the user for configuration values (JMAP URL, port, session backend, etc.),
/// optionally verifies JMAP connectivity with credentials, generates a `config.toml`,
/// and writes it to the chosen path.
pub async fn run_setup() -> Result<(), CliError> {
    if !std::io::stdin().is_terminal() {
        return Err(CliError::ConfigInvalid {
            message: "setup requires an interactive terminal; create config.toml manually or use `missive config --output config.toml`".to_string(),
        });
    }

    println!();
    println!("Missive v{} — Setup Wizard", env!("CARGO_PKG_VERSION"));
    println!("==================================");
    println!();

    let jmap_url = prompt_jmap_url()?;
    let page_size = prompt_page_size("Emails per page", 50)?;
    let service_port = prompt_port("Service port", 8080)?;
    let session_storage = prompt_session_storage()?;

    let redis_url = if session_storage == SessionStorageKind::Redis {
        Some(prompt_input("Redis URL", "redis://localhost:6379")?)
    } else {
        None
    };

    // Optional: verify JMAP connectivity
    let verify = Confirm::new()
        .with_prompt("Verify JMAP connectivity? (requires username & password)")
        .default(true)
        .interact()
        .map_err(|_| CliError::SetupCancelled)?;

    if verify {
        let username = prompt_input("JMAP username", "")?;
        let password = prompt_password("JMAP password")?;

        println!();
        println!("Running JMAP connectivity checks...");

        let parsed_url = JmapUrl::parse(&jmap_url).map_err(|e| CliError::ConfigInvalid {
            message: format!("JMAP URL: {e}"),
        })?;

        let result =
            sanity::run_jmap_sanity_checks(&parsed_url, &username, password.expose_secret()).await;
        sanity::print_jmap_results(&result);

        let has_failures = !result.connection.passed()
            || !result.mailbox_fetch.passed()
            || !result.identity_fetch.passed()
            || result.required_mailboxes.iter().any(|c| !c.status.passed());

        if has_failures {
            println!();
            let proceed = Confirm::new()
                .with_prompt("Some checks failed. Continue with setup anyway?")
                .default(true)
                .interact()
                .map_err(|_| CliError::SetupCancelled)?;
            if !proceed {
                return Err(CliError::SetupCancelled);
            }
        } else {
            println!();
            println!("All JMAP checks passed.");
        }

        println!();
        println!("Note: Credentials are NOT saved to config. Set JMAP credentials in your");
        println!("      environment or log in through the web interface.");
    }

    let params = ConfigParams {
        jmap_url,
        service_port,
        page_size,
        session_storage,
        redis_url,
    };

    let content = generate_config_toml(&params);

    println!();
    println!("Generated configuration:");
    println!("------------------------");
    println!("{content}");

    let output_path = prompt_input("Config file path", "./config.toml")?;
    let path = PathBuf::from(&output_path);

    // Check for existing file
    if path.exists() {
        let overwrite = Confirm::new()
            .with_prompt(format!("{} already exists. Overwrite?", path.display()))
            .default(false)
            .interact()
            .map_err(|_| CliError::SetupCancelled)?;
        if !overwrite {
            return Err(CliError::SetupCancelled);
        }
    }

    write_config_file(&path, &content)?;

    println!();
    println!("Configuration written to {}", path.display());
    println!();
    println!("Next steps:");
    println!("  missive serve     Start the web server");
    println!("  missive sanity    Validate configuration");

    Ok(())
}

fn prompt_input(prompt: &str, default: &str) -> Result<String, CliError> {
    Input::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .interact_text()
        .map_err(|_| CliError::SetupCancelled)
}

fn prompt_jmap_url() -> Result<String, CliError> {
    Input::new()
        .with_prompt("JMAP server URL")
        .default("https://mail.example.com".to_string())
        .validate_with(|input: &String| -> Result<(), String> {
            JmapUrl::parse(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|_| CliError::SetupCancelled)
}

fn prompt_port(prompt: &str, default: u16) -> Result<u16, CliError> {
    let input: String = Input::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .validate_with(|input: &String| -> Result<(), String> {
            input
                .parse::<u16>()
                .map(|_| ())
                .map_err(|_| "enter a valid port number (1-65535)".to_string())
        })
        .interact_text()
        .map_err(|_| CliError::SetupCancelled)?;

    input.parse::<u16>().map_err(|_| CliError::ConfigInvalid {
        message: format!("invalid port: {input}"),
    })
}

fn prompt_page_size(prompt: &str, default: usize) -> Result<usize, CliError> {
    let input: String = Input::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .validate_with(|input: &String| -> Result<(), String> {
            match input.parse::<usize>() {
                Ok(n) if (1..=1000).contains(&n) => Ok(()),
                Ok(_) => Err("enter a value between 1 and 1000".to_string()),
                Err(_) => Err("enter a valid number".to_string()),
            }
        })
        .interact_text()
        .map_err(|_| CliError::SetupCancelled)?;

    input.parse::<usize>().map_err(|_| CliError::ConfigInvalid {
        message: format!("invalid page size: {input}"),
    })
}

fn prompt_password(prompt: &str) -> Result<SecretString, CliError> {
    let password = Password::new()
        .with_prompt(prompt)
        .interact()
        .map_err(|_| CliError::SetupCancelled)?;
    Ok(SecretString::from(password))
}

fn prompt_session_storage() -> Result<SessionStorageKind, CliError> {
    let items = &[
        "Memory (default, single-instance)",
        "Redis (persistent, multi-instance)",
    ];

    let selection = Select::new()
        .with_prompt("Session storage backend")
        .items(items)
        .default(0)
        .interact()
        .map_err(|_| CliError::SetupCancelled)?;

    match selection {
        0 => Ok(SessionStorageKind::Memory),
        _ => Ok(SessionStorageKind::Redis),
    }
}
