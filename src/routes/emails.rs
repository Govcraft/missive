use acton_service::prelude::*;
use axum::body::Body;

use crate::error::MissiveError;
use crate::jmap::{self, BlobId, EmailDetail, EmailId, EmailSummary, MailboxId};
use crate::session::AuthenticatedClient;

#[derive(Deserialize)]
pub struct EmailListParams {
    pub mailbox_id: MailboxId,
    #[serde(default)]
    pub position: usize,
}

#[derive(Template)]
#[template(path = "partials/email_list.html")]
struct EmailListTemplate {
    emails: Vec<EmailSummary>,
    mailbox_id: MailboxId,
    next_position: Option<usize>,
}

#[derive(Template)]
#[template(path = "partials/email_detail.html")]
struct EmailDetailTemplate {
    email: EmailDetail,
}

pub async fn list_emails(
    AuthenticatedClient(client): AuthenticatedClient,
    Query(params): Query<EmailListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("list_emails: mailbox_id={}", params.mailbox_id);
    let page_size = 50;
    let emails =
        jmap::fetch_emails(&client, &params.mailbox_id, params.position, page_size).await?;
    info!(
        "list_emails: returning {} emails at position {}",
        emails.len(),
        params.position
    );
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
    AuthenticatedClient(client): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("get_email: id={id}");
    let email = jmap::fetch_email_detail(&client, &id).await?;
    info!("get_email: returning email subject={}", email.subject);
    Ok(HtmlTemplate::page(EmailDetailTemplate { email }))
}

#[derive(Deserialize)]
pub struct DownloadParams {
    pub name: Option<String>,
}

pub async fn download_attachment(
    AuthenticatedClient(client): AuthenticatedClient,
    Path(blob_id): Path<BlobId>,
    Query(params): Query<DownloadParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("download_attachment: blob_id={blob_id}");
    let data = jmap::download_blob(&client, &blob_id).await?;
    let filename = params.name.unwrap_or_else(|| "attachment".to_string());
    Ok(Response::builder()
        .header("Content-Type", "application/octet-stream")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(data))?)
}
