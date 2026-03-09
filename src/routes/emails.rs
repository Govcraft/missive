use std::sync::Arc;

use acton_service::prelude::*;
use acton_service::session::{FlashMessage, FlashMessages, Session};
use axum::body::Body;

use crate::config::MissiveConfig;
use crate::error::MissiveError;
use crate::jmap::{self, BlobId, EmailDetail, EmailId, EmailSummary, IdentityId, IdentityInfo, MailboxId, MailboxInfo, SearchQuery};
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
    pub in_reply_to: String,
    #[serde(default)]
    pub references_header: String,
}

pub async fn list_emails(
    State(state): State<AppState<MissiveConfig>>,
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Query(params): Query<EmailListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let search = params.search.as_deref().and_then(SearchQuery::new);
    debug!("list_emails: mailbox_id={}, search={search:?}", params.mailbox_id);
    let page_size = state.config().custom.page_size;
    let (emails, total_count) =
        jmap::fetch_emails(&client, &params.mailbox_id, params.position, page_size, search.as_ref()).await?;
    debug!(
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
    debug!("get_email: id={id}");
    let email = jmap::fetch_email_detail(&client, &id).await?;
    let mailboxes = jmap::fetch_mailboxes(&client).await?;
    debug!("get_email: returning email subject={}", email.subject);

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
    debug!("download_attachment: blob_id={blob_id}");
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
    AuthenticatedClient(client, username, session): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    debug!("compose_form: loading compose view for user={username}");
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
    info!("send_email: sending message to={}", form.to);
    let all_identities = jmap::fetch_identities(&client).await?;
    let identities = filter_identities_for_user(all_identities, &username);
    let identity = identities
        .iter()
        .find(|i| i.id.as_str() == form.identity_id);

    let from_email = match identity {
        Some(i) => i.email.clone(),
        None => {
            push_flash(&session, FlashMessage::error("Invalid sending identity")).await;
            return Ok(HtmlTemplate::page(ComposeFormTemplate { identities, form, title: "New Message".to_string() })
                .with_hx_trigger("flashUpdated")
                .into_response());
        }
    };

    let threading = jmap::ThreadingHeaders {
        in_reply_to: Some(form.in_reply_to.as_str()).filter(|s| !s.is_empty()),
        references: Some(form.references_header.as_str()).filter(|s| !s.is_empty()),
    };
    let content = jmap::EmailContent {
        from_email: &from_email,
        to: &form.to,
        cc: &form.cc,
        bcc: &form.bcc,
        subject: &form.subject,
        body_text: &form.body,
        threading: &threading,
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
            Ok(HtmlTemplate::page(ComposeFormTemplate { identities, form, title: "New Message".to_string() })
                .with_hx_trigger("flashUpdated")
                .into_response())
        }
    }
}

pub async fn save_draft(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Form(form): Form<ComposeFormData>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    info!("save_draft: saving draft");
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
    let content = jmap::EmailContent {
        from_email,
        to: &form.to,
        cc: &form.cc,
        bcc: &form.bcc,
        subject: &form.subject,
        body_text: &form.body,
        threading: &threading,
    };
    match jmap::save_draft(&client, &content).await {
        Ok(()) => {
            push_flash(&session, FlashMessage::success("Draft saved")).await;
            Ok(empty_state_with_flash())
        }
        Err(e) => {
            error!("save_draft failed: {e}");
            push_flash(&session, FlashMessage::error(e.to_string())).await;
            Ok(HtmlTemplate::page(ComposeFormTemplate { identities, form, title: "New Message".to_string() })
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
    debug!("delete_email: id={id}");
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
    debug!("archive_email: id={id}");
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
    debug!("spam_email: id={id}");
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
    debug!("unspam_email: id={id}");
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
    debug!("move_email: id={id} to={}", params.target_mailbox_id);
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

pub async fn mark_unread(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    debug!("mark_unread: id={id}");
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
    if trimmed.len() >= check.len()
        && trimmed[..check.len()].eq_ignore_ascii_case(&check)
    {
        trimmed.to_string()
    } else {
        format!("{prefix}: {trimmed}")
    }
}

fn quote_body(body: &str) -> String {
    body.lines().map(|line| format!("> {line}")).collect::<Vec<_>>().join("\n")
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
            let references_header =
                build_reply_references(&email.references, &email.message_id);
            let quoted = format!(
                "\n\nOn {}, {} wrote:\n{}",
                email.received_at,
                email.from,
                quote_body(&email.body_text)
            );
            (
                "Reply".to_string(),
                ComposeFormData {
                    to: email.from_email.clone(),
                    subject: prepend_subject_prefix(&email.subject, "Re"),
                    body: quoted,
                    in_reply_to,
                    references_header,
                    ..Default::default()
                },
            )
        }
        ComposeMode::ReplyAll => {
            let in_reply_to = email.message_id.first().cloned().unwrap_or_default();
            let references_header =
                build_reply_references(&email.references, &email.message_id);
            let quoted = format!(
                "\n\nOn {}, {} wrote:\n{}",
                email.received_at,
                email.from,
                quote_body(&email.body_text)
            );
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
    debug!("reply: id={id}");
    compose_for_email(&client, &username, &session, &id, ComposeMode::Reply).await
}

pub async fn reply_all(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    debug!("reply_all: id={id}");
    compose_for_email(&client, &username, &session, &id, ComposeMode::ReplyAll).await
}

pub async fn forward(
    AuthenticatedClient(client, username, session): AuthenticatedClient,
    Path(id): Path<EmailId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    debug!("forward: id={id}");
    compose_for_email(&client, &username, &session, &id, ComposeMode::Forward).await
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
        assert_eq!(
            remove_address("alice@example.com", "alice@example.com"),
            ""
        );
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
}
