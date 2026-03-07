use acton_service::prelude::*;

use crate::config::PostalConfig;
use crate::error::PostalError;
use crate::jmap::{self, EmailDetail, EmailSummary};
use crate::session::{get_credentials, PostalSession};

#[derive(Deserialize)]
pub struct EmailListParams {
    pub mailbox_id: String,
}

#[derive(Template)]
#[template(path = "partials/email_list.html")]
struct EmailListTemplate {
    emails: Vec<EmailSummary>,
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
    let emails = jmap::fetch_emails(&client, &params.mailbox_id, 50).await?;
    info!("list_emails: returning {} emails", emails.len());
    Ok(HtmlTemplate::page(EmailListTemplate { emails }))
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
