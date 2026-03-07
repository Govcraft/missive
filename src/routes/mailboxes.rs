use acton_service::prelude::*;

use crate::config::PostalConfig;
use crate::error::PostalError;
use crate::jmap::{self, MailboxInfo};
use crate::session::{PostalSession, get_credentials};

#[derive(Template)]
#[template(path = "partials/mailbox_list.html")]
struct MailboxListTemplate {
    mailboxes: Vec<MailboxInfo>,
}

pub async fn list_mailboxes(
    State(state): State<AppState<PostalConfig>>,
    session: TypedSession<PostalSession>,
) -> std::result::Result<impl IntoResponse, PostalError> {
    info!("list_mailboxes: request received");
    let (username, password) = get_credentials(&session).ok_or(PostalError::SessionRequired)?;
    info!("list_mailboxes: authenticated as {username}");
    let jmap_url = &state.config().custom.jmap_url;
    let client = jmap::create_client(jmap_url, &username, &password).await?;
    let mailboxes = jmap::fetch_mailboxes(&client).await?;
    info!("list_mailboxes: returning {} mailboxes", mailboxes.len());
    Ok(HtmlTemplate::page(MailboxListTemplate { mailboxes }))
}
