use acton_service::prelude::*;

use crate::error::MissiveError;
use crate::jmap::{self, MailboxInfo};
use crate::session::AuthenticatedClient;

#[derive(Template)]
#[template(path = "partials/mailbox_list.html")]
struct MailboxListTemplate {
    mailboxes: Vec<MailboxInfo>,
}

pub async fn list_mailboxes(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    debug!("list_mailboxes: request received");
    let mailboxes = jmap::fetch_mailboxes(&client).await?;
    debug!("list_mailboxes: returning {} mailboxes", mailboxes.len());
    Ok(HtmlTemplate::page(MailboxListTemplate { mailboxes }))
}
