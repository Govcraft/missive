use acton_service::prelude::*;
use acton_service::session::{SessionConfig, create_memory_session_layer};
use tower_http::services::ServeDir;

mod config;
mod error;
mod jmap;
mod routes;
mod sanitize;
mod session;

use config::PostalConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let session_config = SessionConfig::default();
    let session_layer = create_memory_session_layer(&session_config);

    let mut config = Config::<PostalConfig>::load()?;
    info!(
        "Loaded config: service={}, jmap_url={}",
        config.service.name, config.custom.jmap_url
    );

    if config.custom.jmap_url.is_empty() {
        // Fallback: try loading jmap_url directly from config.toml
        // since #[serde(flatten)] may not work with figment
        if let Ok(raw) = std::fs::read_to_string("config.toml") {
            for line in raw.lines() {
                if let Some(val) = line.strip_prefix("jmap_url")
                    && let Some(val) = val.trim().strip_prefix('=')
                {
                    let val = val.trim().trim_matches('"');
                    config.custom.jmap_url = val.to_string();
                    info!("Loaded jmap_url from config.toml: {val}");
                }
            }
        }
    }

    let routes = VersionedApiBuilder::<PostalConfig>::with_config()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/mailboxes", get(routes::mailboxes::list_mailboxes))
                .route("/emails", get(routes::emails::list_emails))
                .route("/emails/{id}", get(routes::emails::get_email))
                .route(
                    "/attachments/{blob_id}",
                    get(routes::emails::download_attachment),
                )
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
                .layer(session_layer)
        })
        .build_routes();

    ServiceBuilder::<PostalConfig>::new()
        .with_config(config)
        .with_routes(routes)
        .build()
        .serve()
        .await
}
