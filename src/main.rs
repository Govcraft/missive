#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use acton_service::prelude::*;
use acton_service::session::{
    SessionManagerLayer, SessionStorage, create_memory_session_layer, create_redis_session_layer,
};
use axum::Extension;
use clap::Parser;

use crate::jmap::{JmapUrl, new_client_cache};

mod assets;
pub mod cli;
mod config;
mod contacts;
mod error;
mod jmap;
mod routes;
mod sanitize;
mod session;
mod webhook;

use config::MissiveConfig;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = cli::Cli::parse();
    cli::dispatch(cli).await.into()
}

/// Start the Missive web server.
///
/// Loads configuration, creates session layers, builds routes,
/// and starts the HTTP server. This is the main server entry point
/// called by the `serve` CLI subcommand.
pub async fn serve(
    config_path: Option<std::path::PathBuf>,
) -> std::result::Result<(), cli::error::CliError> {
    use cli::error::CliError;

    info!("Missive v{}", env!("CARGO_PKG_VERSION"));

    let client_cache = new_client_cache();
    let broadcaster = Arc::new(SseBroadcaster::new());

    let mut config = match config_path {
        Some(ref path) => {
            let path_str = path.to_string_lossy();
            info!("Loading config from {path_str}");
            Config::<MissiveConfig>::load_from(&path_str).map_err(|e| CliError::ServeFailed {
                message: format!("failed to load config from '{path_str}': {e}"),
            })?
        }
        None => Config::<MissiveConfig>::load().map_err(|e| CliError::ServeFailed {
            message: format!("failed to load config: {e}"),
        })?,
    };
    info!(
        "Loaded config: service={}, jmap_url={}",
        config.service.name, config.custom.jmap_url
    );

    if config.custom.jmap_url.is_empty() {
        // Fallback: figment doesn't properly deserialize #[serde(flatten)] fields.
        // Also check inside [service] table in case jmap_url was placed there.
        if let Ok(raw) = std::fs::read_to_string("config.toml")
            && let Ok(table) = raw.parse::<toml::Table>()
        {
            let url = table
                .get("jmap_url")
                .or_else(|| {
                    table
                        .get("service")
                        .and_then(|v| v.as_table())
                        .and_then(|t| t.get("jmap_url"))
                })
                .and_then(|v| v.as_str());
            if let Some(url) = url {
                config.custom.jmap_url = JmapUrl::from(url);
                info!("Loaded jmap_url from config.toml: {url}");
            }
        }
    }

    if let Err(e) = config.custom.jmap_url.validate() {
        error!("Invalid JMAP URL in config: {e}");
        return Err(CliError::ConfigInvalid {
            message: format!("JMAP URL: {e}"),
        });
    }

    let session_config = config.session.clone().unwrap_or_default();

    let routes = match session_config.storage {
        SessionStorage::Redis => {
            let redis_url =
                session_config
                    .redis_url
                    .as_deref()
                    .ok_or_else(|| CliError::ConfigInvalid {
                        message: "session.redis_url is required when session.storage = \"redis\""
                            .to_string(),
                    })?;
            info!("Using Redis session backend");
            let layer = create_redis_session_layer(&session_config, redis_url)
                .await
                .map_err(|e| CliError::ServeFailed {
                    message: format!("failed to create Redis session layer: {e}"),
                })?;
            build_routes(client_cache, broadcaster, layer)
        }
        SessionStorage::Memory => {
            info!("Using in-memory session backend");
            let layer = create_memory_session_layer(&session_config);
            build_routes(client_cache, broadcaster, layer)
        }
    };

    let service = ServiceBuilder::<MissiveConfig>::new()
        .with_config(config.clone())
        .with_routes(routes)
        .build();

    // Submit webhook worker if configured (between build and serve)
    if let Some(ref webhook_config) = config.custom.webhook {
        info!("Webhook worker enabled, target: {}", webhook_config.url);

        if webhook_config.jmap_username.is_empty() || webhook_config.jmap_password.is_empty() {
            return Err(CliError::ConfigInvalid {
                message: "webhook.jmap_username and webhook.jmap_password are required \
                          (set ACTON_WEBHOOK_JMAP_USERNAME and ACTON_WEBHOOK_JMAP_PASSWORD)"
                    .to_string(),
            });
        }

        let worker = service.state().background_worker().ok_or_else(|| {
            CliError::ConfigInvalid {
                message: "BackgroundWorker not available; \
                          enable [background_worker] in config"
                    .to_string(),
            }
        })?;

        let jmap_url = config.custom.jmap_url.clone();
        let wh_config = webhook_config.clone();

        worker
            .submit("jmap-webhook", || async move {
                webhook::run_webhook_worker(jmap_url, wh_config).await
            })
            .await;
    }

    service.serve().await.map_err(|e| CliError::ServeFailed {
        message: e.to_string(),
    })
}

fn build_routes<S>(
    client_cache: jmap::JmapClientCache,
    broadcaster: Arc<SseBroadcaster>,
    session_layer: SessionManagerLayer<S>,
) -> VersionedRoutes<MissiveConfig>
where
    SessionManagerLayer<S>: Clone + Send + Sync + 'static,
    S: tower_sessions_core::session_store::SessionStore + Clone + Send + Sync + 'static,
{
    VersionedApiBuilder::<MissiveConfig>::with_config()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/mailboxes", get(routes::mailboxes::list_mailboxes))
                .route("/emails", get(routes::emails::list_emails))
                .route("/emails/bulk", post(routes::emails::bulk_action))
                .route(
                    "/emails/{id}",
                    get(routes::emails::get_email).delete(routes::emails::delete_email),
                )
                .route(
                    "/emails/{id}/mark-unread",
                    post(routes::emails::mark_unread),
                )
                .route(
                    "/emails/{id}/toggle-flag",
                    post(routes::emails::toggle_flag),
                )
                .route("/emails/{id}/reply", get(routes::emails::reply))
                .route("/emails/{id}/reply-all", get(routes::emails::reply_all))
                .route("/emails/{id}/forward", get(routes::emails::forward))
                .route("/emails/{id}/archive", post(routes::emails::archive_email))
                .route("/emails/{id}/spam", post(routes::emails::spam_email))
                .route("/emails/{id}/unspam", post(routes::emails::unspam_email))
                .route("/emails/{id}/move", post(routes::emails::move_email))
                .route(
                    "/attachments/{blob_id}",
                    get(routes::emails::download_attachment),
                )
                .route("/compose", get(routes::emails::compose_form))
                .route("/compose/cancel", get(routes::emails::compose_cancel))
                .route("/compose/upload", post(routes::emails::upload_attachment))
                .route("/send", post(routes::emails::send_email))
                .route("/drafts", post(routes::emails::save_draft))
                .route("/flash", get(routes::emails::get_flash))
                .route(
                    "/contacts",
                    get(routes::contacts::list_contacts)
                        .post(routes::contacts::create_contact),
                )
                .route("/contacts/new", get(routes::contacts::new_contact_form))
                .route("/contacts/cancel", get(routes::contacts::cancel_form))
                .route(
                    "/contacts/{id}",
                    get(routes::contacts::get_contact)
                        .post(routes::contacts::update_contact)
                        .delete(routes::contacts::delete_contact),
                )
                .route(
                    "/contacts/{id}/edit",
                    get(routes::contacts::edit_contact_form),
                )
                .route("/events", get(routes::events::event_stream))
                .layer(Extension(broadcaster))
                .layer(Extension(client_cache.clone()))
                .layer(session_layer.clone())
        })
        .with_frontend_routes(|router| {
            router
                .route("/", get(routes::pages::index))
                .route(
                    "/login",
                    get(routes::pages::login_page).post(routes::auth::login),
                )
                .route("/logout", post(routes::auth::logout))
                .route("/inbox", get(routes::pages::inbox))
                .route("/calendar", get(routes::pages::calendar))
                .route("/contacts", get(routes::pages::contacts))
                .route("/static/{*path}", get(assets::serve_embedded))
                .layer(Extension(client_cache))
                .layer(session_layer)
        })
        .build_routes()
}
