use std::collections::HashMap;
use std::sync::Arc;

use acton_service::prelude::{error, info};
use chrono::{DateTime, Local};
use dashmap::DashMap;
use jmap_client::{
    client::Client,
    email::{self, Email, EmailAddress, EmailBodyPart},
    identity,
    mailbox::{self, Role},
};

use serde::{Deserialize, Serialize};

use crate::error::{JmapErrorKind, MissiveError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery(String);

impl SearchQuery {
    pub fn new(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(Self(trimmed.to_string()))
        }
    }
}

impl std::fmt::Display for SearchQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SearchQuery {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

fn build_email_filter(
    mailbox_id: &MailboxId,
    search: Option<&SearchQuery>,
) -> jmap_client::core::query::Filter<email::query::Filter> {
    let mailbox = email::query::Filter::in_mailbox(mailbox_id.as_str());
    match search {
        Some(q) => jmap_client::core::query::Filter::and([
            mailbox,
            email::query::Filter::text(q.as_ref()),
        ]),
        None => mailbox.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JmapUrlError(String);

impl std::fmt::Display for JmapUrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid JMAP URL: {}", self.0)
    }
}

impl std::error::Error for JmapUrlError {}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JmapUrl(String);

impl JmapUrl {
    pub fn parse(s: &str) -> Result<Self, JmapUrlError> {
        if s.is_empty() {
            return Ok(Self(String::new()));
        }
        if !s.contains("://") {
            return Err(JmapUrlError(
                "URL must include a scheme (e.g., https://)".to_string(),
            ));
        }
        let after_scheme = s.split("://").nth(1).unwrap_or("");
        let host = after_scheme.split('/').next().unwrap_or("");
        if host.is_empty() {
            return Err(JmapUrlError("URL must include a host".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn validate(&self) -> Result<(), JmapUrlError> {
        if self.0.is_empty() {
            return Ok(());
        }
        Self::parse(&self.0)?;
        Ok(())
    }

    pub fn host(&self) -> &str {
        self.0
            .split("://")
            .nth(1)
            .unwrap_or(&self.0)
            .split('/')
            .next()
            .unwrap_or(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::str::FromStr for JmapUrl {
    type Err = JmapUrlError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl std::fmt::Display for JmapUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for JmapUrl {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for JmapUrl {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

define_id!(MailboxId);
define_id!(EmailId);
define_id!(BlobId);
define_id!(IdentityId);

#[derive(Debug, Clone)]
pub struct IdentityInfo {
    pub id: IdentityId,
    pub name: String,
    pub email: String,
}

pub type JmapClientCache = Arc<DashMap<String, Arc<Client>>>;

pub fn new_client_cache() -> JmapClientCache {
    Arc::new(DashMap::new())
}

pub async fn get_or_create_client(
    cache: &JmapClientCache,
    jmap_url: &JmapUrl,
    username: &str,
    password: &str,
) -> Result<Arc<Client>, MissiveError> {
    if let Some(client) = cache.get(username) {
        return Ok(Arc::clone(client.value()));
    }
    let client = create_client(jmap_url, username, password).await?;
    let client = Arc::new(client);
    cache.insert(username.to_string(), Arc::clone(&client));
    Ok(client)
}
use crate::sanitize::sanitize_email_html;

#[derive(Debug, Clone)]
pub struct MailboxInfo {
    pub id: MailboxId,
    pub name: String,
    pub role: String,
    pub unread_count: usize,
}

#[derive(Debug, Clone)]
pub struct EmailSummary {
    pub id: EmailId,
    pub from: String,
    pub subject: String,
    pub received_at: String,
    pub preview: String,
    pub is_unread: bool,
    pub has_attachment: bool,
}

#[derive(Debug, Clone)]
pub struct EmailDetail {
    pub id: EmailId,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub received_at: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
    pub message_id: Vec<String>,
    pub references: Vec<String>,
    pub from_email: String,
    pub to_emails: String,
    pub cc_emails: String,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub blob_id: BlobId,
    pub name: String,
    pub size_display: String,
}

pub async fn create_client(
    jmap_url: &JmapUrl,
    username: &str,
    password: &str,
) -> Result<Client, MissiveError> {
    let host = jmap_url.host();

    info!("Creating JMAP client: url={jmap_url}, host={host}, user={username}");

    let client = Client::new()
        .credentials((username, password))
        .follow_redirects([host])
        .connect(jmap_url.as_str())
        .await
        .map_err(|e| {
            let msg = e.to_string();
            error!("JMAP connection error: {msg} | debug: {e:?}");
            if msg.contains("401") || msg.contains("403") || msg.contains("auth") {
                MissiveError::AuthFailed
            } else {
                MissiveError::Jmap(JmapErrorKind::ConnectionFailed {
                    url: jmap_url.to_string(),
                    message: msg,
                })
            }
        })?;

    info!("JMAP client connected successfully for user={username}");
    Ok(client)
}

pub async fn fetch_mailboxes(client: &Client) -> Result<Vec<MailboxInfo>, MissiveError> {
    info!("Fetching mailboxes from JMAP server");
    let mut request = client.build();
    request.get_mailbox().properties([
        mailbox::Property::Id,
        mailbox::Property::Name,
        mailbox::Property::Role,
        mailbox::Property::UnreadEmails,
    ]);

    let response = request.send_get_mailbox().await.map_err(|e| {
        error!("JMAP fetch mailboxes error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Mailbox/get".to_string(),
            message: e.to_string(),
        })
    })?;

    let mut mailboxes: Vec<MailboxInfo> = response
        .list()
        .iter()
        .map(|m| MailboxInfo {
            id: MailboxId::from(m.id().unwrap_or_default()),
            name: m.name().unwrap_or("(unnamed)").to_string(),
            role: role_to_string(&m.role()),
            unread_count: m.unread_emails(),
        })
        .collect();

    info!("Fetched {} mailboxes from JMAP", mailboxes.len());

    mailboxes.sort_by(|a, b| {
        let a_priority = role_sort_priority(&a.role);
        let b_priority = role_sort_priority(&b.role);
        a_priority
            .cmp(&b_priority)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(mailboxes)
}

pub async fn fetch_emails(
    client: &Client,
    mailbox_id: &MailboxId,
    position: usize,
    limit: usize,
    search: Option<&SearchQuery>,
) -> Result<Vec<EmailSummary>, MissiveError> {
    info!("Fetching emails: mailbox_id={mailbox_id}, position={position}, limit={limit}, search={search:?}");
    let mut request = client.build();
    let query_req = request.query_email();
    query_req.filter(build_email_filter(mailbox_id, search));
    query_req.sort([email::query::Comparator::received_at().descending()]);
    query_req.position(position as i32);
    query_req.limit(limit);

    let query_response = request
        .send_single::<jmap_client::core::query::QueryResponse>()
        .await
        .map_err(|e| {
            error!("JMAP email query error: {e}");
            MissiveError::Jmap(JmapErrorKind::QueryFailed {
                method: "Email/query".to_string(),
                message: e.to_string(),
            })
        })?;

    let ids: Vec<&str> = query_response.ids().iter().map(|s| s.as_str()).collect();
    info!("Email query returned {} ids", ids.len());
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut request = client.build();
    let get_request = request.get_email().ids(ids.iter().copied());
    get_request.properties([
        email::Property::Id,
        email::Property::From,
        email::Property::Subject,
        email::Property::ReceivedAt,
        email::Property::Preview,
        email::Property::Keywords,
        email::Property::HasAttachment,
    ]);

    let response = request.send_get_email().await.map_err(|e| {
        error!("JMAP get emails error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/get".to_string(),
            message: e.to_string(),
        })
    })?;

    let emails: Vec<EmailSummary> = response
        .list()
        .iter()
        .map(|e| EmailSummary {
            id: EmailId::from(e.id().unwrap_or_default()),
            from: format_addresses(e.from()),
            subject: e.subject().unwrap_or("(no subject)").to_string(),
            received_at: format_timestamp(e.received_at().unwrap_or(0), Local::now()),
            preview: e.preview().unwrap_or_default().to_string(),
            is_unread: !e.keywords().contains(&"$seen"),
            has_attachment: e.has_attachment(),
        })
        .collect();

    Ok(emails)
}

pub async fn fetch_email_detail(
    client: &Client,
    email_id: &EmailId,
) -> Result<EmailDetail, MissiveError> {
    info!("Fetching email detail: id={email_id}");
    let mut request = client.build();
    let get_request = request.get_email().ids([email_id.as_str()]);
    get_request.properties([
        email::Property::From,
        email::Property::To,
        email::Property::Cc,
        email::Property::Subject,
        email::Property::ReceivedAt,
        email::Property::BodyValues,
        email::Property::TextBody,
        email::Property::HtmlBody,
        email::Property::Attachments,
        email::Property::MessageId,
        email::Property::References,
    ]);
    get_request.arguments().fetch_text_body_values(true);
    get_request.arguments().fetch_html_body_values(true);

    let response = request.send_get_email().await.map_err(|e| {
        error!("JMAP get email detail error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/get".to_string(),
            message: e.to_string(),
        })
    })?;

    let email = response
        .list()
        .first()
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::NotFound {
                resource: "Email".to_string(),
                id: email_id.to_string(),
            })
        })?;

    let body_text = extract_text_body(email);
    let cid_map = build_cid_map(email);
    let body_html = extract_html_body(email, &cid_map);

    let attachments = email
        .attachments()
        .unwrap_or_default()
        .iter()
        .filter_map(|part| {
            if part.content_id().is_some() {
                return None;
            }
            let blob_id = BlobId::from(part.blob_id()?.to_string());
            Some(AttachmentInfo {
                name: part.name().unwrap_or("attachment").to_string(),
                size_display: format_file_size(part.size()),
                blob_id,
            })
        })
        .collect();

    Ok(EmailDetail {
        id: email_id.clone(),
        from: format_addresses(email.from()),
        to: format_addresses(email.to()),
        cc: format_addresses(email.cc()),
        subject: email.subject().unwrap_or("(no subject)").to_string(),
        received_at: format_timestamp(email.received_at().unwrap_or(0), Local::now()),
        body_text,
        body_html,
        attachments,
        message_id: email.message_id().unwrap_or_default().to_vec(),
        references: email.references().unwrap_or_default().to_vec(),
        from_email: format_addresses_raw(email.from()),
        to_emails: format_addresses_raw(email.to()),
        cc_emails: format_addresses_raw(email.cc()),
    })
}

fn format_file_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub async fn download_blob(client: &Client, blob_id: &BlobId) -> Result<Vec<u8>, MissiveError> {
    info!("Downloading blob: id={blob_id}");
    client.download(blob_id.as_str()).await.map_err(|e| {
        error!("JMAP blob download error: {e}");
        MissiveError::Jmap(JmapErrorKind::BlobDownloadFailed {
            blob_id: blob_id.to_string(),
            message: e.to_string(),
        })
    })
}

pub async fn fetch_identities(client: &Client) -> Result<Vec<IdentityInfo>, MissiveError> {
    info!("Fetching identities from JMAP server");
    let mut request = client.build();
    request.get_identity().properties([
        identity::Property::Id,
        identity::Property::Name,
        identity::Property::Email,
    ]);

    let response = request.send_get_identity().await.map_err(|e| {
        error!("JMAP fetch identities error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Identity/get".to_string(),
            message: e.to_string(),
        })
    })?;

    let identities: Vec<IdentityInfo> = response
        .list()
        .iter()
        .map(|i| IdentityInfo {
            id: IdentityId::from(i.id().unwrap_or_default()),
            name: i.name().unwrap_or_default().to_string(),
            email: i.email().unwrap_or_default().to_string(),
        })
        .collect();

    info!("Fetched {} identities from JMAP", identities.len());
    Ok(identities)
}

#[derive(Default)]
pub struct ThreadingHeaders<'a> {
    pub in_reply_to: Option<&'a str>,
    pub references: Option<&'a str>,
}

pub struct EmailContent<'a> {
    pub from_email: &'a str,
    pub to: &'a str,
    pub cc: &'a str,
    pub subject: &'a str,
    pub body_text: &'a str,
    pub threading: &'a ThreadingHeaders<'a>,
}

fn parse_recipient_emails(input: &str) -> Vec<&str> {
    input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_address_list(input: &str) -> Vec<EmailAddress> {
    parse_recipient_emails(input)
        .into_iter()
        .map(EmailAddress::from)
        .collect()
}

pub async fn save_draft(
    client: &Client,
    content: &EmailContent<'_>,
) -> Result<(), MissiveError> {
    let mailboxes = fetch_mailboxes(client).await?;
    let drafts_id = mailboxes
        .iter()
        .find(|m| m.role == "drafts")
        .map(|m| m.id.as_str().to_string())
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::NoMailbox {
                role: "drafts".to_string(),
            })
        })?;

    let mut request = client.build();
    let email = request.set_email().create();
    email
        .mailbox_ids([&drafts_id])
        .from([EmailAddress::from(content.from_email)])
        .subject(content.subject)
        .keywords(["$draft"])
        .body_value("body".to_string(), content.body_text)
        .text_body(
            EmailBodyPart::new()
                .content_type("text/plain")
                .part_id("body"),
        );

    let to_addrs = parse_address_list(content.to);
    if !to_addrs.is_empty() {
        email.to(to_addrs);
    }

    let cc_addrs = parse_address_list(content.cc);
    if !cc_addrs.is_empty() {
        email.cc(cc_addrs);
    }

    if let Some(irt) = content.threading.in_reply_to.filter(|s| !s.is_empty()) {
        let ids: Vec<String> = irt.split_whitespace().map(String::from).collect();
        email.in_reply_to(ids);
    }
    if let Some(refs) = content.threading.references.filter(|s| !s.is_empty()) {
        let ids: Vec<String> = refs.split_whitespace().map(String::from).collect();
        email.references(ids);
    }

    request.send_set_email().await.map_err(|e| {
        error!("JMAP save draft error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/set".to_string(),
            message: e.to_string(),
        })
    })?;

    info!("Draft saved successfully");
    Ok(())
}

pub async fn send_email(
    client: &Client,
    identity_id: &IdentityId,
    content: &EmailContent<'_>,
) -> Result<(), MissiveError> {
    let to_addrs = parse_address_list(content.to);
    if to_addrs.is_empty() {
        return Err(MissiveError::Jmap(JmapErrorKind::NoRecipient));
    }

    // Find Drafts and Sent mailboxes
    let mailboxes = fetch_mailboxes(client).await?;
    let drafts_id = mailboxes
        .iter()
        .find(|m| m.role == "drafts")
        .map(|m| m.id.as_str().to_string())
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::NoMailbox {
                role: "drafts".to_string(),
            })
        })?;
    let sent_id = mailboxes
        .iter()
        .find(|m| m.role == "sent")
        .map(|m| m.id.as_str().to_string())
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::NoMailbox {
                role: "sent".to_string(),
            })
        })?;

    // Step 1: Create email via Email/set (placed in Drafts initially)
    let mut request = client.build();
    let email = request.set_email().create();
    email
        .mailbox_ids([&drafts_id])
        .from([EmailAddress::from(content.from_email)])
        .to(to_addrs)
        .subject(content.subject)
        .body_value("body".to_string(), content.body_text)
        .text_body(
            EmailBodyPart::new()
                .content_type("text/plain")
                .part_id("body"),
        );

    let cc_addrs = parse_address_list(content.cc);
    if !cc_addrs.is_empty() {
        email.cc(cc_addrs);
    }

    if let Some(irt) = content.threading.in_reply_to.filter(|s| !s.is_empty()) {
        let ids: Vec<String> = irt.split_whitespace().map(String::from).collect();
        email.in_reply_to(ids);
    }
    if let Some(refs) = content.threading.references.filter(|s| !s.is_empty()) {
        let ids: Vec<String> = refs.split_whitespace().map(String::from).collect();
        email.references(ids);
    }

    let mut response = request.send_set_email().await.map_err(|e| {
        error!("JMAP Email/set error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/set".to_string(),
            message: e.to_string(),
        })
    })?;

    let created_email = response
        .created("c0")
        .map_err(|e| {
            MissiveError::Jmap(JmapErrorKind::SubmissionFailed {
                message: e.to_string(),
            })
        })?;
    let email_id = created_email
        .id()
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::SubmissionFailed {
                message: "Created email has no ID".to_string(),
            })
        })?
        .to_string();

    // Step 2: Submit via EmailSubmission/set with onSuccessUpdateEmail
    // to move the email from Drafts to Sent (and Inbox if self-addressed)
    info!("Submitting email: id={email_id}");
    let mut request = client.build();
    let submit_req = request.set_email_submission();
    let mut rcpt_to = parse_recipient_emails(content.to);
    rcpt_to.extend(parse_recipient_emails(content.cc));

    submit_req
        .create()
        .email_id(&email_id)
        .identity_id(identity_id.as_str())
        .envelope(content.from_email, rcpt_to.iter().copied());

    // When sender is also a recipient, Stalwart's duplicate detection will
    // skip local SMTP delivery (same Message-ID already exists in account).
    // Add Inbox mailbox directly so the email appears in both Sent and Inbox.
    let self_addressed = rcpt_to
        .iter()
        .any(|r| r.eq_ignore_ascii_case(content.from_email));

    let on_success = submit_req
        .arguments()
        .on_success_update_email("c0")
        .mailbox_id(&drafts_id, false)
        .mailbox_id(&sent_id, true);

    if self_addressed {
        let inbox_id = mailboxes
            .iter()
            .find(|m| m.role == "inbox")
            .map(|m| m.id.as_str().to_string())
            .ok_or_else(|| {
                MissiveError::Jmap(JmapErrorKind::NoMailbox {
                    role: "inbox".to_string(),
                })
            })?;
        on_success.mailbox_id(&inbox_id, true);
    }
    request.send().await.map_err(|e| {
        error!("JMAP EmailSubmission/set error: {e}");
        MissiveError::Jmap(JmapErrorKind::SubmissionFailed {
            message: e.to_string(),
        })
    })?;

    info!("Email sent successfully");
    Ok(())
}

pub async fn mark_email_read(client: &Client, email_id: &EmailId) -> Result<(), MissiveError> {
    let mut request = client.build();
    request
        .set_email()
        .update(email_id.as_str())
        .keyword("$seen", true);
    request.send_set_email().await.map_err(|e| {
        error!("JMAP Email/set keyword update error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/set".to_string(),
            message: e.to_string(),
        })
    })?;
    Ok(())
}

pub async fn mark_email_unread(client: &Client, email_id: &EmailId) -> Result<(), MissiveError> {
    let mut request = client.build();
    request
        .set_email()
        .update(email_id.as_str())
        .keyword("$seen", false);
    request.send_set_email().await.map_err(|e| {
        error!("JMAP Email/set keyword update error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/set".to_string(),
            message: e.to_string(),
        })
    })?;
    Ok(())
}

pub async fn delete_email(client: &Client, email_id: &EmailId) -> Result<(), MissiveError> {
    info!("Deleting email: id={email_id}");
    let mut request = client.build();
    request.set_email().destroy([email_id.as_str()]);
    let mut response = request.send_set_email().await.map_err(|e| {
        error!("JMAP Email/set destroy error: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/set".to_string(),
            message: e.to_string(),
        })
    })?;
    response.destroyed(email_id.as_str()).map_err(|e| {
        error!("JMAP destroy failed for {email_id}: {e}");
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Email/set".to_string(),
            message: format!("Failed to delete email: {e}"),
        })
    })?;
    info!("Email deleted successfully: id={email_id}");
    Ok(())
}

fn format_timestamp(ts: i64, now: DateTime<Local>) -> String {
    use chrono::Utc;
    let dt: DateTime<Local> = DateTime::<Utc>::from_timestamp(ts, 0)
        .unwrap_or_default()
        .with_timezone(&Local);

    if dt.date_naive() == now.date_naive() {
        dt.format("%l:%M %p").to_string().trim().to_string()
    } else if now.signed_duration_since(dt).num_days() < 7 {
        dt.format("%a %l:%M %p").to_string().trim().to_string()
    } else if dt.format("%Y").to_string() == now.format("%Y").to_string() {
        dt.format("%b %e").to_string().trim().to_string()
    } else {
        dt.format("%b %e, %Y").to_string().trim().to_string()
    }
}

fn build_cid_map(email: &Email) -> HashMap<String, String> {
    let mut cid_map = HashMap::new();
    if let Some(attachments) = email.attachments() {
        for part in attachments {
            if let Some(cid) = part.content_id()
                && let Some(blob_id) = part.blob_id()
            {
                cid_map.insert(cid.to_string(), blob_id.to_string());
            }
        }
    }
    cid_map
}

fn extract_html_body(email: &Email, cid_map: &HashMap<String, String>) -> Option<String> {
    if let Some(html_parts) = email.html_body() {
        for part in html_parts {
            if let Some(part_id) = part.part_id()
                && let Some(body_value) = email.body_value(part_id)
            {
                let raw_html = body_value.value();
                if !raw_html.is_empty() {
                    return Some(sanitize_email_html(raw_html, cid_map));
                }
            }
        }
    }
    None
}

fn extract_text_body(email: &Email) -> String {
    if let Some(text_parts) = email.text_body() {
        for part in text_parts {
            if let Some(part_id) = part.part_id()
                && let Some(body_value) = email.body_value(part_id)
            {
                return body_value.value().to_string();
            }
        }
    }
    String::new()
}

fn format_addresses_raw(addresses: Option<&[EmailAddress]>) -> String {
    addresses
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| a.email().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn format_addresses(addresses: Option<&[EmailAddress]>) -> String {
    addresses
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| {
                    a.name()
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| a.email())
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn role_to_string(role: &Role) -> String {
    match role {
        Role::Inbox => "inbox".to_string(),
        Role::Sent => "sent".to_string(),
        Role::Drafts => "drafts".to_string(),
        Role::Trash => "trash".to_string(),
        Role::Junk => "junk".to_string(),
        Role::Archive => "archive".to_string(),
        Role::Important => "important".to_string(),
        Role::Other(s) => s.clone(),
        Role::None => String::new(),
    }
}

fn role_sort_priority(role: &str) -> u8 {
    match role {
        "inbox" => 0,
        "drafts" => 1,
        "sent" => 2,
        "archive" => 3,
        "junk" => 4,
        "trash" => 5,
        "" => 6,
        _ => 7,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use chrono::TimeZone;
    use jmap_client::mailbox::Role;

    // --- format_timestamp tests ---

    #[test]
    fn test_format_timestamp_today() {
        let now = Local.with_ymd_and_hms(2025, 6, 15, 18, 0, 0).unwrap();
        // 3:30 PM on same day in UTC => convert to local
        let ts = chrono::Utc
            .with_ymd_and_hms(2025, 6, 15, 15, 30, 0)
            .unwrap()
            .timestamp();
        let result = format_timestamp(ts, now);
        // Should show time only (no date), e.g. "3:30 PM" or local equivalent
        assert!(
            result.contains(":30"),
            "Expected time format, got: {result}"
        );
        assert!(
            !result.contains("Jun"),
            "Should not contain month for today, got: {result}"
        );
    }

    #[test]
    fn test_format_timestamp_this_week() {
        // "now" is Sunday Jun 15, timestamp is Wednesday Jun 11 (4 days ago)
        let now = Local.with_ymd_and_hms(2025, 6, 15, 18, 0, 0).unwrap();
        let dt_target = Local.with_ymd_and_hms(2025, 6, 11, 15, 30, 0).unwrap();
        let ts = dt_target.timestamp();
        let result = format_timestamp(ts, now);
        // Should show day + time like "Wed 3:30 PM"
        assert!(result.contains("Wed"), "Expected day name, got: {result}");
    }

    #[test]
    fn test_format_timestamp_this_year() {
        // "now" is Jun 15, timestamp is Jan 15 (same year, >7 days ago)
        let now = Local.with_ymd_and_hms(2025, 6, 15, 18, 0, 0).unwrap();
        let dt_target = Local.with_ymd_and_hms(2025, 1, 15, 15, 30, 0).unwrap();
        let ts = dt_target.timestamp();
        let result = format_timestamp(ts, now);
        assert!(
            result.contains("Jan") && result.contains("15"),
            "Expected 'Jan 15', got: {result}"
        );
        assert!(
            !result.contains("2025"),
            "Should not contain year for same year, got: {result}"
        );
    }

    #[test]
    fn test_format_timestamp_older_year() {
        let now = Local.with_ymd_and_hms(2025, 6, 15, 18, 0, 0).unwrap();
        let dt_target = Local.with_ymd_and_hms(2023, 1, 15, 15, 30, 0).unwrap();
        let ts = dt_target.timestamp();
        let result = format_timestamp(ts, now);
        assert!(
            result.contains("Jan") && result.contains("15") && result.contains("2023"),
            "Expected 'Jan 15, 2023', got: {result}"
        );
    }

    // --- format_file_size tests ---

    #[test]
    fn test_format_file_size_bytes() {
        assert_eq!(format_file_size(500), "500 B");
    }

    #[test]
    fn test_format_file_size_kb() {
        let result = format_file_size(2048);
        assert_eq!(result, "2.0 KB");
    }

    #[test]
    fn test_format_file_size_mb() {
        let result = format_file_size(2 * 1024 * 1024);
        assert_eq!(result, "2.0 MB");
    }

    // --- format_addresses tests ---

    #[test]
    fn test_format_addresses_none() {
        assert_eq!(format_addresses(None), "");
    }

    #[test]
    fn test_format_addresses_with_name() {
        let addrs: Vec<EmailAddress> = serde_json::from_value(serde_json::json!([
            {"name": "Alice", "email": "alice@example.com"}
        ]))
        .unwrap();
        assert_eq!(format_addresses(Some(&addrs)), "Alice");
    }

    #[test]
    fn test_format_addresses_without_name() {
        let addrs: Vec<EmailAddress> = serde_json::from_value(serde_json::json!([
            {"name": null, "email": "bob@example.com"}
        ]))
        .unwrap();
        assert_eq!(format_addresses(Some(&addrs)), "bob@example.com");
    }

    #[test]
    fn test_format_addresses_multiple() {
        let addrs: Vec<EmailAddress> = serde_json::from_value(serde_json::json!([
            {"name": "Alice", "email": "alice@example.com"},
            {"name": null, "email": "bob@example.com"}
        ]))
        .unwrap();
        assert_eq!(format_addresses(Some(&addrs)), "Alice, bob@example.com");
    }

    // --- role_to_string tests ---

    #[test]
    fn test_role_to_string_known() {
        assert_eq!(role_to_string(&Role::Inbox), "inbox");
        assert_eq!(role_to_string(&Role::Sent), "sent");
        assert_eq!(role_to_string(&Role::Drafts), "drafts");
        assert_eq!(role_to_string(&Role::Trash), "trash");
        assert_eq!(role_to_string(&Role::Junk), "junk");
        assert_eq!(role_to_string(&Role::Archive), "archive");
        assert_eq!(role_to_string(&Role::Important), "important");
    }

    #[test]
    fn test_role_to_string_other() {
        assert_eq!(role_to_string(&Role::Other("custom".into())), "custom");
    }

    #[test]
    fn test_role_to_string_none() {
        assert_eq!(role_to_string(&Role::None), "");
    }

    // --- typed ID tests ---

    #[test]
    fn typed_id_from_str() {
        let id = MailboxId::from("abc");
        assert_eq!(id.as_str(), "abc");
    }

    #[test]
    fn typed_id_from_string() {
        let id = EmailId::from("123".to_string());
        assert_eq!(id.as_str(), "123");
    }

    #[test]
    fn typed_id_display() {
        let id = BlobId::from("blob-xyz");
        assert_eq!(format!("{id}"), "blob-xyz");
    }

    #[test]
    fn typed_id_serde_roundtrip() {
        let id = MailboxId::from("test-id");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"test-id\"");
        let deserialized: MailboxId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    // --- JmapUrl tests ---

    #[test]
    fn jmap_url_host_with_scheme() {
        let url = JmapUrl::from("https://mail.example.com/jmap");
        assert_eq!(url.host(), "mail.example.com");
    }

    #[test]
    fn jmap_url_host_without_scheme() {
        let url = JmapUrl::from("mail.example.com");
        assert_eq!(url.host(), "mail.example.com");
    }

    #[test]
    fn jmap_url_host_with_port() {
        let url = JmapUrl::from("https://mail.example.com:443/jmap");
        assert_eq!(url.host(), "mail.example.com:443");
    }

    #[test]
    fn jmap_url_display() {
        let url = JmapUrl::from("https://mail.example.com");
        assert_eq!(format!("{url}"), "https://mail.example.com");
    }

    #[test]
    fn jmap_url_is_empty() {
        assert!(JmapUrl::default().is_empty());
        assert!(!JmapUrl::from("https://example.com").is_empty());
    }

    // --- role_sort_priority tests ---

    // --- parse_address_list tests ---

    #[test]
    fn parse_address_list_single() {
        let result = parse_address_list("alice@example.com");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].email(), "alice@example.com");
    }

    #[test]
    fn parse_address_list_multiple() {
        let result = parse_address_list("alice@example.com, bob@example.com");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].email(), "alice@example.com");
        assert_eq!(result[1].email(), "bob@example.com");
    }

    #[test]
    fn parse_address_list_trailing_leading_commas() {
        let result = parse_address_list(",alice@example.com,");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].email(), "alice@example.com");
    }

    #[test]
    fn parse_address_list_whitespace_trimmed() {
        let result = parse_address_list("  alice@example.com  ,  bob@example.com  ");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].email(), "alice@example.com");
        assert_eq!(result[1].email(), "bob@example.com");
    }

    #[test]
    fn parse_address_list_empty() {
        let result = parse_address_list("");
        assert!(result.is_empty());
    }

    // --- parse_recipient_emails tests ---

    #[test]
    fn parse_recipient_emails_to_only() {
        let result = parse_recipient_emails("alice@example.com, bob@example.com");
        assert_eq!(result, vec!["alice@example.com", "bob@example.com"]);
    }

    #[test]
    fn parse_recipient_emails_empty_cc() {
        let mut rcpt_to = parse_recipient_emails("alice@example.com");
        rcpt_to.extend(parse_recipient_emails(""));
        assert_eq!(rcpt_to, vec!["alice@example.com"]);
    }

    #[test]
    fn parse_recipient_emails_to_and_cc() {
        let mut rcpt_to = parse_recipient_emails("alice@example.com");
        rcpt_to.extend(parse_recipient_emails("bob@example.com, carol@example.com"));
        assert_eq!(
            rcpt_to,
            vec!["alice@example.com", "bob@example.com", "carol@example.com"]
        );
    }

    #[test]
    fn parse_recipient_emails_self_delivery() {
        let sender = "alice@example.com";
        let mut rcpt_to = parse_recipient_emails(sender);
        rcpt_to.extend(parse_recipient_emails(""));
        assert_eq!(rcpt_to, vec!["alice@example.com"]);
        assert!(rcpt_to.contains(&sender));
    }

    #[test]
    fn parse_recipient_emails_dedup_across_to_and_cc() {
        let mut rcpt_to = parse_recipient_emails("alice@example.com, bob@example.com");
        rcpt_to.extend(parse_recipient_emails("bob@example.com, carol@example.com"));
        rcpt_to.sort_unstable();
        rcpt_to.dedup();
        assert_eq!(
            rcpt_to,
            vec!["alice@example.com", "bob@example.com", "carol@example.com"]
        );
    }

    #[test]
    fn test_role_sort_priority() {
        assert_eq!(role_sort_priority("inbox"), 0);
        assert_eq!(role_sort_priority("drafts"), 1);
        assert_eq!(role_sort_priority("sent"), 2);
        assert_eq!(role_sort_priority("archive"), 3);
        assert_eq!(role_sort_priority("junk"), 4);
        assert_eq!(role_sort_priority("trash"), 5);
        assert_eq!(role_sort_priority(""), 6);
        assert_eq!(role_sort_priority("other"), 7);
    }

    // --- JmapUrl validation tests ---

    #[test]
    fn jmap_url_parse_valid() {
        let url = JmapUrl::parse("https://mail.example.com/jmap").unwrap();
        assert_eq!(url.as_str(), "https://mail.example.com/jmap");
    }

    #[test]
    fn jmap_url_parse_empty_allowed() {
        let url = JmapUrl::parse("").unwrap();
        assert!(url.is_empty());
    }

    #[test]
    fn jmap_url_parse_missing_scheme() {
        let err = JmapUrl::parse("mail.example.com").unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid JMAP URL: URL must include a scheme (e.g., https://)"
        );
    }

    #[test]
    fn jmap_url_parse_missing_host() {
        let err = JmapUrl::parse("https://").unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid JMAP URL: URL must include a host"
        );
    }

    #[test]
    fn jmap_url_from_str_valid() {
        let url: JmapUrl = "https://mail.example.com/jmap".parse().unwrap();
        assert_eq!(url.as_str(), "https://mail.example.com/jmap");
    }

    #[test]
    fn jmap_url_from_str_invalid() {
        let result: Result<JmapUrl, _> = "mail.example.com".parse();
        assert!(result.is_err());
    }

    #[test]
    fn jmap_url_validate_valid() {
        let url = JmapUrl::from("https://mail.example.com/jmap");
        assert!(url.validate().is_ok());
    }

    #[test]
    fn jmap_url_validate_empty() {
        let url = JmapUrl::default();
        assert!(url.validate().is_ok());
    }

    #[test]
    fn jmap_url_validate_invalid() {
        let url = JmapUrl::from("mail.example.com");
        assert!(url.validate().is_err());
    }

    // --- SearchQuery tests ---

    #[test]
    fn search_query_empty_returns_none() {
        assert!(SearchQuery::new("").is_none());
    }

    #[test]
    fn search_query_whitespace_returns_none() {
        assert!(SearchQuery::new("  ").is_none());
    }

    #[test]
    fn search_query_valid_returns_some() {
        let q = SearchQuery::new("hello").unwrap();
        assert_eq!(q.as_ref(), "hello");
    }

    #[test]
    fn search_query_trims_whitespace() {
        let q = SearchQuery::new(" hello world ").unwrap();
        assert_eq!(q.as_ref(), "hello world");
    }

    #[test]
    fn search_query_display() {
        let q = SearchQuery::new("test").unwrap();
        assert_eq!(format!("{q}"), "test");
    }

    // --- build_email_filter tests ---

    #[test]
    fn build_filter_without_search() {
        let mailbox_id = MailboxId::from("inbox-1");
        let filter = build_email_filter(&mailbox_id, None);
        // Without search, should be a simple condition (not an operator)
        let json = serde_json::to_string(&filter).unwrap();
        assert!(json.contains("inMailbox"));
        assert!(!json.contains("text"));
    }

    #[test]
    fn build_filter_with_search() {
        let mailbox_id = MailboxId::from("inbox-1");
        let query = SearchQuery::new("hello").unwrap();
        let filter = build_email_filter(&mailbox_id, Some(&query));
        let json = serde_json::to_string(&filter).unwrap();
        assert!(json.contains("inMailbox"));
        assert!(json.contains("\"text\""));
        assert!(json.contains("hello"));
        assert!(json.contains("AND"));
    }
}
