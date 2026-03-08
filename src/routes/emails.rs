use acton_service::prelude::*;
use axum::body::Body;

use crate::config::MissiveConfig;
use crate::error::MissiveError;
use crate::jmap::{self, BlobId, EmailDetail, EmailId, EmailSummary, IdentityId, IdentityInfo, MailboxId};
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

#[derive(Template)]
#[template(path = "partials/compose_form.html")]
struct ComposeFormTemplate {
    identities: Vec<IdentityInfo>,
    error: Option<String>,
    form: ComposeFormData,
}

#[derive(Template)]
#[template(path = "partials/compose_success.html")]
struct ComposeSuccessTemplate;

#[derive(Template)]
#[template(path = "partials/empty_state.html")]
struct EmptyStateTemplate;

#[derive(Deserialize, Default)]
pub struct ComposeFormData {
    #[serde(default)]
    pub identity_id: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
}

pub async fn list_emails(
    State(state): State<AppState<MissiveConfig>>,
    AuthenticatedClient(client, _): AuthenticatedClient,
    Query(params): Query<EmailListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("list_emails: mailbox_id={}", params.mailbox_id);
    let page_size = state.config().custom.page_size;
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
    AuthenticatedClient(client, _): AuthenticatedClient,
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
    AuthenticatedClient(client, _): AuthenticatedClient,
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

fn filter_identities_for_user(
    identities: Vec<IdentityInfo>,
    username: &str,
) -> Vec<IdentityInfo> {
    let filtered: Vec<IdentityInfo> = identities
        .iter()
        .filter(|i| i.email == username)
        .cloned()
        .collect();
    // Fall back to all identities if none match the username exactly
    if filtered.is_empty() { identities } else { filtered }
}

pub async fn compose_form(
    AuthenticatedClient(client, username): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("compose_form: loading compose view for user={username}");
    let all_identities = jmap::fetch_identities(&client).await?;
    let identities = filter_identities_for_user(all_identities, &username);
    let error = if identities.is_empty() {
        Some("No sending identities configured for your account".to_string())
    } else {
        None
    };
    Ok(HtmlTemplate::page(ComposeFormTemplate {
        identities,
        error,
        form: ComposeFormData::default(),
    }))
}

pub async fn send_email(
    AuthenticatedClient(client, username): AuthenticatedClient,
    Form(form): Form<ComposeFormData>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("send_email: sending message to={}", form.to);
    let all_identities = jmap::fetch_identities(&client).await?;
    let identities = filter_identities_for_user(all_identities, &username);
    let identity = identities
        .iter()
        .find(|i| i.id.as_str() == form.identity_id);

    let from_email = match identity {
        Some(i) => i.email.clone(),
        None => {
            return Ok(HtmlTemplate::page(ComposeFormTemplate {
                identities,
                error: Some("Invalid sending identity".to_string()),
                form,
            })
            .into_response());
        }
    };

    match jmap::send_email(
        &client,
        &IdentityId::from(form.identity_id.as_str()),
        &from_email,
        &form.to,
        &form.cc,
        &form.subject,
        &form.body,
    )
    .await
    {
        Ok(()) => Ok(HtmlTemplate::page(ComposeSuccessTemplate).into_response()),
        Err(e) => {
            let error_msg = e.to_string();
            Ok(HtmlTemplate::page(ComposeFormTemplate {
                identities,
                error: Some(error_msg),
                form,
            })
            .into_response())
        }
    }
}

pub async fn compose_cancel() -> std::result::Result<impl IntoResponse, MissiveError> {
    Ok(HtmlTemplate::page(EmptyStateTemplate))
}
