use acton_service::prelude::*;
use axum::response::Redirect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JmapErrorKind {
    ConnectionFailed { url: String, message: String },
    QueryFailed { method: String, message: String },
    NotFound { resource: String, id: String },
    SubmissionFailed { message: String },
    BlobDownloadFailed { blob_id: String, message: String },
    NoMailbox { role: String },
    NoRecipient,
    Unknown { message: String },
}

impl std::fmt::Display for JmapErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed { url, message } => {
                write!(f, "connection to {url} failed: {message}")
            }
            Self::QueryFailed { method, message } => {
                write!(f, "{method} query failed: {message}")
            }
            Self::NotFound { resource, id } => {
                write!(f, "{resource} not found: {id}")
            }
            Self::SubmissionFailed { message } => {
                write!(f, "email submission failed: {message}")
            }
            Self::BlobDownloadFailed { blob_id, message } => {
                write!(f, "blob download failed for {blob_id}: {message}")
            }
            Self::NoMailbox { role } => {
                write!(f, "no {role} mailbox found")
            }
            Self::NoRecipient => {
                write!(f, "at least one recipient required")
            }
            Self::Unknown { message } => {
                write!(f, "{message}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissiveError {
    Jmap(JmapErrorKind),
    AuthFailed,
    SessionRequired,
    HttpResponse(String),
}

impl std::fmt::Display for MissiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jmap(kind) => write!(f, "JMAP error: {kind}"),
            Self::AuthFailed => write!(f, "Authentication failed: invalid credentials"),
            Self::SessionRequired => write!(f, "Session required: please log in"),
            Self::HttpResponse(msg) => write!(f, "HTTP response error: {msg}"),
        }
    }
}

impl std::error::Error for MissiveError {}

impl From<axum::http::Error> for MissiveError {
    fn from(error: axum::http::Error) -> Self {
        Self::HttpResponse(error.to_string())
    }
}

impl IntoResponse for MissiveError {
    fn into_response(self) -> Response {
        error!("MissiveError: {self}");
        let (status, message) = match &self {
            MissiveError::Jmap(kind) => (StatusCode::BAD_GATEWAY, kind.to_string()),
            MissiveError::AuthFailed => {
                (StatusCode::UNAUTHORIZED, "Invalid credentials".to_string())
            }
            MissiveError::SessionRequired => {
                return Redirect::to("/login").into_response();
            }
            MissiveError::HttpResponse(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
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

    // --- JmapErrorKind Display tests ---

    #[test]
    fn display_jmap_error_kind_connection_failed() {
        let kind = JmapErrorKind::ConnectionFailed {
            url: "https://mail.example.com".to_string(),
            message: "connection refused".to_string(),
        };
        assert_eq!(
            kind.to_string(),
            "connection to https://mail.example.com failed: connection refused"
        );
    }

    #[test]
    fn display_jmap_error_kind_query_failed() {
        let kind = JmapErrorKind::QueryFailed {
            method: "Email/get".to_string(),
            message: "timeout".to_string(),
        };
        assert_eq!(kind.to_string(), "Email/get query failed: timeout");
    }

    #[test]
    fn display_jmap_error_kind_not_found() {
        let kind = JmapErrorKind::NotFound {
            resource: "Email".to_string(),
            id: "abc123".to_string(),
        };
        assert_eq!(kind.to_string(), "Email not found: abc123");
    }

    #[test]
    fn display_jmap_error_kind_submission_failed() {
        let kind = JmapErrorKind::SubmissionFailed {
            message: "rejected".to_string(),
        };
        assert_eq!(kind.to_string(), "email submission failed: rejected");
    }

    #[test]
    fn display_jmap_error_kind_blob_download_failed() {
        let kind = JmapErrorKind::BlobDownloadFailed {
            blob_id: "blob-1".to_string(),
            message: "not found".to_string(),
        };
        assert_eq!(
            kind.to_string(),
            "blob download failed for blob-1: not found"
        );
    }

    #[test]
    fn display_jmap_error_kind_no_mailbox() {
        let kind = JmapErrorKind::NoMailbox {
            role: "drafts".to_string(),
        };
        assert_eq!(kind.to_string(), "no drafts mailbox found");
    }

    #[test]
    fn display_jmap_error_kind_no_recipient() {
        assert_eq!(
            JmapErrorKind::NoRecipient.to_string(),
            "at least one recipient required"
        );
    }

    #[test]
    fn display_jmap_error_kind_unknown() {
        let kind = JmapErrorKind::Unknown {
            message: "something went wrong".to_string(),
        };
        assert_eq!(kind.to_string(), "something went wrong");
    }

    // --- MissiveError Display tests ---

    #[test]
    fn display_jmap_error() {
        let error = MissiveError::Jmap(JmapErrorKind::ConnectionFailed {
            url: "https://example.com".to_string(),
            message: "connection refused".to_string(),
        });
        assert_eq!(
            error.to_string(),
            "JMAP error: connection to https://example.com failed: connection refused"
        );
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
    fn display_http_response_error() {
        let error = MissiveError::HttpResponse("invalid status code".to_string());
        assert_eq!(
            error.to_string(),
            "HTTP response error: invalid status code"
        );
    }

    #[test]
    fn source_returns_none_for_all_variants() {
        use std::error::Error;

        let jmap = MissiveError::Jmap(JmapErrorKind::NoRecipient);
        assert!(jmap.source().is_none());

        let auth = MissiveError::AuthFailed;
        assert!(auth.source().is_none());

        let session = MissiveError::SessionRequired;
        assert!(session.source().is_none());

        let http = MissiveError::HttpResponse("err".to_string());
        assert!(http.source().is_none());
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
    fn into_response_jmap_returns_bad_gateway() {
        let error = MissiveError::Jmap(JmapErrorKind::Unknown {
            message: "upstream failure".to_string(),
        });
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
        let error = MissiveError::HttpResponse("some error".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn jmap_error_kind_clone_and_eq() {
        let kind = JmapErrorKind::NoRecipient;
        let cloned = kind.clone();
        assert_eq!(kind, cloned);
    }

    #[test]
    fn missive_error_clone_and_eq() {
        let err = MissiveError::AuthFailed;
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }
}
