use acton_service::prelude::*;
use axum::response::Redirect;

use crate::config::PostalConfig;
use crate::jmap::create_client;
use crate::session::PostalSession;

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
    State(state): State<AppState<PostalConfig>>,
    mut session: TypedSession<PostalSession>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let jmap_url = &state.config().custom.jmap_url;
    info!("Login attempt: user={}, jmap_url={jmap_url}", form.username);
    match create_client(jmap_url, &form.username, &form.password).await {
        Ok(_) => {
            let data = session.data_mut();
            data.username = Some(form.username);
            data.password = Some(form.password);
            let _ = session.save().await;
            Redirect::to("/inbox").into_response()
        }
        Err(_) => HtmlTemplate::page(LoginTemplate {
            error: Some("Invalid credentials".to_string()),
        })
        .into_response(),
    }
}

pub async fn logout(session: TypedSession<PostalSession>) -> impl IntoResponse {
    let _ = session.session().flush().await;
    Redirect::to("/login")
}
