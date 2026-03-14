use acton_service::prelude::{error, trace};
use jmap_client::client::Client;

use crate::error::{JmapErrorKind, MissiveError};

/// Build a JMAP request envelope with the given namespace URIs and method calls.
///
/// Each method call is a tuple of (method_name, args_json, call_id).
/// The `accountId` is automatically injected into each method call's arguments.
pub fn build_jmap_request(
    account_id: &str,
    using: &[&str],
    method_calls: Vec<(&str, serde_json::Value, &str)>,
) -> serde_json::Value {
    let calls: Vec<serde_json::Value> = method_calls
        .into_iter()
        .map(|(method, mut args, call_id)| {
            args.as_object_mut()
                .map(|obj| obj.insert("accountId".to_string(), serde_json::json!(account_id)));
            serde_json::json!([method, args, call_id])
        })
        .collect();

    serde_json::json!({
        "using": using,
        "methodCalls": calls
    })
}

/// Send a raw JMAP request to the server, reusing auth from a `jmap_client::Client`.
pub async fn send_raw_jmap_request(
    client: &Client,
    request_body: &serde_json::Value,
) -> Result<serde_json::Value, MissiveError> {
    let session = client.session();
    let api_url = session.api_url();
    let headers = client.headers().clone();

    let http = reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(true)
        .http1_only()
        .build()
        .map_err(|e| {
            MissiveError::Jmap(JmapErrorKind::ConnectionFailed {
                url: api_url.to_string(),
                message: e.to_string(),
            })
        })?;

    let body = serde_json::to_vec(request_body).map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::Unknown {
            message: format!("serialize request: {e}"),
        })
    })?;

    trace!("JMAP raw request to {api_url}");

    let response = http
        .post(api_url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| {
            MissiveError::Jmap(JmapErrorKind::ConnectionFailed {
                url: api_url.to_string(),
                message: e.to_string(),
            })
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("JMAP raw request failed: HTTP {status}: {body}");
        return Err(MissiveError::Jmap(JmapErrorKind::Unknown {
            message: format!("JMAP HTTP {status}"),
        }));
    }

    let bytes = response.bytes().await.map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::Unknown {
            message: format!("response read: {e}"),
        })
    })?;

    serde_json::from_slice(&bytes).map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::Unknown {
            message: format!("response parse: {e}"),
        })
    })
}

/// Extract a specific method response from the JMAP response by call ID.
///
/// Returns the response data (2nd element of the method response tuple).
/// Returns an error if the call ID is not found or the server returned an error.
pub fn extract_method_response<'a>(
    response: &'a serde_json::Value,
    call_id: &str,
) -> Result<&'a serde_json::Value, MissiveError> {
    let calls = response["methodResponses"]
        .as_array()
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::QueryFailed {
                method: call_id.to_string(),
                message: "no methodResponses array".to_string(),
            })
        })?;

    for call in calls {
        let arr = call.as_array();
        if let Some(arr) = arr
            && arr.len() >= 3
            && arr[2].as_str() == Some(call_id)
        {
            if arr[0].as_str() == Some("error") {
                let err_type = arr[1]["type"].as_str().unwrap_or("unknown");
                let err_desc = arr[1]["description"].as_str().unwrap_or("");
                return Err(MissiveError::Jmap(JmapErrorKind::QueryFailed {
                    method: call_id.to_string(),
                    message: format!("{err_type}: {err_desc}"),
                }));
            }
            return Ok(&arr[1]);
        }
    }

    Err(MissiveError::Jmap(JmapErrorKind::QueryFailed {
        method: call_id.to_string(),
        message: "call ID not found in response".to_string(),
    }))
}

/// JMAP namespace URIs for contacts operations.
pub const USING_CONTACTS: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:contacts",
];

/// JMAP namespace URIs for calendar operations.
pub const USING_CALENDARS: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:calendars",
];
