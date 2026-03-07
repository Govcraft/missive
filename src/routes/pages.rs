use acton_service::prelude::*;
use axum::response::Redirect;

use crate::session::PostalSession;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPageTemplate {
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "inbox.html")]
struct InboxTemplate {
    username: String,
    active_app: String,
}

#[derive(Template)]
#[template(path = "calendar.html")]
struct CalendarTemplate {
    username: String,
    active_app: String,
}

#[derive(Template)]
#[template(path = "contacts.html")]
struct ContactsTemplate {
    username: String,
    active_app: String,
}

pub async fn index(session: TypedSession<PostalSession>) -> impl IntoResponse {
    if session.data().username.is_some() {
        Redirect::to("/inbox").into_response()
    } else {
        Redirect::to("/login").into_response()
    }
}

pub async fn login_page() -> impl IntoResponse {
    HtmlTemplate::page(LoginPageTemplate { error: None })
}

pub async fn inbox(session: TypedSession<PostalSession>) -> impl IntoResponse {
    match &session.data().username {
        Some(username) => HtmlTemplate::page(InboxTemplate {
            username: username.clone(),
            active_app: "mail".to_string(),
        })
        .into_response(),
        None => Redirect::to("/login").into_response(),
    }
}

pub async fn calendar(session: TypedSession<PostalSession>) -> impl IntoResponse {
    match &session.data().username {
        Some(username) => HtmlTemplate::page(CalendarTemplate {
            username: username.clone(),
            active_app: "calendar".to_string(),
        })
        .into_response(),
        None => Redirect::to("/login").into_response(),
    }
}

pub async fn contacts(session: TypedSession<PostalSession>) -> impl IntoResponse {
    match &session.data().username {
        Some(username) => HtmlTemplate::page(ContactsTemplate {
            username: username.clone(),
            active_app: "contacts".to_string(),
        })
        .into_response(),
        None => Redirect::to("/login").into_response(),
    }
}
