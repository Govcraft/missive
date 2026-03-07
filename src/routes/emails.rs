use acton_service::prelude::*;

use crate::config::PostalConfig;
use crate::error::PostalError;
use crate::jmap::{self, EmailDetail, EmailSummary};
use crate::session::{get_credentials, PostalSession};

#[derive(Deserialize)]
pub struct EmailListParams {
    pub mailbox_id: String,
    #[serde(default)]
    pub position: usize,
}

#[derive(Template)]
#[template(path = "partials/email_list.html")]
struct EmailListTemplate {
    emails: Vec<EmailSummary>,
    mailbox_id: String,
    next_position: Option<usize>,
}

#[derive(Template)]
#[template(path = "partials/email_detail.html")]
struct EmailDetailTemplate {
    email: EmailDetail,
}

pub async fn list_emails(
    State(state): State<AppState<PostalConfig>>,
    session: TypedSession<PostalSession>,
    Query(params): Query<EmailListParams>,
) -> std::result::Result<impl IntoResponse, PostalError> {
    info!("list_emails: mailbox_id={}", params.mailbox_id);
    let (username, password) =
        get_credentials(&session).ok_or(PostalError::SessionRequired)?;
    let jmap_url = &state.config().custom.jmap_url;
    let client = jmap::create_client(jmap_url, &username, &password).await?;
    let page_size = 50;
    let emails = jmap::fetch_emails(&client, &params.mailbox_id, params.position, page_size).await?;
    info!("list_emails: returning {} emails at position {}", emails.len(), params.position);
    let next_position = if emails.len() == page_size {
        Some(params.position + page_size)
    } else {
        None
    };
    Ok(HtmlTemplate::page(EmailListTemplate {
        emails,
        mailbox_id: params.mailbox_id,
        next_position,
    }))
}

pub async fn get_email(
    State(state): State<AppState<PostalConfig>>,
    session: TypedSession<PostalSession>,
    Path(id): Path<String>,
) -> std::result::Result<impl IntoResponse, PostalError> {
    info!("get_email: id={id}");
    let (username, password) =
        get_credentials(&session).ok_or(PostalError::SessionRequired)?;
    let jmap_url = &state.config().custom.jmap_url;
    let client = jmap::create_client(jmap_url, &username, &password).await?;
    let email = jmap::fetch_email_detail(&client, &id).await?;
    info!("get_email: returning email subject={}", email.subject);
    Ok(HtmlTemplate::page(EmailDetailTemplate { email }))
}
