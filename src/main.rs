#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use acton_service::prelude::*;
use acton_service::session::{SessionConfig, create_memory_session_layer};
use axum::Extension;
use tower_http::services::ServeDir;

use crate::jmap::{JmapUrl, new_client_cache};

mod config;
mod error;
mod jmap;
mod routes;
mod sanitize;
mod session;

use config::MissiveConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let session_config = SessionConfig::default();
    let session_layer = create_memory_session_layer(&session_config);
    let client_cache = new_client_cache();

    let mut config = Config::<MissiveConfig>::load()?;
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
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()).into());
    }

    let routes = VersionedApiBuilder::<MissiveConfig>::with_config()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/mailboxes", get(routes::mailboxes::list_mailboxes))
                .route("/emails", get(routes::emails::list_emails))
                .route("/emails/bulk", post(routes::emails::bulk_action))
                .route("/emails/{id}", get(routes::emails::get_email).delete(routes::emails::delete_email))
                .route("/emails/{id}/mark-unread", post(routes::emails::mark_unread))
                .route("/emails/{id}/toggle-flag", post(routes::emails::toggle_flag))
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
                .nest_service("/static", ServeDir::new("static"))
                .layer(Extension(client_cache))
                .layer(session_layer)
        })
        .build_routes();

    ServiceBuilder::<MissiveConfig>::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await
}
