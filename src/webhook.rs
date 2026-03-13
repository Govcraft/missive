use std::time::Duration;

use acton_service::prelude::*;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use jmap_client::{
    client::Client,
    email::{self, Email, EmailAddress},
    event_source::PushNotification,
    DataType, Get,
};
use sha2::Sha256;

use crate::config::WebhookConfig;
use crate::jmap::{self, JmapUrl};

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebhookPayload {
    pub event: &'static str,
    pub email_id: String,
    pub message_id: Vec<String>,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub from: Vec<AddressPayload>,
    pub to: Vec<AddressPayload>,
    pub cc: Vec<AddressPayload>,
    pub preview: Option<String>,
    pub body_text: Option<String>,
    pub has_attachment: bool,
    pub received_at: Option<i64>,
    pub keywords: Vec<String>,
    pub size: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AddressPayload {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeletedPayload {
    pub event: &'static str,
    pub email_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum WebhookEvent<'a> {
    Full(&'a WebhookPayload),
    Deleted(&'a DeletedPayload),
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

pub fn email_to_webhook_payload(
    email: &Email<Get>,
    include_body: bool,
    event: &'static str,
) -> Option<WebhookPayload> {
    let email_id = email.id()?.to_string();

    let body_text = if include_body {
        extract_text_body(email)
    } else {
        None
    };

    Some(WebhookPayload {
        event,
        email_id,
        message_id: email
            .message_id()
            .unwrap_or_default()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        thread_id: email.thread_id().map(str::to_string),
        subject: email.subject().map(str::to_string),
        from: addresses_to_payload(email.from()),
        to: addresses_to_payload(email.to()),
        cc: addresses_to_payload(email.cc()),
        preview: email.preview().map(str::to_string),
        body_text,
        has_attachment: email.has_attachment(),
        received_at: email.received_at(),
        keywords: email.keywords().into_iter().map(str::to_string).collect(),
        size: email.size(),
    })
}

pub fn addresses_to_payload(addrs: Option<&[EmailAddress<Get>]>) -> Vec<AddressPayload> {
    addrs
        .unwrap_or_default()
        .iter()
        .map(|a| AddressPayload {
            name: a.name().map(str::to_string),
            email: a.email().to_string(),
        })
        .collect()
}

pub fn sign_payload(body: &[u8], secret: &str) -> String {
    // HmacSha256::new_from_slice only fails for invalid key length,
    // which cannot happen with HMAC-SHA256 (accepts any length).
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        unreachable!("HMAC-SHA256 accepts any key length");
    };
    mac.update(body);
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}

fn extract_text_body(email: &Email<Get>) -> Option<String> {
    let text_parts = email.text_body()?;
    for part in text_parts {
        if let Some(part_id) = part.part_id()
            && let Some(body_value) = email.body_value(part_id)
        {
            return Some(body_value.value().to_string());
        }
    }
    None
}

fn classify_email_change(notification: &PushNotification) -> bool {
    match notification {
        PushNotification::StateChange(changes) => {
            changes.has_type(DataType::Email) || changes.has_type(DataType::EmailDelivery)
        }
        _ => false,
    }
}

fn webhook_email_properties(include_body: bool) -> Vec<email::Property> {
    let mut props = vec![
        email::Property::Id,
        email::Property::ThreadId,
        email::Property::MailboxIds,
        email::Property::Keywords,
        email::Property::Size,
        email::Property::ReceivedAt,
        email::Property::MessageId,
        email::Property::From,
        email::Property::To,
        email::Property::Cc,
        email::Property::Subject,
        email::Property::Preview,
        email::Property::HasAttachment,
    ];
    if include_body {
        props.push(email::Property::TextBody);
        props.push(email::Property::BodyValues);
    }
    props
}

// ---------------------------------------------------------------------------
// Async helpers
// ---------------------------------------------------------------------------

async fn post_webhook(
    client: &reqwest::Client,
    url: &str,
    payload: &WebhookEvent<'_>,
    secret: Option<&str>,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(payload)?;

    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.clone());

    if let Some(secret) = secret {
        let signature = sign_payload(&body, secret);
        request = request.header("X-Signature", signature);
    }

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        anyhow::bail!("webhook POST returned {status}: {body_text}");
    }

    Ok(())
}

async fn get_initial_state(jmap: &Client) -> anyhow::Result<String> {
    let mut request = jmap.build();
    let empty: Vec<&str> = Vec::new();
    request
        .get_email()
        .ids(empty)
        .properties([email::Property::Id]);

    let response = request.send_get_email().await.map_err(|e| {
        anyhow::anyhow!("failed to get initial JMAP state: {e}")
    })?;

    Ok(response.state().to_string())
}

async fn fetch_and_post_emails(
    jmap: &Client,
    http: &reqwest::Client,
    ids: &[String],
    config: &WebhookConfig,
    event: &'static str,
) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    let properties = webhook_email_properties(config.include_body);

    let mut request = jmap.build();
    let get_request = request.get_email().ids(ids.iter().map(String::as_str));
    get_request.properties(properties);
    if config.include_body {
        get_request.arguments().fetch_text_body_values(true);
    }

    let response = request.send_get_email().await.map_err(|e| {
        anyhow::anyhow!("failed to fetch emails: {e}")
    })?;

    for email in response.list() {
        let Some(payload) = email_to_webhook_payload(email, config.include_body, event) else {
            warn!("webhook: skipping email with no ID");
            continue;
        };

        let email_id = &payload.email_id;
        let subject = payload.subject.as_deref().unwrap_or("(no subject)");
        info!("webhook: posting {event} for {email_id} — {subject}");

        if let Err(e) = post_webhook(http, &config.url, &WebhookEvent::Full(&payload), config.secret.as_deref()).await {
            error!("webhook: POST failed for {email_id}: {e}");
        }
    }

    Ok(())
}

async fn post_deleted_emails(
    http: &reqwest::Client,
    ids: &[String],
    config: &WebhookConfig,
) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    for email_id in ids {
        let payload = DeletedPayload {
            event: "email.deleted",
            email_id: email_id.clone(),
        };

        info!("webhook: posting email.deleted for {email_id}");

        if let Err(e) = post_webhook(
            http,
            &config.url,
            &WebhookEvent::Deleted(&payload),
            config.secret.as_deref(),
        )
        .await
        {
            error!("webhook: POST failed for deleted {email_id}: {e}");
        }
    }

    Ok(())
}

async fn process_email_changes(
    jmap: &Client,
    http: &reqwest::Client,
    since_state: &str,
    config: &WebhookConfig,
) -> anyhow::Result<String> {
    let mut changes = jmap.email_changes(since_state, None).await.map_err(|e| {
        anyhow::anyhow!("email_changes failed: {e}")
    })?;

    let created = changes.take_created();
    let updated = changes.take_updated();
    let destroyed = changes.take_destroyed();
    let new_state = changes.take_new_state();

    info!(
        "webhook: changes since {since_state}: {} created, {} updated, {} destroyed → {new_state}",
        created.len(),
        updated.len(),
        destroyed.len()
    );

    if !created.is_empty() {
        info!("webhook: {} new email(s) since state {since_state}", created.len());
        fetch_and_post_emails(jmap, http, &created, config, "email.received").await?;
    }

    if !updated.is_empty() {
        info!("webhook: {} updated email(s) since state {since_state}", updated.len());
        fetch_and_post_emails(jmap, http, &updated, config, "email.updated").await?;
    }

    if !destroyed.is_empty() {
        info!("webhook: {} deleted email(s) since state {since_state}", destroyed.len());
        post_deleted_emails(http, &destroyed, config).await?;
    }

    Ok(new_state)
}

// ---------------------------------------------------------------------------
// Main worker task
// ---------------------------------------------------------------------------

pub async fn run_webhook_worker(
    jmap_url: JmapUrl,
    config: WebhookConfig,
) -> anyhow::Result<()> {
    info!("webhook worker starting, target: {}", config.url);

    let jmap = jmap::create_client(&jmap_url, &config.jmap_username, &config.jmap_password)
        .await
        .map_err(|e| anyhow::anyhow!("webhook: JMAP connection failed: {e}"))?;

    info!("webhook worker connected to JMAP");

    let mut since_state = get_initial_state(&jmap).await?;
    info!("webhook: initial state = {since_state}");

    let http = reqwest::Client::new();
    let ping_interval = config.ping_interval;
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!("webhook: opening EventSource stream");

        let stream_result = jmap
            .event_source(
                Some([DataType::Email, DataType::EmailDelivery]),
                false,
                Some(ping_interval),
                None,
            )
            .await;

        let stream = match stream_result {
            Ok(s) => {
                backoff = Duration::from_secs(1);
                s
            }
            Err(e) => {
                error!("webhook: failed to open EventSource: {e}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        let mut stream = std::pin::pin!(stream);

        while let Some(event) = stream.next().await {
            match event {
                Ok(ref notification) if classify_email_change(notification) => {
                    debug!("webhook: email state change detected");
                    match process_email_changes(&jmap, &http, &since_state, &config).await {
                        Ok(new_state) => since_state = new_state,
                        Err(e) => error!("webhook: failed to process changes: {e}"),
                    }
                }
                Ok(_) => {
                    trace!("webhook: non-email event, ignoring");
                }
                Err(e) => {
                    error!("webhook: EventSource error: {e}");
                    break;
                }
            }
        }

        info!("webhook: EventSource stream ended, catching up on missed events");
        match process_email_changes(&jmap, &http, &since_state, &config).await {
            Ok(new_state) => since_state = new_state,
            Err(e) => error!("webhook: failed to catch up after disconnect: {e}"),
        }

        info!("webhook: reconnecting in {}s", backoff.as_secs());
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jmap_client::event_source::Changes;

    #[test]
    fn test_sign_payload_sha256_format() {
        let signature = sign_payload(b"hello world", "secret");
        assert!(signature.starts_with("sha256="));
        // known HMAC-SHA256 of "hello world" with key "secret"
        assert_eq!(
            signature,
            "sha256=734cc62f32841568f45715aeb9f4d7891324e6d948e4c6c60c0621cdac48623a"
        );
    }

    #[test]
    fn test_sign_payload_empty_body() {
        let signature = sign_payload(b"", "secret");
        assert!(signature.starts_with("sha256="));
        assert_eq!(signature.len(), "sha256=".len() + 64);
    }

    #[test]
    fn test_addresses_to_payload_none() {
        let result = addresses_to_payload(None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_addresses_to_payload_empty() {
        let result = addresses_to_payload(Some(&[]));
        assert!(result.is_empty());
    }

    fn make_state_change(types: &[DataType]) -> PushNotification {
        let mut account_changes = std::collections::HashMap::<DataType, String>::new();
        for dt in types {
            account_changes.insert(dt.clone(), "state-1".to_string());
        }
        let mut account_map = std::collections::HashMap::new();
        account_map.insert("account-1".to_string(), account_changes);
        let json = serde_json::json!({
            "id": null,
            "changes": account_map,
        });
        let changes: Changes = serde_json::from_value(json).unwrap();
        PushNotification::StateChange(changes)
    }

    #[test]
    fn test_classify_email_change_true_for_email() {
        let notification = make_state_change(&[DataType::Email]);
        assert!(classify_email_change(&notification));
    }

    #[test]
    fn test_classify_email_change_true_for_delivery() {
        let notification = make_state_change(&[DataType::EmailDelivery]);
        assert!(classify_email_change(&notification));
    }

    #[test]
    fn test_classify_email_change_false_for_mailbox() {
        let notification = make_state_change(&[DataType::Mailbox]);
        assert!(!classify_email_change(&notification));
    }

    #[test]
    fn test_classify_email_change_false_for_empty() {
        let notification = make_state_change(&[]);
        assert!(!classify_email_change(&notification));
    }

    #[test]
    fn test_webhook_email_properties_without_body() {
        let props = webhook_email_properties(false);
        assert!(!props.contains(&email::Property::TextBody));
        assert!(!props.contains(&email::Property::BodyValues));
        assert!(props.contains(&email::Property::Subject));
    }

    #[test]
    fn test_webhook_email_properties_with_body() {
        let props = webhook_email_properties(true);
        assert!(props.contains(&email::Property::TextBody));
        assert!(props.contains(&email::Property::BodyValues));
    }

    #[test]
    fn test_deleted_payload_serialization() {
        let payload = DeletedPayload {
            event: "email.deleted",
            email_id: "abc-123".to_string(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["event"], "email.deleted");
        assert_eq!(json["email_id"], "abc-123");
        assert_eq!(json.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_webhook_event_deleted_serializes_flat() {
        let payload = DeletedPayload {
            event: "email.deleted",
            email_id: "xyz-789".to_string(),
        };
        let event = WebhookEvent::Deleted(&payload);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "email.deleted");
        assert_eq!(json["email_id"], "xyz-789");
        assert_eq!(json.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_sign_deleted_payload() {
        let payload = DeletedPayload {
            event: "email.deleted",
            email_id: "test-id".to_string(),
        };
        let body = serde_json::to_vec(&payload).unwrap();
        let signature = sign_payload(&body, "secret");
        assert!(signature.starts_with("sha256="));
        assert_eq!(signature.len(), "sha256=".len() + 64);
    }
}
