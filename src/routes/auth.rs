use acton_service::prelude::*;
use axum::Extension;
use axum::response::Redirect;
use secrecy::SecretString;

use crate::config::MissiveConfig;
use crate::jmap::{JmapClientCache, create_client};
use crate::session::MissiveSession;

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: Option<String>,
}

pub async fn login(
    State(state): State<AppState<MissiveConfig>>,
    Extension(cache): Extension<JmapClientCache>,
    mut session: TypedSession<MissiveSession>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let jmap_url = &state.config().custom.jmap_url;
    info!("Login attempt: user={}, jmap_url={}", form.username, jmap_url);
    match create_client(jmap_url, &form.username, &form.password).await {
        Ok(client) => {
            cache.insert(form.username.clone(), std::sync::Arc::new(client));
            let data = session.data_mut();
            data.username = Some(form.username);
            data.password = Some(SecretString::from(form.password));
            let _ = session.save().await;
            Redirect::to("/inbox").into_response()
        }
        Err(_) => HtmlTemplate::page(LoginTemplate {
            error: Some("Invalid credentials".to_string()),
        })
        .into_response(),
    }
}

pub async fn logout(
    Extension(cache): Extension<JmapClientCache>,
    session: TypedSession<MissiveSession>,
) -> impl IntoResponse {
    if let Some(username) = &session.data().username {
        cache.remove(username);
    }
    let _ = session.session().flush().await;
    Redirect::to("/login")
}
