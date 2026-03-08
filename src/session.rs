use std::sync::Arc;

use acton_service::prelude::*;
use axum::Extension;
use axum::extract::FromRequestParts;
use jmap_client::client::Client;
use secrecy::{ExposeSecret, SecretString};

use crate::config::MissiveConfig;
use crate::error::MissiveError;
use crate::jmap::{self, JmapClientCache};

#[derive(Default, Serialize, Deserialize)]
pub struct MissiveSession {
    pub username: Option<String>,
    #[serde(
        serialize_with = "serialize_secret",
        deserialize_with = "deserialize_secret",
        default
    )]
    pub password: Option<SecretString>,
}

fn serialize_secret<S: serde::Serializer>(
    secret: &Option<SecretString>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match secret {
        Some(s) => serializer.serialize_some(s.expose_secret()),
        None => serializer.serialize_none(),
    }
}

fn deserialize_secret<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<SecretString>, D::Error> {
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(SecretString::from))
}

fn get_credentials(session: &TypedSession<MissiveSession>) -> Option<(String, SecretString)> {
    let data = session.data();
    match (&data.username, &data.password) {
        (Some(u), Some(p)) => Some((u.clone(), p.clone())),
        _ => None,
    }
}

pub struct AuthenticatedClient(pub Arc<Client>, pub String);

impl FromRequestParts<AppState<MissiveConfig>> for AuthenticatedClient {
    type Rejection = MissiveError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState<MissiveConfig>,
    ) -> std::result::Result<Self, Self::Rejection> {
        let session = TypedSession::<MissiveSession>::from_request_parts(parts, state)
            .await
            .map_err(|_| MissiveError::SessionRequired)?;

        let Extension(cache) = Extension::<JmapClientCache>::from_request_parts(parts, state)
            .await
            .map_err(|_| MissiveError::Jmap("Client cache not available".to_string()))?;

        let (username, password) =
            get_credentials(&session).ok_or(MissiveError::SessionRequired)?;
        let client = jmap::get_or_create_client(
            &cache,
            &state.config().custom.jmap_url,
            &username,
            password.expose_secret(),
        )
        .await?;

        Ok(AuthenticatedClient(client, username))
    }
}
