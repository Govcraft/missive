#![allow(clippy::unwrap_used)]

//! Debug test to inspect what the JMAP server actually matches.

use jmap_client::{
    client::Client,
    core::query::Filter,
    email::{self, query::Comparator},
};

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

/// Fetch email summaries matching a text filter and print their details.
async fn search_and_print(client: &Client, mailbox_id: &str, term: &str) {
    let mut req = client.build();
    let query = req.query_email();
    query.filter(Filter::and([
        email::query::Filter::in_mailbox(mailbox_id),
        email::query::Filter::text(term),
    ]));
    query.sort([Comparator::received_at().descending()]);
    query.limit(10);

    let query_response = req
        .send_single::<jmap_client::core::query::QueryResponse>()
        .await
        .unwrap();

    let ids: Vec<&str> = query_response.ids().iter().map(|s| s.as_str()).collect();
    if ids.is_empty() {
        println!("  text('{term}') => 0 results");
        return;
    }

    println!("  text('{term}') => {} results:", ids.len());

    let mut req = client.build();
    let get = req.get_email().ids(ids.iter().copied());
    get.properties([
        email::Property::Id,
        email::Property::From,
        email::Property::Subject,
        email::Property::Preview,
    ]);

    let response = req.send_get_email().await.unwrap();
    for email in response.list() {
        let from = email
            .from()
            .map(|addrs| {
                addrs
                    .iter()
                    .map(|a| format!("{} <{}>", a.name().unwrap_or(""), a.email()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let subject = email.subject().unwrap_or("(no subject)");
        let preview = email.preview().unwrap_or("");
        println!("    From: {from}");
        println!("    Subject: {subject}");
        println!("    Preview: {}", &preview[..preview.len().min(200)]);
        println!();
    }
}

#[tokio::test]
#[ignore = "requires MISSIVE_LOGIN and MISSIVE_PASSWORD env vars"]
async fn debug_search_results() {
    let client = connect().await;
    let inbox = inbox_id(&client).await;

    println!("\n=== Search: 'coin' ===");
    search_and_print(&client, &inbox, "coin").await;

    println!("=== Search: 'coinbase' ===");
    search_and_print(&client, &inbox, "coinbase").await;

    println!("=== Search: 'coingecko' ===");
    search_and_print(&client, &inbox, "coingecko").await;
}
