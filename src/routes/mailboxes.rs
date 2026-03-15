use acton_service::prelude::*;

use crate::error::MissiveError;
use crate::jmap::{self, MailboxInfo};
use crate::session::AuthenticatedClient;

#[derive(Deserialize)]
pub struct MailboxListParams {
    #[serde(default)]
    pub mailbox_id: Option<String>,
}

#[derive(Template)]
#[template(path = "partials/mailbox_list.html")]
struct MailboxListTemplate {
    mailboxes: Vec<MailboxInfo>,
    active_mailbox_id: String,
}

pub async fn list_mailboxes(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Query(params): Query<MailboxListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("list_mailboxes: request received");
    let mailboxes = jmap::fetch_mailboxes(&client).await?;
    let active_mailbox_id = params.mailbox_id.unwrap_or_default();
    trace!(
        "list_mailboxes: returning {} mailboxes, active={}",
        mailboxes.len(),
        active_mailbox_id
    );
    Ok(HtmlTemplate::page(MailboxListTemplate {
        mailboxes,
        active_mailbox_id,
    }))
}
