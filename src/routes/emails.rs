use std::sync::Arc;

use acton_service::prelude::*;
use acton_service::session::{FlashMessage, FlashMessages, Session};
use axum::body::Body;
use axum::extract::Multipart;

use crate::config::MissiveConfig;
use crate::error::{JmapErrorKind, MissiveError};
use crate::jmap::{
    self, BlobId, EmailDetail, EmailId, EmailSummary, IdentityId, IdentityInfo, MailboxId,
    MailboxInfo, SearchQuery,
};
use crate::sanitize::sanitize_compose_html;
use crate::session::AuthenticatedClient;

#[derive(Deserialize)]
pub struct EmailListParams {
    pub mailbox_id: MailboxId,
    #[serde(default)]
    pub position: usize,
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Template)]
#[template(path = "partials/email_list.html")]
struct EmailListTemplate {
    emails: Vec<EmailSummary>,
    mailbox_id: MailboxId,
    next_position: Option<usize>,
    search: Option<SearchQuery>,
    total_count: Option<usize>,
}

#[derive(Template)]
#[template(path = "partials/email_detail.html")]
struct EmailDetailTemplate {
    email: EmailDetail,
    mailboxes: Vec<MailboxInfo>,
}

#[derive(Template)]
#[template(path = "partials/compose_form.html")]
struct ComposeFormTemplate {
    identities: Vec<IdentityInfo>,
    form: ComposeFormData,
    title: String,
}

#[derive(Template)]
#[template(path = "partials/flash_toast.html")]
struct FlashToastTemplate {
    messages: Vec<FlashMessage>,
}

#[derive(Template)]
#[template(path = "partials/attachment_row.html")]
struct AttachmentRowTemplate {
    blob_id: BlobId,
    name: String,
    content_type: String,
    size_display: String,
}

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
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub body_html: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub references_header: String,
    /// JSON-encoded array of attachment objects: [{"blob_id":"...","name":"...","content_type":"..."}]
    #[serde(default)]
    pub attachments_json: String,
}

pub async fn list_emails(
    State(state): State<AppState<MissiveConfig>>,
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Query(params): Query<EmailListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let search = params.search.as_deref().and_then(SearchQuery::new);
    trace!(
        "list_emails: mailbox_id={}, search={search:?}",
        params.mailbox_id
    );
    let page_size = state.config().custom.page_size;
    let (emails, total_count) = jmap::fetch_emails(
        &client,
        &params.mailbox_id,
        params.position,
        page_size,
        search.as_ref(),
    )
    .await?;
    trace!(
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
        search,
        total_count,
    }))
}

pub async fn get_email(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("get_email: id={id}");
    let email = jmap::fetch_email_detail(&client, &id).await?;
    let mailboxes = jmap::fetch_mailboxes(&client).await?;
    trace!("get_email: returning email subject={}", email.subject);

    // Mark as read on the server (fire-and-forget; don't fail the view)
    if let Err(e) = jmap::mark_email_read(&client, &id).await {
        error!("Failed to mark email as read: {e}");
    }

    Ok(HtmlTemplate::page(EmailDetailTemplate { email, mailboxes })
        .with_hx_trigger("mailboxesUpdated, emailRead"))
}

#[derive(Deserialize)]
pub struct DownloadParams {
    pub name: Option<String>,
}

pub async fn download_attachment(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Path(blob_id): Path<BlobId>,
    Query(params): Query<DownloadParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("download_attachment: blob_id={blob_id}");
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

async fn push_flash(session: &Session, message: FlashMessage) {
    let description = format!("{:?}: {}", message.kind, message.message);
    match FlashMessages::push(session, message).await {
        Ok(()) => trace!("flash message pushed: {description}"),
        Err(e) => error!("failed to push flash message ({description}): {e}"),
    }
}

fn empty_state_with_flash() -> Response {
    HtmlTemplate::page(EmptyStateTemplate)
        .with_hx_trigger("flashUpdated")
        .into_response()
}

fn filter_identities_for_user(identities: Vec<IdentityInfo>, username: &str) -> Vec<IdentityInfo> {
    let filtered: Vec<IdentityInfo> = identities
        .iter()
        .filter(|i| i.email == username)
        .cloned()
        .collect();
    // Fall back to all identities if none match the username exactly
    if filtered.is_empty() {
        identities
    } else {
        filtered
    }
}

pub async fn compose_form(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("compose_form: loading compose view for user={username}");
    let all_identities = jmap::fetch_identities(&client).await?;
    let identities = filter_identities_for_user(all_identities, &username);
    if identities.is_empty() {
        push_flash(
            &session,
            FlashMessage::error("No sending identities configured for your account"),
        )
        .await;
        return Ok(empty_state_with_flash());
    }
    Ok(HtmlTemplate::page(ComposeFormTemplate {
        identities,
        form: ComposeFormData::default(),
        title: "New Message".to_string(),
    })
    .into_response())
}

pub async fn send_email(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Form(form): Form<ComposeFormData>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("send_email: sending message to={}", form.to);
    let all_identities = jmap::fetch_identities(&client).await?;
    let identities = filter_identities_for_user(all_identities, &username);
    let identity = identities
        .iter()
        .find(|i| i.id.as_str() == form.identity_id);

    let from_email = match identity {
        Some(i) => i.email.clone(),
        None => {
            push_flash(&session, FlashMessage::error("Invalid sending identity")).await;
            return Ok(HtmlTemplate::page(ComposeFormTemplate {
                identities,
                form,
                title: "New Message".to_string(),
            })
            .with_hx_trigger("flashUpdated")
            .into_response());
        }
    };

    let threading = jmap::ThreadingHeaders {
        in_reply_to: Some(form.in_reply_to.as_str()).filter(|s| !s.is_empty()),
        references: Some(form.references_header.as_str()).filter(|s| !s.is_empty()),
    };
    debug!(
        "send_email: attachments_json raw value={:?}",
        form.attachments_json
    );
    let attachments = build_attachments_from_form(&form);
    debug!(
        "send_email: {} attachment(s) parsed from form",
        attachments.len()
    );
    for att in &attachments {
        debug!(
            "send_email: attachment blob_id={}, name={}, type={}",
            att.blob_id, att.name, att.content_type
        );
    }
    let body_html = sanitized_body_html(&form.body_html);
    let content = jmap::EmailContent {
        from_email: &from_email,
        to: &form.to,
        cc: &form.cc,
        bcc: &form.bcc,
        subject: &form.subject,
        body_text: &form.body,
        body_html: body_html.as_deref(),
        threading: &threading,
        attachments,
    };
    match jmap::send_email(
        &client,
        &IdentityId::from(form.identity_id.as_str()),
        &content,
    )
    .await
    {
        Ok(()) => {
            push_flash(&session, FlashMessage::success("Message sent")).await;
            Ok(empty_state_with_flash())
        }
        Err(e) => {
            error!("send_email failed: {e}");
            push_flash(&session, FlashMessage::error(e.to_string())).await;
            Ok(HtmlTemplate::page(ComposeFormTemplate {
                identities,
                form,
                title: "New Message".to_string(),
            })
            .with_hx_trigger("flashUpdated")
            .into_response())
        }
    }
}

pub async fn save_draft(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Form(form): Form<ComposeFormData>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("save_draft: saving draft");
    let all_identities = jmap::fetch_identities(&client).await?;
    let identities = filter_identities_for_user(all_identities, &username);
    let from_email = identities
        .iter()
        .find(|i| i.id.as_str() == form.identity_id)
        .map(|i| i.email.as_str())
        .unwrap_or(&username);

    let threading = jmap::ThreadingHeaders {
        in_reply_to: Some(form.in_reply_to.as_str()).filter(|s| !s.is_empty()),
        references: Some(form.references_header.as_str()).filter(|s| !s.is_empty()),
    };
    let attachments = build_attachments_from_form(&form);
    debug!("save_draft: {} attachment(s) from form", attachments.len());
    for att in &attachments {
        debug!(
            "save_draft: attachment blob_id={}, name={}, type={}",
            att.blob_id, att.name, att.content_type
        );
    }
    let body_html = sanitized_body_html(&form.body_html);
    let content = jmap::EmailContent {
        from_email,
        to: &form.to,
        cc: &form.cc,
        bcc: &form.bcc,
        subject: &form.subject,
        body_text: &form.body,
        body_html: body_html.as_deref(),
        threading: &threading,
        attachments,
    };
    match jmap::save_draft(&client, &content).await {
        Ok(()) => {
            push_flash(&session, FlashMessage::success("Draft saved")).await;
            Ok(empty_state_with_flash())
        }
        Err(e) => {
            error!("save_draft failed: {e}");
            push_flash(&session, FlashMessage::error(e.to_string())).await;
            Ok(HtmlTemplate::page(ComposeFormTemplate {
                identities,
                form,
                title: "New Message".to_string(),
            })
            .with_hx_trigger("flashUpdated")
            .into_response())
        }
    }
}

#[derive(Deserialize)]
pub struct DeleteParams {
    pub mailbox_id: MailboxId,
}

pub async fn delete_email(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
    Query(params): Query<DeleteParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("delete_email: id={id}");
    let trash_id = jmap::find_mailbox_by_role(&client, "trash").await?;

    let flash_msg = if params.mailbox_id == trash_id {
        jmap::delete_email(&client, &id).await?;
        "Email permanently deleted"
    } else {
        jmap::move_email(&client, &id, &params.mailbox_id, &trash_id).await?;
        "Email moved to trash"
    };

    push_flash(&session, FlashMessage::success(flash_msg)).await;

    // Primary response replaces the detail pane (hx-target="#email-detail-pane").
    // OOB swap removes the deleted email's row from the list.
    let html = format!(
        "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">\
            Select an email to read\
         </div>\
         <li id=\"email-row-{id}\" hx-swap-oob=\"delete\"></li>"
    );

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(html))?)
}

#[derive(Deserialize)]
pub struct ArchiveParams {
    pub mailbox_id: MailboxId,
}

pub async fn archive_email(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
    Form(params): Form<ArchiveParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("archive_email: id={id}");
    let archive_id = jmap::find_mailbox_by_role(&client, "archive").await?;
    jmap::move_email(&client, &id, &params.mailbox_id, &archive_id).await?;
    push_flash(&session, FlashMessage::success("Email archived")).await;

    let html = format!(
        "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">\
            Select an email to read\
         </div>\
         <li id=\"email-row-{id}\" hx-swap-oob=\"delete\"></li>"
    );

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(html))?)
}

#[derive(Deserialize)]
pub struct SpamParams {
    pub mailbox_id: MailboxId,
}

pub async fn spam_email(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
    Form(params): Form<SpamParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("spam_email: id={id}");
    let junk_id = jmap::find_mailbox_by_role(&client, "junk").await?;
    jmap::move_email(&client, &id, &params.mailbox_id, &junk_id).await?;
    push_flash(&session, FlashMessage::success("Moved to spam")).await;

    let html = format!(
        "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">\
            Select an email to read\
         </div>\
         <li id=\"email-row-{id}\" hx-swap-oob=\"delete\"></li>"
    );

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(html))?)
}

pub async fn unspam_email(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
    Form(params): Form<SpamParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("unspam_email: id={id}");
    let inbox_id = jmap::find_mailbox_by_role(&client, "inbox").await?;
    jmap::move_email(&client, &id, &params.mailbox_id, &inbox_id).await?;
    push_flash(&session, FlashMessage::success("Moved to inbox")).await;

    let html = format!(
        "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">\
            Select an email to read\
         </div>\
         <li id=\"email-row-{id}\" hx-swap-oob=\"delete\"></li>"
    );

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(html))?)
}

#[derive(Deserialize)]
pub struct MoveEmailForm {
    pub target_mailbox_id: MailboxId,
    pub mailbox_id: MailboxId,
}

pub async fn move_email(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
    Form(params): Form<MoveEmailForm>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("move_email: id={id} to={}", params.target_mailbox_id);
    jmap::move_email(&client, &id, &params.mailbox_id, &params.target_mailbox_id).await?;
    push_flash(&session, FlashMessage::success("Email moved")).await;

    let html = format!(
        "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">\
            Select an email to read\
         </div>\
         <li id=\"email-row-{id}\" hx-swap-oob=\"delete\"></li>"
    );

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(html))?)
}

#[derive(Deserialize)]
pub struct ToggleFlagParams {
    pub flagged: bool,
}

#[derive(Template)]
#[template(path = "partials/star_button.html")]
struct StarButtonTemplate {
    email_id: EmailId,
    is_flagged: bool,
}

pub async fn toggle_flag(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
    Form(params): Form<ToggleFlagParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("toggle_flag: id={id}, flagged={}", params.flagged);
    jmap::toggle_email_flagged(&client, &id, params.flagged).await?;
    let label = if params.flagged {
        "Starred"
    } else {
        "Unstarred"
    };
    push_flash(&session, FlashMessage::success(label)).await;
    Ok(HtmlTemplate::page(StarButtonTemplate {
        email_id: id,
        is_flagged: params.flagged,
    })
    .with_hx_trigger("flashUpdated"))
}

pub async fn mark_unread(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("mark_unread: id={id}");
    jmap::mark_email_unread(&client, &id).await?;
    push_flash(&session, FlashMessage::success("Marked as unread")).await;
    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(
            "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">Select an email to read</div>",
        ))?)
}

fn prepend_subject_prefix(subject: &str, prefix: &str) -> String {
    let trimmed = subject.trim();
    let check = format!("{prefix}:");
    if trimmed.len() >= check.len() && trimmed[..check.len()].eq_ignore_ascii_case(&check) {
        trimmed.to_string()
    } else {
        format!("{prefix}: {trimmed}")
    }
}

fn quote_body(body: &str) -> String {
    body.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_reply_references(original_refs: &[String], original_msg_id: &[String]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut ids = Vec::new();
    for id in original_refs.iter().chain(original_msg_id.iter()) {
        if seen.insert(id) {
            ids.push(id.as_str());
        }
    }
    ids.join(" ")
}

fn remove_address(addresses: &str, to_remove: &str) -> String {
    addresses
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(to_remove))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_forwarded_body(email: &EmailDetail) -> String {
    format!(
        "\n\n---------- Forwarded message ----------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{}",
        email.from, email.received_at, email.subject, email.to, email.body_text
    )
}

fn sanitized_body_html(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(sanitize_compose_html(trimmed))
    }
}

fn quote_body_html(html: &str, from: &str, date: &str) -> String {
    format!("<br><br><p>On {date}, {from} wrote:</p><blockquote>{html}</blockquote>")
}

fn format_forwarded_body_html(email: &EmailDetail) -> String {
    let body = email.body_html.as_deref().unwrap_or(&email.body_text);
    format!(
        "<br><br><hr><p><b>---------- Forwarded message ----------</b><br>\
         From: {}<br>Date: {}<br>Subject: {}<br>To: {}</p><br>{}",
        email.from, email.received_at, email.subject, email.to, body
    )
}

fn build_attachments_from_form(form: &ComposeFormData) -> Vec<jmap::UploadedAttachment> {
    if form.attachments_json.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<AttachmentJson>>(&form.attachments_json) {
        Ok(items) => items
            .into_iter()
            .map(|a| jmap::UploadedAttachment {
                blob_id: BlobId::from(a.blob_id.as_str()),
                name: a.name,
                content_type: a.content_type,
            })
            .collect(),
        Err(e) => {
            error!("Failed to parse attachments_json: {e}");
            Vec::new()
        }
    }
}

#[derive(Deserialize)]
struct AttachmentJson {
    blob_id: String,
    name: String,
    content_type: String,
}

pub async fn upload_attachment(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    mut multipart: Multipart,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    if let Some(field) = multipart.next_field().await.map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "multipart".to_string(),
            message: e.to_string(),
        })
    })? {
        let name = field.file_name().unwrap_or("attachment").to_string();
        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let data = field.bytes().await.map_err(|e| {
            MissiveError::Jmap(JmapErrorKind::QueryFailed {
                method: "multipart".to_string(),
                message: e.to_string(),
            })
        })?;

        let size = data.len();
        let blob_id = jmap::upload_blob(&client, data.to_vec(), Some(&content_type)).await?;

        Ok(HtmlTemplate::page(AttachmentRowTemplate {
            blob_id,
            name,
            content_type,
            size_display: jmap::format_file_size(size),
        })
        .into_response())
    } else {
        Ok(Response::builder()
            .status(400)
            .header("Content-Type", "text/plain")
            .body(Body::from("No file uploaded"))
            .map_err(|e| MissiveError::HttpResponse(e.to_string()))?
            .into_response())
    }
}

pub async fn compose_cancel() -> std::result::Result<impl IntoResponse, MissiveError> {
    Ok(HtmlTemplate::page(EmptyStateTemplate))
}

enum ComposeMode {
    Reply,
    ReplyAll,
    Forward,
}

async fn compose_for_email(
    client: &Arc<jmap_client::client::Client>,
    username: &str,
    session: &Session,
    email_id: &EmailId,
    mode: ComposeMode,
) -> std::result::Result<Response, MissiveError> {
    let email = jmap::fetch_email_detail(client, email_id).await?;
    let all_identities = jmap::fetch_identities(client).await?;
    let identities = filter_identities_for_user(all_identities, username);
    if identities.is_empty() {
        push_flash(
            session,
            FlashMessage::error("No sending identities configured for your account"),
        )
        .await;
        return Ok(empty_state_with_flash());
    }

    let (title, form) = match mode {
        ComposeMode::Reply => {
            let in_reply_to = email.message_id.first().cloned().unwrap_or_default();
            let references_header = build_reply_references(&email.references, &email.message_id);
            let quoted = format!(
                "\n\nOn {}, {} wrote:\n{}",
                email.received_at,
                email.from,
                quote_body(&email.body_text)
            );
            let body_html = email
                .body_html
                .as_deref()
                .map(|html| quote_body_html(html, &email.from, &email.received_at))
                .unwrap_or_default();
            (
                "Reply".to_string(),
                ComposeFormData {
                    to: email.from_email.clone(),
                    subject: prepend_subject_prefix(&email.subject, "Re"),
                    body: quoted,
                    body_html,
                    in_reply_to,
                    references_header,
                    ..Default::default()
                },
            )
        }
        ComposeMode::ReplyAll => {
            let in_reply_to = email.message_id.first().cloned().unwrap_or_default();
            let references_header = build_reply_references(&email.references, &email.message_id);
            let quoted = format!(
                "\n\nOn {}, {} wrote:\n{}",
                email.received_at,
                email.from,
                quote_body(&email.body_text)
            );
            let body_html = email
                .body_html
                .as_deref()
                .map(|html| quote_body_html(html, &email.from, &email.received_at))
                .unwrap_or_default();
            let all_recipients = if email.cc_emails.is_empty() {
                email.to_emails.clone()
            } else {
                format!("{}, {}", email.to_emails, email.cc_emails)
            };
            let cc = remove_address(&all_recipients, username);
            (
                "Reply All".to_string(),
                ComposeFormData {
                    to: email.from_email.clone(),
                    cc,
                    subject: prepend_subject_prefix(&email.subject, "Re"),
                    body: quoted,
                    body_html,
                    in_reply_to,
                    references_header,
                    ..Default::default()
                },
            )
        }
        ComposeMode::Forward => (
            "Forward".to_string(),
            ComposeFormData {
                subject: prepend_subject_prefix(&email.subject, "Fwd"),
                body: format_forwarded_body(&email),
                body_html: format_forwarded_body_html(&email),
                ..Default::default()
            },
        ),
    };

    Ok(HtmlTemplate::page(ComposeFormTemplate {
        identities,
        form,
        title,
    })
    .into_response())
}

pub async fn reply(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("reply: id={id}");
    compose_for_email(&client, &username, &session, &id, ComposeMode::Reply).await
}

pub async fn reply_all(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("reply_all: id={id}");
    compose_for_email(&client, &username, &session, &id, ComposeMode::ReplyAll).await
}

pub async fn forward(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!("forward: id={id}");
    compose_for_email(&client, &username, &session, &id, ComposeMode::Forward).await
}

#[derive(Deserialize)]
pub struct BulkActionForm {
    pub email_ids: String,
    pub action: String,
    pub mailbox_id: MailboxId,
}

pub async fn bulk_action(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Form(form): Form<BulkActionForm>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    trace!(
        "bulk_action: action={}, mailbox={}",
        form.action, form.mailbox_id
    );
    let ids: Vec<EmailId> = form
        .email_ids
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(EmailId::from)
        .collect();

    if ids.is_empty() {
        push_flash(&session, FlashMessage::error("No emails selected")).await;
        return Ok(empty_state_with_flash());
    }

    let count = ids.len();
    let flash_msg = match form.action.as_str() {
        "delete" => {
            let trash_id = jmap::find_mailbox_by_role(&client, "trash").await?;
            if form.mailbox_id == trash_id {
                jmap::bulk_delete_emails(&client, &ids).await?;
                format!("{count} emails permanently deleted")
            } else {
                jmap::bulk_move_emails(&client, &ids, &form.mailbox_id, &trash_id).await?;
                format!("{count} emails moved to trash")
            }
        }
        "archive" => {
            let archive_id = jmap::find_mailbox_by_role(&client, "archive").await?;
            jmap::bulk_move_emails(&client, &ids, &form.mailbox_id, &archive_id).await?;
            format!("{count} emails archived")
        }
        "spam" => {
            let junk_id = jmap::find_mailbox_by_role(&client, "junk").await?;
            jmap::bulk_move_emails(&client, &ids, &form.mailbox_id, &junk_id).await?;
            format!("{count} emails marked as spam")
        }
        "read" => {
            jmap::bulk_set_keyword(&client, &ids, "$seen", true).await?;
            format!("{count} emails marked as read")
        }
        "unread" => {
            jmap::bulk_set_keyword(&client, &ids, "$seen", false).await?;
            format!("{count} emails marked as unread")
        }
        "flag" => {
            jmap::bulk_set_keyword(&client, &ids, "$flagged", true).await?;
            format!("{count} emails starred")
        }
        "unflag" => {
            jmap::bulk_set_keyword(&client, &ids, "$flagged", false).await?;
            format!("{count} emails unstarred")
        }
        _ => {
            push_flash(&session, FlashMessage::error("Unknown action")).await;
            return Ok(empty_state_with_flash());
        }
    };

    push_flash(&session, FlashMessage::success(flash_msg)).await;

    // Build OOB deletes for move/delete actions (not for keyword-only actions)
    let oob_deletes: String = match form.action.as_str() {
        "delete" | "archive" | "spam" => ids
            .iter()
            .map(|id| format!("<li id=\"email-row-{id}\" hx-swap-oob=\"delete\"></li>"))
            .collect(),
        _ => String::new(),
    };

    let html = format!(
        "<div class=\"flex items-center justify-center h-full text-sm text-gray-400\">\
            Select an email to read\
         </div>{oob_deletes}"
    );

    Ok(Response::builder()
        .header("Content-Type", "text/html")
        .header("HX-Trigger", "flashUpdated, mailboxesUpdated")
        .body(Body::from(html))?
        .into_response())
}

pub async fn get_flash(
    flash: FlashMessages,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let messages = flash.into_messages();
    trace!("get_flash: returning {} flash messages", messages.len());
    for msg in &messages {
        trace!("get_flash: {:?} - {}", msg.kind, msg.message);
    }
    Ok(HtmlTemplate::page(FlashToastTemplate { messages }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jmap::{EmailDetail, EmailId};

    fn make_email_detail() -> EmailDetail {
        EmailDetail {
            id: EmailId::from("test-id"),
            from: "Alice <alice@example.com>".to_string(),
            to: "Bob <bob@example.com>".to_string(),
            cc: String::new(),
            subject: "Hello World".to_string(),
            received_at: "Jan 1, 2025".to_string(),
            body_text: "Line one\nLine two".to_string(),
            body_html: None,
            attachments: Vec::new(),
            message_id: vec!["<msg-1@example.com>".to_string()],
            references: Vec::new(),
            from_email: "alice@example.com".to_string(),
            to_emails: "bob@example.com".to_string(),
            cc_emails: String::new(),
            is_flagged: false,
        }
    }

    // --- prepend_subject_prefix ---

    #[test]
    fn prepend_re_when_missing() {
        assert_eq!(prepend_subject_prefix("Hello", "Re"), "Re: Hello");
    }

    #[test]
    fn no_duplicate_re_prefix() {
        assert_eq!(prepend_subject_prefix("Re: Hello", "Re"), "Re: Hello");
    }

    #[test]
    fn no_duplicate_re_case_insensitive() {
        assert_eq!(prepend_subject_prefix("re: Hello", "Re"), "re: Hello");
    }

    #[test]
    fn prepend_fwd_when_missing() {
        assert_eq!(prepend_subject_prefix("Hello", "Fwd"), "Fwd: Hello");
    }

    #[test]
    fn no_duplicate_fwd_prefix() {
        assert_eq!(prepend_subject_prefix("Fwd: Hello", "Fwd"), "Fwd: Hello");
    }

    #[test]
    fn prepend_trims_whitespace() {
        assert_eq!(prepend_subject_prefix("  Hello  ", "Re"), "Re: Hello");
    }

    // --- quote_body ---

    #[test]
    fn quote_body_prefixes_lines() {
        assert_eq!(quote_body("a\nb"), "> a\n> b");
    }

    #[test]
    fn quote_body_empty() {
        assert_eq!(quote_body(""), "");
    }

    // --- build_reply_references ---

    #[test]
    fn build_refs_concatenates() {
        let refs = vec!["<a>".to_string()];
        let msg_id = vec!["<b>".to_string()];
        assert_eq!(build_reply_references(&refs, &msg_id), "<a> <b>");
    }

    #[test]
    fn build_refs_deduplicates() {
        let refs = vec!["<a>".to_string(), "<b>".to_string()];
        let msg_id = vec!["<b>".to_string()];
        assert_eq!(build_reply_references(&refs, &msg_id), "<a> <b>");
    }

    #[test]
    fn build_refs_empty() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(build_reply_references(&empty, &empty), "");
    }

    // --- remove_address ---

    #[test]
    fn remove_address_filters_match() {
        assert_eq!(
            remove_address("alice@example.com, bob@example.com", "alice@example.com"),
            "bob@example.com"
        );
    }

    #[test]
    fn remove_address_case_insensitive() {
        assert_eq!(
            remove_address("Alice@Example.com, bob@example.com", "alice@example.com"),
            "bob@example.com"
        );
    }

    #[test]
    fn remove_address_no_match() {
        assert_eq!(
            remove_address("bob@example.com", "alice@example.com"),
            "bob@example.com"
        );
    }

    #[test]
    fn remove_address_empty_result() {
        assert_eq!(remove_address("alice@example.com", "alice@example.com"), "");
    }

    // --- format_forwarded_body ---

    #[test]
    fn format_forwarded_body_structure() {
        let email = make_email_detail();
        let result = format_forwarded_body(&email);
        assert!(result.contains("---------- Forwarded message ----------"));
        assert!(result.contains("From: Alice <alice@example.com>"));
        assert!(result.contains("Date: Jan 1, 2025"));
        assert!(result.contains("Subject: Hello World"));
        assert!(result.contains("To: Bob <bob@example.com>"));
        assert!(result.contains("Line one\nLine two"));
    }

    // --- quote_body_html ---

    #[test]
    fn quote_body_html_structure() {
        let result = quote_body_html("<p>Hello</p>", "Alice", "Jan 1, 2025");
        assert!(result.contains("<blockquote><p>Hello</p></blockquote>"));
        assert!(result.contains("On Jan 1, 2025, Alice wrote:"));
    }

    // --- format_forwarded_body_html ---

    #[test]
    fn format_forwarded_body_html_structure() {
        let mut email = make_email_detail();
        email.body_html = Some("<p>HTML body</p>".to_string());
        let result = format_forwarded_body_html(&email);
        assert!(result.contains("---------- Forwarded message ----------"));
        assert!(result.contains("From: Alice <alice@example.com>"));
        assert!(result.contains("<p>HTML body</p>"));
    }

    #[test]
    fn format_forwarded_body_html_falls_back_to_text() {
        let email = make_email_detail();
        let result = format_forwarded_body_html(&email);
        assert!(result.contains("Line one\nLine two"));
    }

    // --- sanitized_body_html ---

    #[test]
    fn sanitized_body_html_returns_none_for_empty() {
        assert!(sanitized_body_html("").is_none());
        assert!(sanitized_body_html("   ").is_none());
    }

    #[test]
    fn sanitized_body_html_returns_some_for_content() {
        let result = sanitized_body_html("<p>Hello</p>");
        assert!(result.is_some());
        assert!(result.unwrap().contains("<p>Hello</p>"));
    }

    // --- build_attachments_from_form ---

    #[test]
    fn build_attachments_from_form_parses_json() {
        let form = ComposeFormData {
            attachments_json: r#"[{"blob_id":"blob-1","name":"file1.pdf","content_type":"application/pdf"},{"blob_id":"blob-2","name":"file2.png","content_type":"image/png"}]"#.to_string(),
            ..Default::default()
        };
        let attachments = build_attachments_from_form(&form);
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].blob_id.as_str(), "blob-1");
        assert_eq!(attachments[0].name, "file1.pdf");
        assert_eq!(attachments[0].content_type, "application/pdf");
        assert_eq!(attachments[1].blob_id.as_str(), "blob-2");
        assert_eq!(attachments[1].name, "file2.png");
        assert_eq!(attachments[1].content_type, "image/png");
    }

    #[test]
    fn build_attachments_from_form_empty() {
        let form = ComposeFormData::default();
        let attachments = build_attachments_from_form(&form);
        assert!(attachments.is_empty());
    }

    #[test]
    fn build_attachments_from_form_empty_json_array() {
        let form = ComposeFormData {
            attachments_json: "[]".to_string(),
            ..Default::default()
        };
        let attachments = build_attachments_from_form(&form);
        assert!(attachments.is_empty());
    }

    #[test]
    fn build_attachments_from_form_invalid_json_returns_empty() {
        let form = ComposeFormData {
            attachments_json: "not valid json".to_string(),
            ..Default::default()
        };
        let attachments = build_attachments_from_form(&form);
        assert!(attachments.is_empty());
    }

    // --- ComposeFormData deserialization with attachments_json ---

    #[test]
    fn compose_form_data_deserializes_with_attachments_json() {
        let json = r#"[{"blob_id":"blob-1","name":"file1.pdf","content_type":"application/pdf"},{"blob_id":"blob-2","name":"file2.png","content_type":"image/png"}]"#;
        let encoded_json = serde_urlencoded::to_string([("attachments_json", json)]).unwrap();
        let form_data = format!("to=test%40example.com&subject=Hello&{encoded_json}");
        let form: ComposeFormData =
            serde_urlencoded::from_str(&form_data).expect("deserialization should succeed");
        let attachments = build_attachments_from_form(&form);
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].blob_id.as_str(), "blob-1");
        assert_eq!(attachments[0].name, "file1.pdf");
        assert_eq!(attachments[1].blob_id.as_str(), "blob-2");
        assert_eq!(attachments[1].name, "file2.png");
    }

    #[test]
    fn compose_form_data_deserializes_no_attachments() {
        let form_data = "to=test%40example.com&subject=Hello";
        let form: ComposeFormData =
            serde_urlencoded::from_str(form_data).expect("deserialization should succeed");
        assert!(form.attachments_json.is_empty());
        let attachments = build_attachments_from_form(&form);
        assert!(attachments.is_empty());
    }
}
