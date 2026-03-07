use acton_service::prelude::*;
use axum::response::Redirect;

#[derive(Debug, thiserror::Error)]
pub enum PostalError {
    #[error("JMAP error: {0}")]
    Jmap(String),
    #[error("Authentication failed: invalid credentials")]
    AuthFailed,
    #[error("Session required: please log in")]
    SessionRequired,
}

impl IntoResponse for PostalError {
    fn into_response(self) -> Response {
        error!("PostalError: {self}");
        let (status, message) = match &self {
            PostalError::Jmap(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            PostalError::AuthFailed => (
                StatusCode::UNAUTHORIZED,
                "Invalid credentials".to_string(),
            ),
            PostalError::SessionRequired => {
                return Redirect::to("/login").into_response();
            }
        };
        (status, Html(format!("<div class=\"error\">{message}</div>"))).into_response()
    }
}
