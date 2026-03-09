use axum::body::Body;
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticAssets;

pub async fn serve_embedded(Path(path): Path<String>) -> Response {
    let Some(file) = StaticAssets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = file.metadata.mimetype();

    (
        [(header::CONTENT_TYPE, mime)],
        Body::from(file.data),
    )
        .into_response()
}
