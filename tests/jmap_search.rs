#![allow(clippy::unwrap_used)]

//! Integration tests for JMAP email search.
//!
//! These tests require a live JMAP server and credentials in `.env`:
//!   MISSIVE_LOGIN=user@example.com
//!   MISSIVE_PASSWORD=secret
//!
//! Run with: cargo nextest run --test jmap_search

use jmap_client::{
    client::Client,
    core::query::Filter,
    email::{self, query::Comparator},
};

/// Load credentials from `.env` and connect to the JMAP server.
async fn connect() -> Client {
    dotenvy::dotenv().ok();
    let jmap_url =
        std::env::var("JMAP_URL").unwrap_or_else(|_| "https://mail.govcraft.ai".to_string());
    let login = std::env::var("MISSIVE_LOGIN").expect("MISSIVE_LOGIN must be set");
    let password = std::env::var("MISSIVE_PASSWORD").expect("MISSIVE_PASSWORD must be set");

    let host = url::Url::parse(&jmap_url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_else(|| jmap_url.clone());

    Client::new()
        .credentials((login, password))
        .follow_redirects([host.as_str()])
        .connect(&jmap_url)
        .await
        .expect("Failed to connect to JMAP server")
}

/// Find the Inbox mailbox ID.
async fn inbox_id(client: &Client) -> String {
    let mut req = client.build();
    req.get_mailbox().properties([
        jmap_client::mailbox::Property::Id,
        jmap_client::mailbox::Property::Role,
    ]);
    let response = req.send_get_mailbox().await.unwrap();
    response
        .list()
        .iter()
        .find(|m| m.role() == jmap_client::mailbox::Role::Inbox)
        .map(|m| m.id().unwrap().to_string())
        .expect("No Inbox found")
}

/// Helper: run an Email/query with the given filter and return the total count.
async fn query_count(client: &Client, filter: Filter<email::query::Filter>) -> usize {
    let mut req = client.build();
    let query = req.query_email();
    query.filter(filter);
    query.sort([Comparator::received_at().descending()]);
    query.calculate_total(true);
    query.limit(0);

    let response = req
        .send_single::<jmap_client::core::query::QueryResponse>()
        .await
        .unwrap();

    response.total().unwrap_or(0)
}

fn mailbox_text(mailbox_id: &str, text: &str) -> Filter<email::query::Filter> {
    Filter::and([
        email::query::Filter::in_mailbox(mailbox_id),
        email::query::Filter::text(text),
    ])
}

/// Verify that full-word JMAP text search works correctly.
#[tokio::test]
async fn text_filter_full_word_match() {
    let client = connect().await;
    let inbox = inbox_id(&client).await;

    let coinbase = query_count(&client, mailbox_text(&inbox, "coinbase")).await;
    let coingecko = query_count(&client, mailbox_text(&inbox, "coingecko")).await;

    assert!(coinbase > 0, "Expected Coinbase emails in inbox");
    assert!(coingecko > 0, "Expected CoinGecko emails in inbox");
}

/// Document that partial/prefix matching is NOT supported by the JMAP server's
/// full-text search. The tokenizer treats "coinbase" as a single token, so
/// searching for "coin" does not match it.
#[tokio::test]
async fn text_filter_no_prefix_matching() {
    let client = connect().await;
    let inbox = inbox_id(&client).await;

    let coinbase_full = query_count(&client, mailbox_text(&inbox, "coinbase")).await;
    let coin_partial = query_count(&client, mailbox_text(&inbox, "coin")).await;

    // The partial search "coin" finds fewer results than "coinbase" because
    // the FTS engine does not do prefix/substring matching.
    assert!(
        coin_partial < coinbase_full,
        "Expected 'coin' ({coin_partial}) to find fewer results than 'coinbase' ({coinbase_full}) \
         due to full-text tokenization"
    );
}

/// Verify that calculateTotal works and returns a count.
#[tokio::test]
async fn calculate_total_returns_count() {
    let client = connect().await;
    let inbox = inbox_id(&client).await;

    let mut req = client.build();
    let query = req.query_email();
    query.filter(email::query::Filter::in_mailbox(&inbox));
    query.calculate_total(true);
    query.limit(5);

    let response = req
        .send_single::<jmap_client::core::query::QueryResponse>()
        .await
        .unwrap();

    let total = response.total();
    assert!(total.is_some(), "Expected total to be returned");
    assert!(
        total.unwrap() >= response.ids().len(),
        "Total ({}) should be >= returned ids ({})",
        total.unwrap(),
        response.ids().len()
    );
}
