use acton_service::prelude::*;
use axum::response::Redirect;

#[derive(Debug)]
pub enum MissiveError {
    Jmap(String),
    AuthFailed,
    SessionRequired,
    HttpResponse(axum::http::Error),
}

impl std::fmt::Display for MissiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jmap(msg) => write!(f, "JMAP error: {msg}"),
            Self::AuthFailed => write!(f, "Authentication failed: invalid credentials"),
            Self::SessionRequired => write!(f, "Session required: please log in"),
            Self::HttpResponse(e) => write!(f, "HTTP response error: {e}"),
        }
    }
}

impl std::error::Error for MissiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HttpResponse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<axum::http::Error> for MissiveError {
    fn from(error: axum::http::Error) -> Self {
        Self::HttpResponse(error)
    }
}

impl IntoResponse for MissiveError {
    fn into_response(self) -> Response {
        error!("MissiveError: {self}");
        let (status, message) = match &self {
            MissiveError::Jmap(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            MissiveError::AuthFailed => {
                (StatusCode::UNAUTHORIZED, "Invalid credentials".to_string())
            }
            MissiveError::SessionRequired => {
                return Redirect::to("/login").into_response();
            }
            MissiveError::HttpResponse(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        };
        (
            status,
            Html(format!("<div class=\"error\">{message}</div>")),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn display_jmap_error() {
        let error = MissiveError::Jmap("connection refused".to_string());
        assert_eq!(error.to_string(), "JMAP error: connection refused");
    }

    #[test]
    fn display_auth_failed() {
        let error = MissiveError::AuthFailed;
        assert_eq!(
            error.to_string(),
            "Authentication failed: invalid credentials"
        );
    }

    #[test]
    fn display_session_required() {
        let error = MissiveError::SessionRequired;
        assert_eq!(error.to_string(), "Session required: please log in");
    }

    #[test]
    fn source_returns_none_for_leaf_variants() {
        use std::error::Error;

        let jmap = MissiveError::Jmap("test".to_string());
        assert!(jmap.source().is_none());

        let auth = MissiveError::AuthFailed;
        assert!(auth.source().is_none());

        let session = MissiveError::SessionRequired;
        assert!(session.source().is_none());
    }

    #[test]
    fn from_http_error_conversion() {
        let http_err = axum::http::Response::builder()
            .status(9999)
            .body(())
            .unwrap_err();

        let missive_err = MissiveError::from(http_err);
        assert!(matches!(missive_err, MissiveError::HttpResponse(_)));
    }

    #[test]
    fn source_returns_some_for_http_response() {
        use std::error::Error;

        let http_err = axum::http::Response::builder()
            .status(9999)
            .body(())
            .unwrap_err();

        let missive_err = MissiveError::HttpResponse(http_err);
        assert!(missive_err.source().is_some());
    }

    #[test]
    fn display_http_response_error() {
        let http_err = axum::http::Response::builder()
            .status(9999)
            .body(())
            .unwrap_err();

        let missive_err = MissiveError::HttpResponse(http_err);
        let display = missive_err.to_string();
        assert!(
            display.starts_with("HTTP response error:"),
            "Expected display to start with 'HTTP response error:', got: {display}"
        );
    }

    #[test]
    fn into_response_jmap_returns_bad_gateway() {
        let error = MissiveError::Jmap("upstream failure".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn into_response_auth_failed_returns_unauthorized() {
        let error = MissiveError::AuthFailed;
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn into_response_session_required_redirects_to_login() {
        let error = MissiveError::SessionRequired;
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers().get("location").map(|v| v.to_str().ok());
        assert_eq!(location, Some(Some("/login")));
    }

    #[test]
    fn into_response_http_response_returns_internal_server_error() {
        let http_err = axum::http::Response::builder()
            .status(9999)
            .body(())
            .unwrap_err();

        let error = MissiveError::HttpResponse(http_err);
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
