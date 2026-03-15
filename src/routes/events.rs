use std::convert::Infallible;
use std::sync::Arc;

use crate::error::{JmapErrorKind, MissiveError};
use crate::jmap;
use crate::session::AuthenticatedClient;
use acton_service::prelude::*;
use futures_util::StreamExt;
use futures_util::stream;
use jmap_client::DataType;
use jmap_client::event_source::PushNotification;

fn classify_push_notification(notification: &PushNotification) -> Vec<&'static str> {
    match notification {
        PushNotification::StateChange(changes) => {
            let has_email =
                changes.has_type(DataType::Email) || changes.has_type(DataType::EmailDelivery);
            let has_mailbox = changes.has_type(DataType::Mailbox);

            let mut events = Vec::new();
            if has_email {
                events.push("emailsUpdated");
            }
            if has_mailbox {
                events.push("mailboxesUpdated");
            }
            events
        }
        _ => Vec::new(),
    }
}

/// SSE endpoint following the acton-service SseBroadcaster pattern.
///
/// Uses `subscribe_channel` for per-user event delivery and `stream::unfold`
/// over the broadcast receiver as documented. A spawned task bridges the JMAP
/// EventSource into the broadcaster. A oneshot channel carried in the unfold
/// state signals the task to exit when the SSE connection drops.
pub async fn event_stream(
    AuthenticatedClient(client, username, _session): AuthenticatedClient,
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> std::result::Result<
    Sse<impl futures_util::Stream<Item = std::result::Result<SseEvent, Infallible>>>,
    MissiveError,
> {
    info!("SSE: opening JMAP EventSource for {username}");

    let jmap_stream = client
        .event_source(
            Some([DataType::Email, DataType::EmailDelivery, DataType::Mailbox]),
            false,
            Some(60),
            None,
        )
        .await
        .map_err(|e| {
            error!("SSE: failed to open JMAP EventSource for {username}: {e}");
            MissiveError::Jmap(JmapErrorKind::ConnectionFailed {
                url: "EventSource".to_string(),
                message: e.to_string(),
            })
        })?;

    info!("SSE: JMAP EventSource connected for {username}");

    // Subscribe to user-specific channel (per the docs' User-Specific Updates pattern)
    let rx = broadcaster.subscribe_channel(&username).await;

    // Oneshot channel for lifecycle: when the SSE stream is dropped (client
    // disconnect or graceful shutdown), the sender drops, signaling the task.
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Send a test event to verify the full pipeline
    let connected_msg = BroadcastMessage::named("connected", "ok");
    match broadcaster
        .broadcast_to_channel(&username, connected_msg)
        .await
    {
        Ok(n) => info!("SSE: sent connected event to {n} receiver(s) for {username}"),
        Err(e) => error!("SSE: failed to send connected event for {username}: {e}"),
    }

    // Get initial email state for detecting new emails vs updates
    let initial_email_state = jmap::get_email_state(&client)
        .await
        .unwrap_or_default();
    info!("SSE: initial email state for {username}: {initial_email_state}");

    // Spawn a task that bridges JMAP EventSource → SseBroadcaster channel
    let bc = broadcaster.clone();
    let user = username.clone();
    let jmap_client = client.clone();
    tokio::spawn(async move {
        let mut jmap_stream = std::pin::pin!(jmap_stream);
        let mut shutdown_rx = shutdown_rx;
        let mut since_state = initial_email_state;
        loop {
            tokio::select! {
                event = jmap_stream.next() => {
                    match event {
                        Some(Ok(ref notification)) => {
                            debug!("SSE: raw JMAP event for {user}: {notification:?}");
                            let events = classify_push_notification(notification);
                            let has_email_change = events.contains(&"emailsUpdated");
                            for event_name in &events {
                                info!("SSE: emitting {event_name} for {user}");
                                let msg = BroadcastMessage::named(*event_name, "refresh");
                                match bc.broadcast_to_channel(&user, msg).await {
                                    Ok(n) => info!("SSE: broadcast delivered to {n} receiver(s) for {user}"),
                                    Err(e) => error!("SSE: broadcast send error for {user}: {e}"),
                                }
                            }
                            // Check if the email change includes newly created emails
                            if has_email_change {
                                match jmap_client.email_changes(&since_state, None).await {
                                    Ok(mut changes) => {
                                        let created = changes.take_created();
                                        since_state = changes.take_new_state();
                                        if !created.is_empty() {
                                            info!("SSE: {} new email(s) for {user}", created.len());
                                            let msg = BroadcastMessage::named("emailReceived", "new");
                                            match bc.broadcast_to_channel(&user, msg).await {
                                                Ok(n) => info!("SSE: emailReceived delivered to {n} receiver(s) for {user}"),
                                                Err(e) => error!("SSE: emailReceived send error for {user}: {e}"),
                                            }
                                        }
                                    }
                                    Err(e) => error!("SSE: email_changes failed for {user}: {e}"),
                                }
                            }
                        }
                        Some(Err(e)) => {
                            error!("SSE: JMAP event stream error for {user}: {e}");
                            break;
                        }
                        None => break,
                    }
                }
                _ = &mut shutdown_rx => {
                    info!("SSE: client disconnected, closing JMAP listener for {user}");
                    break;
                }
            }
        }
        info!("SSE: JMAP EventSource stream ended for {user}");
    });

    // Convert broadcast receiver into SSE stream using unfold (per the docs).
    // The shutdown_tx is carried in the state so it drops when the stream ends.
    // A Ctrl+C listener ensures the stream terminates during graceful shutdown,
    // which drops shutdown_tx and signals the spawned JMAP task to exit.
    let stream = stream::unfold(
        (rx, Some(shutdown_tx)),
        |(mut rx, shutdown_tx)| async move {
            loop {
                tokio::select! {
                    result = rx.recv() => {
                        match result {
                            Ok(msg) => {
                                let mut event = SseEvent::default().data(msg.data);
                                if let Some(event_type) = msg.event_type {
                                    event = event.event(event_type);
                                }
                                return Some((Ok(event), (rx, shutdown_tx)));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                warn!("SSE: broadcast receiver lagged, skipped {n} message(s)");
                                continue;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        info!("SSE: shutdown signal received, closing SSE stream");
                        return None;
                    }
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jmap_client::event_source::Changes;

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
    fn classify_email_change() {
        let notification = make_state_change(&[DataType::Email]);
        assert_eq!(
            classify_push_notification(&notification),
            vec!["emailsUpdated"]
        );
    }

    #[test]
    fn classify_email_delivery_change() {
        let notification = make_state_change(&[DataType::EmailDelivery]);
        assert_eq!(
            classify_push_notification(&notification),
            vec!["emailsUpdated"]
        );
    }

    #[test]
    fn classify_mailbox_change() {
        let notification = make_state_change(&[DataType::Mailbox]);
        assert_eq!(
            classify_push_notification(&notification),
            vec!["mailboxesUpdated"]
        );
    }

    #[test]
    fn classify_email_and_mailbox_emits_both() {
        let notification = make_state_change(&[DataType::Email, DataType::Mailbox]);
        let events = classify_push_notification(&notification);
        assert!(events.contains(&"emailsUpdated"));
        assert!(events.contains(&"mailboxesUpdated"));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn classify_unrelated_type_returns_empty() {
        let notification = make_state_change(&[DataType::Identity]);
        assert!(classify_push_notification(&notification).is_empty());
    }

    #[test]
    fn classify_empty_state_change_returns_empty() {
        let notification = make_state_change(&[]);
        assert!(classify_push_notification(&notification).is_empty());
    }

    #[tokio::test]
    async fn broadcast_round_trip() {
        let broadcaster = Arc::new(SseBroadcaster::new());
        let mut rx = broadcaster.subscribe_channel("test-user").await;

        let msg = BroadcastMessage::named("emailsUpdated", "");
        broadcaster
            .broadcast_to_channel("test-user", msg)
            .await
            .ok();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.event_type.as_deref(), Some("emailsUpdated"));
        assert_eq!(received.data, "");
    }

    #[tokio::test]
    async fn shutdown_signal_fires_on_drop() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        drop(tx);
        // Receiver should resolve with Err when sender is dropped
        assert!(rx.await.is_err());
    }
}
