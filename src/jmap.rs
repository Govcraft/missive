use std::collections::HashMap;
use std::sync::Arc;

use acton_service::prelude::{error, info};
use chrono::{DateTime, Local};
use dashmap::DashMap;
use jmap_client::{
    client::Client,
    email::{self, Email, EmailAddress},
    mailbox::{self, Role},
};

use serde::{Deserialize, Serialize};

use crate::error::MissiveError;

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

pub type JmapClientCache = Arc<DashMap<String, Arc<Client>>>;

pub fn new_client_cache() -> JmapClientCache {
    Arc::new(DashMap::new())
}

pub async fn get_or_create_client(
    cache: &JmapClientCache,
    jmap_url: &str,
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
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub received_at: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub blob_id: BlobId,
    pub name: String,
    pub size_display: String,
}

pub async fn create_client(
    jmap_url: &str,
    username: &str,
    password: &str,
) -> Result<Client, MissiveError> {
    let host = jmap_url
        .split("://")
        .nth(1)
        .unwrap_or(jmap_url)
        .split('/')
        .next()
        .unwrap_or(jmap_url);

    info!("Creating JMAP client: url={jmap_url}, host={host}, user={username}");

    let client = Client::new()
        .credentials((username, password))
        .follow_redirects([host])
        .connect(jmap_url)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            error!("JMAP connection error: {msg} | debug: {e:?}");
            if msg.contains("401") || msg.contains("403") || msg.contains("auth") {
                MissiveError::AuthFailed
            } else {
                MissiveError::Jmap(msg)
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
        MissiveError::Jmap(e.to_string())
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
) -> Result<Vec<EmailSummary>, MissiveError> {
    info!("Fetching emails: mailbox_id={mailbox_id}, position={position}, limit={limit}");
    let mut request = client.build();
    let query_req = request.query_email();
    query_req.filter(email::query::Filter::in_mailbox(mailbox_id.as_str()));
    query_req.sort([email::query::Comparator::received_at().descending()]);
    query_req.position(position as i32);
    query_req.limit(limit);

    let query_response = request
        .send_single::<jmap_client::core::query::QueryResponse>()
        .await
        .map_err(|e| {
            error!("JMAP email query error: {e}");
            MissiveError::Jmap(e.to_string())
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
        MissiveError::Jmap(e.to_string())
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
    ]);
    get_request.arguments().fetch_text_body_values(true);
    get_request.arguments().fetch_html_body_values(true);

    let response = request.send_get_email().await.map_err(|e| {
        error!("JMAP get email detail error: {e}");
        MissiveError::Jmap(e.to_string())
    })?;

    let email = response
        .list()
        .first()
        .ok_or_else(|| MissiveError::Jmap("Email not found".to_string()))?;

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
        from: format_addresses(email.from()),
        to: format_addresses(email.to()),
        cc: format_addresses(email.cc()),
        subject: email.subject().unwrap_or("(no subject)").to_string(),
        received_at: format_timestamp(email.received_at().unwrap_or(0), Local::now()),
        body_text,
        body_html,
        attachments,
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
        MissiveError::Jmap(e.to_string())
    })
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

    // --- role_sort_priority tests ---

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
}
