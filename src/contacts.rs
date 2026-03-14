use std::collections::HashMap;

use acton_service::prelude::{error, trace};
use jmap_client::client::Client;
use serde::{Deserialize, Serialize};

use crate::error::{JmapErrorKind, MissiveError};
use crate::jmap::SearchQuery;

// ---------------------------------------------------------------------------
// ID newtypes
// ---------------------------------------------------------------------------

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

define_id!(ContactId);

// ---------------------------------------------------------------------------
// JSContact wire types (serde — raw JMAP JSON shapes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsContactCard {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub card_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<ContactName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emails: Option<HashMap<String, ContactEmail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phones: Option<HashMap<String, ContactPhone>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addresses: Option<HashMap<String, ContactAddress>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizations: Option<HashMap<String, ContactOrganization>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titles: Option<HashMap<String, ContactTitle>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<HashMap<String, ContactNote>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_book_ids: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContactName {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<NameComponent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameComponent {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactEmail {
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactPhone {
    pub number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pref: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactAddress {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub street: Option<Vec<AddressComponent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postcode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contexts: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressComponent {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactOrganization {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<Vec<OrgUnit>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgUnit {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactTitle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactNote {
    pub note: String,
}

// ---------------------------------------------------------------------------
// View types (for templates)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ContactSummary {
    pub id: ContactId,
    pub full_name: String,
    pub primary_email: String,
}

impl ContactSummary {
    pub fn initials(&self) -> String {
        self.full_name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
    }

    pub fn sort_letter(&self) -> char {
        self.full_name
            .chars()
            .find(|c| c.is_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('#')
    }
}

#[derive(Debug, Clone)]
pub struct ContactDetail {
    pub id: ContactId,
    pub full_name: String,
    pub given_name: String,
    pub surname: String,
    pub emails: Vec<DisplayEmail>,
    pub phones: Vec<DisplayPhone>,
    pub addresses: Vec<DisplayAddress>,
    pub organization: String,
    pub job_title: String,
    pub notes: String,
}

impl ContactDetail {
    pub fn initials(&self) -> String {
        self.full_name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
    }
}

#[derive(Debug, Clone)]
pub struct DisplayEmail {
    pub address: String,
    pub label: String,
    pub is_preferred: bool,
}

#[derive(Debug, Clone)]
pub struct DisplayPhone {
    pub number: String,
    pub label: String,
    pub is_preferred: bool,
}

#[derive(Debug, Clone)]
pub struct DisplayAddress {
    pub formatted: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ContactGroup {
    pub letter: char,
    pub contacts: Vec<ContactSummary>,
}

// ---------------------------------------------------------------------------
// Form data (from HTML form submission)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContactFormData {
    #[serde(default)]
    pub given_name: String,
    #[serde(default)]
    pub surname: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub organization: String,
    #[serde(default)]
    pub job_title: String,
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub notes: String,
}

// ---------------------------------------------------------------------------
// Pure conversion functions
// ---------------------------------------------------------------------------

pub fn extract_full_name(card: &JsContactCard) -> String {
    let Some(ref name) = card.name else {
        return String::new();
    };
    // Prefer the explicit full name
    if let Some(ref full) = name.full {
        let trimmed = full.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    // Fall back to constructing from name components
    if let Some(ref components) = name.components {
        let given: Vec<&str> = components
            .iter()
            .filter(|c| c.kind.as_deref() == Some("given"))
            .map(|c| c.value.as_str())
            .collect();
        let surnames: Vec<&str> = components
            .iter()
            .filter(|c| c.kind.as_deref() == Some("surname"))
            .map(|c| c.value.as_str())
            .collect();
        let constructed = format!("{} {}", given.join(" "), surnames.join(" "));
        let trimmed = constructed.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    String::new()
}

fn extract_given_name(card: &JsContactCard) -> String {
    card.name
        .as_ref()
        .and_then(|n| n.components.as_ref())
        .and_then(|components| {
            components
                .iter()
                .find(|c| c.kind.as_deref() == Some("given"))
                .map(|c| c.value.clone())
        })
        .unwrap_or_default()
}

fn extract_surname(card: &JsContactCard) -> String {
    card.name
        .as_ref()
        .and_then(|n| n.components.as_ref())
        .and_then(|components| {
            components
                .iter()
                .find(|c| c.kind.as_deref() == Some("surname"))
                .map(|c| c.value.clone())
        })
        .unwrap_or_default()
}

pub fn context_label(contexts: &Option<HashMap<String, bool>>) -> String {
    contexts
        .as_ref()
        .and_then(|ctx| {
            ctx.iter()
                .find(|&(_, v)| *v)
                .map(|(k, _)| titlecase_first(k))
        })
        .unwrap_or_default()
}

fn titlecase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let upper: String = first.to_uppercase().collect();
            format!("{upper}{}", chars.as_str())
        }
    }
}

pub fn extract_primary_email(emails: &Option<HashMap<String, ContactEmail>>) -> String {
    let Some(map) = emails else {
        return String::new();
    };
    // Pick the one with lowest pref value (highest priority), or first
    map.values()
        .min_by_key(|e| e.pref.unwrap_or(u32::MAX))
        .map(|e| e.address.clone())
        .unwrap_or_default()
}

pub fn extract_organization(orgs: &Option<HashMap<String, ContactOrganization>>) -> String {
    let Some(map) = orgs else {
        return String::new();
    };
    map.values()
        .find_map(|o| o.name.clone())
        .unwrap_or_default()
}

fn extract_job_title(titles: &Option<HashMap<String, ContactTitle>>) -> String {
    let Some(map) = titles else {
        return String::new();
    };
    map.values()
        .find(|t| t.kind.as_deref() != Some("role"))
        .or_else(|| map.values().next())
        .map(|t| t.name.clone())
        .unwrap_or_default()
}

fn extract_notes(notes: &Option<HashMap<String, ContactNote>>) -> String {
    let Some(map) = notes else {
        return String::new();
    };
    map.values()
        .map(|n| n.note.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_address(addr: &ContactAddress) -> String {
    let street = addr
        .street
        .as_ref()
        .map(|parts| {
            parts
                .iter()
                .map(|p| p.value.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let parts: Vec<&str> = [
        street.as_str(),
        addr.locality.as_deref().unwrap_or_default(),
        addr.region.as_deref().unwrap_or_default(),
        addr.postcode.as_deref().unwrap_or_default(),
        addr.country.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();

    parts.join(", ")
}

fn build_display_emails(emails: &Option<HashMap<String, ContactEmail>>) -> Vec<DisplayEmail> {
    let Some(map) = emails else {
        return Vec::new();
    };
    let mut result: Vec<DisplayEmail> = map
        .values()
        .map(|e| DisplayEmail {
            address: e.address.clone(),
            label: context_label(&e.contexts),
            is_preferred: e.pref == Some(1),
        })
        .collect();
    result.sort_by(|a, b| b.is_preferred.cmp(&a.is_preferred));
    result
}

fn build_display_phones(phones: &Option<HashMap<String, ContactPhone>>) -> Vec<DisplayPhone> {
    let Some(map) = phones else {
        return Vec::new();
    };
    let mut result: Vec<DisplayPhone> = map
        .values()
        .map(|p| DisplayPhone {
            number: p.number.clone(),
            label: context_label(&p.contexts),
            is_preferred: p.pref == Some(1),
        })
        .collect();
    result.sort_by(|a, b| b.is_preferred.cmp(&a.is_preferred));
    result
}

fn build_display_addresses(
    addresses: &Option<HashMap<String, ContactAddress>>,
) -> Vec<DisplayAddress> {
    let Some(map) = addresses else {
        return Vec::new();
    };
    map.values()
        .map(|a| DisplayAddress {
            formatted: format_address(a),
            label: context_label(&a.contexts),
        })
        .filter(|d| !d.formatted.is_empty())
        .collect()
}

pub fn contact_card_to_summary(card: &JsContactCard) -> Option<ContactSummary> {
    let id = card.id.as_ref()?;
    let full_name = extract_full_name(card);
    Some(ContactSummary {
        id: ContactId::from(id.as_str()),
        full_name,
        primary_email: extract_primary_email(&card.emails),
    })
}

pub fn contact_card_to_detail(card: &JsContactCard) -> Option<ContactDetail> {
    let id = card.id.as_ref()?;
    let full_name = extract_full_name(card);
    Some(ContactDetail {
        id: ContactId::from(id.as_str()),
        full_name,
        given_name: extract_given_name(card),
        surname: extract_surname(card),
        emails: build_display_emails(&card.emails),
        phones: build_display_phones(&card.phones),
        addresses: build_display_addresses(&card.addresses),
        organization: extract_organization(&card.organizations),
        job_title: extract_job_title(&card.titles),
        notes: extract_notes(&card.notes),
    })
}

pub fn group_contacts_alphabetically(mut contacts: Vec<ContactSummary>) -> Vec<ContactGroup> {
    contacts.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));

    let mut groups: Vec<ContactGroup> = Vec::new();
    for contact in contacts {
        let letter = contact.sort_letter();
        if let Some(last) = groups.last_mut()
            && last.letter == letter
        {
            last.contacts.push(contact);
            continue;
        }
        groups.push(ContactGroup {
            letter,
            contacts: vec![contact],
        });
    }
    groups
}

pub fn form_to_jscontact(form: &ContactFormData) -> JsContactCard {
    let mut name_components = Vec::new();
    if !form.given_name.trim().is_empty() {
        name_components.push(NameComponent {
            value: form.given_name.trim().to_string(),
            kind: Some("given".to_string()),
        });
    }
    if !form.surname.trim().is_empty() {
        name_components.push(NameComponent {
            value: form.surname.trim().to_string(),
            kind: Some("surname".to_string()),
        });
    }

    let full_name = format!("{} {}", form.given_name.trim(), form.surname.trim());
    let full_name = full_name.trim().to_string();

    let emails = if form.email.trim().is_empty() {
        None
    } else {
        let mut map = HashMap::new();
        map.insert(
            "e1".to_string(),
            ContactEmail {
                address: form.email.trim().to_string(),
                contexts: Some(HashMap::from([("work".to_string(), true)])),
                pref: Some(1),
                label: None,
            },
        );
        Some(map)
    };

    let phones = if form.phone.trim().is_empty() {
        None
    } else {
        let mut map = HashMap::new();
        map.insert(
            "p1".to_string(),
            ContactPhone {
                number: form.phone.trim().to_string(),
                features: None,
                contexts: Some(HashMap::from([("work".to_string(), true)])),
                pref: Some(1),
                label: None,
            },
        );
        Some(map)
    };

    let addresses = if form.street.trim().is_empty()
        && form.city.trim().is_empty()
        && form.state.trim().is_empty()
        && form.postal_code.trim().is_empty()
        && form.country.trim().is_empty()
    {
        None
    } else {
        let street = if form.street.trim().is_empty() {
            None
        } else {
            Some(vec![AddressComponent {
                value: form.street.trim().to_string(),
                kind: None,
            }])
        };
        let mut map = HashMap::new();
        map.insert(
            "a1".to_string(),
            ContactAddress {
                street,
                locality: non_empty_opt(&form.city),
                region: non_empty_opt(&form.state),
                postcode: non_empty_opt(&form.postal_code),
                country: non_empty_opt(&form.country),
                country_code: None,
                contexts: Some(HashMap::from([("work".to_string(), true)])),
            },
        );
        Some(map)
    };

    let organizations = if form.organization.trim().is_empty() {
        None
    } else {
        let mut map = HashMap::new();
        map.insert(
            "o1".to_string(),
            ContactOrganization {
                name: Some(form.organization.trim().to_string()),
                units: None,
            },
        );
        Some(map)
    };

    let titles = if form.job_title.trim().is_empty() {
        None
    } else {
        let mut map = HashMap::new();
        map.insert(
            "t1".to_string(),
            ContactTitle {
                name: form.job_title.trim().to_string(),
                kind: Some("title".to_string()),
            },
        );
        Some(map)
    };

    let notes = if form.notes.trim().is_empty() {
        None
    } else {
        let mut map = HashMap::new();
        map.insert(
            "n1".to_string(),
            ContactNote {
                note: form.notes.trim().to_string(),
            },
        );
        Some(map)
    };

    JsContactCard {
        card_type: Some("Card".to_string()),
        version: Some("1.0".to_string()),
        name: if name_components.is_empty() && full_name.is_empty() {
            None
        } else {
            Some(ContactName {
                full: if full_name.is_empty() {
                    None
                } else {
                    Some(full_name)
                },
                components: if name_components.is_empty() {
                    None
                } else {
                    Some(name_components)
                },
            })
        },
        emails,
        phones,
        addresses,
        organizations,
        titles,
        notes,
        ..Default::default()
    }
}

fn non_empty_opt(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ---------------------------------------------------------------------------
// Raw JMAP request infrastructure
// ---------------------------------------------------------------------------

fn build_jmap_request(
    account_id: &str,
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
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:contacts"
        ],
        "methodCalls": calls
    })
}

async fn send_raw_jmap_request(
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
        MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "serialize request".to_string(),
            message: e.to_string(),
        })
    })?;

    trace!("JMAP contacts request to {api_url}");

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
        error!("JMAP contacts request failed: HTTP {status}: {body}");
        return Err(MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "HTTP request".to_string(),
            message: format!("HTTP {status}"),
        }));
    }

    let bytes = response.bytes().await.map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "response read".to_string(),
            message: e.to_string(),
        })
    })?;

    serde_json::from_slice(&bytes).map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "response parse".to_string(),
            message: e.to_string(),
        })
    })
}

fn extract_method_response<'a>(
    response: &'a serde_json::Value,
    call_id: &str,
) -> Result<&'a serde_json::Value, MissiveError> {
    let calls = response["methodResponses"]
        .as_array()
        .ok_or_else(|| MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: call_id.to_string(),
            message: "no methodResponses array".to_string(),
        }))?;

    for call in calls {
        let arr = call.as_array();
        if let Some(arr) = arr
            && arr.len() >= 3
            && arr[2].as_str() == Some(call_id)
        {
            // Check for error response
            if arr[0].as_str() == Some("error") {
                let err_type = arr[1]["type"].as_str().unwrap_or("unknown");
                let err_desc = arr[1]["description"].as_str().unwrap_or("");
                return Err(MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
                    operation: call_id.to_string(),
                    message: format!("{err_type}: {err_desc}"),
                }));
            }
            return Ok(&arr[1]);
        }
    }

    Err(MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
        operation: call_id.to_string(),
        message: "call ID not found in response".to_string(),
    }))
}

// ---------------------------------------------------------------------------
// JMAP contact operations
// ---------------------------------------------------------------------------

pub async fn fetch_contacts(
    client: &Client,
    position: usize,
    limit: usize,
    search: Option<&SearchQuery>,
) -> Result<(Vec<ContactSummary>, Option<usize>), MissiveError> {
    let account_id = client.default_account_id();

    // Build filter
    let filter = if let Some(q) = search {
        serde_json::json!({ "name": q.as_ref() })
    } else {
        serde_json::json!({})
    };

    // Step 1: ContactCard/query to get IDs
    let query_request = build_jmap_request(
        account_id,
        vec![(
            "ContactCard/query",
            serde_json::json!({
                "filter": filter,
                "position": position,
                "limit": limit,
                "calculateTotal": true
            }),
            "q0",
        )],
    );

    let query_response = send_raw_jmap_request(client, &query_request).await?;
    let query_data = extract_method_response(&query_response, "q0")?;

    let ids: Vec<String> = query_data["ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let total = query_data["total"].as_u64().map(|t| t as usize);

    if ids.is_empty() {
        return Ok((Vec::new(), total));
    }

    // Step 2: ContactCard/get to fetch details
    let get_request = build_jmap_request(
        account_id,
        vec![(
            "ContactCard/get",
            serde_json::json!({
                "ids": ids,
                "properties": ["id", "name", "fullName", "emails", "phones", "organizations"]
            }),
            "g0",
        )],
    );

    let get_response = send_raw_jmap_request(client, &get_request).await?;
    let get_data = extract_method_response(&get_response, "g0")?;

    let list = get_data["list"].as_array().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "ContactCard/get".to_string(),
            message: "no list in response".to_string(),
        })
    })?;

    let contacts: Vec<ContactSummary> = list
        .iter()
        .filter_map(|item| {
            let card: JsContactCard = serde_json::from_value(item.clone()).ok()?;
            contact_card_to_summary(&card)
        })
        .collect();

    trace!(
        "fetch_contacts: returning {} contacts (total: {total:?})",
        contacts.len()
    );
    Ok((contacts, total))
}

pub async fn fetch_contact_detail(
    client: &Client,
    contact_id: &ContactId,
) -> Result<ContactDetail, MissiveError> {
    let account_id = client.default_account_id();
    let request = build_jmap_request(
        account_id,
        vec![(
            "ContactCard/get",
            serde_json::json!({
                "ids": [contact_id.as_str()],
                "properties": [
                    "id", "name", "fullName", "emails", "phones",
                    "addresses", "organizations", "titles", "notes",
                    "addressBookIds"
                ]
            }),
            "g0",
        )],
    );

    let response = send_raw_jmap_request(client, &request).await?;
    let data = extract_method_response(&response, "g0")?;

    let list = data["list"].as_array().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::NotFound {
            resource: "ContactCard".to_string(),
            id: contact_id.to_string(),
        })
    })?;

    let item = list.first().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::NotFound {
            resource: "ContactCard".to_string(),
            id: contact_id.to_string(),
        })
    })?;

    let card: JsContactCard =
        serde_json::from_value(item.clone()).map_err(|e| {
            MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
                operation: "ContactCard/get".to_string(),
                message: format!("deserialization failed: {e}"),
            })
        })?;

    contact_card_to_detail(&card).ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::NotFound {
            resource: "ContactCard".to_string(),
            id: contact_id.to_string(),
        })
    })
}

pub async fn create_contact(
    client: &Client,
    form: &ContactFormData,
) -> Result<ContactId, MissiveError> {
    let account_id = client.default_account_id();
    let card = form_to_jscontact(form);

    let request = build_jmap_request(
        account_id,
        vec![(
            "ContactCard/set",
            serde_json::json!({
                "create": {
                    "new1": card
                }
            }),
            "s0",
        )],
    );

    let response = send_raw_jmap_request(client, &request).await?;
    let data = extract_method_response(&response, "s0")?;

    // Check for creation errors
    if let Some(not_created) = data["notCreated"].as_object()
        && let Some(err) = not_created.get("new1")
    {
        let err_type = err["type"].as_str().unwrap_or("unknown");
        let err_desc = err["description"].as_str().unwrap_or("");
        return Err(MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "ContactCard/set create".to_string(),
            message: format!("{err_type}: {err_desc}"),
        }));
    }

    let created_id = data["created"]["new1"]["id"]
        .as_str()
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
                operation: "ContactCard/set create".to_string(),
                message: "no id in created response".to_string(),
            })
        })?;

    trace!("create_contact: created {created_id}");
    Ok(ContactId::from(created_id))
}

pub async fn update_contact(
    client: &Client,
    contact_id: &ContactId,
    form: &ContactFormData,
) -> Result<(), MissiveError> {
    let account_id = client.default_account_id();
    let card = form_to_jscontact(form);

    let request = build_jmap_request(
        account_id,
        vec![(
            "ContactCard/set",
            serde_json::json!({
                "update": {
                    contact_id.as_str(): card
                }
            }),
            "s0",
        )],
    );

    let response = send_raw_jmap_request(client, &request).await?;
    let data = extract_method_response(&response, "s0")?;

    // Check for update errors
    if let Some(not_updated) = data["notUpdated"].as_object()
        && let Some(err) = not_updated.get(contact_id.as_str())
    {
        let err_type = err["type"].as_str().unwrap_or("unknown");
        let err_desc = err["description"].as_str().unwrap_or("");
        return Err(MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "ContactCard/set update".to_string(),
            message: format!("{err_type}: {err_desc}"),
        }));
    }

    trace!("update_contact: updated {contact_id}");
    Ok(())
}

pub async fn delete_contact(
    client: &Client,
    contact_id: &ContactId,
) -> Result<(), MissiveError> {
    let account_id = client.default_account_id();

    let request = build_jmap_request(
        account_id,
        vec![(
            "ContactCard/set",
            serde_json::json!({
                "destroy": [contact_id.as_str()]
            }),
            "s0",
        )],
    );

    let response = send_raw_jmap_request(client, &request).await?;
    let data = extract_method_response(&response, "s0")?;

    // Check for destroy errors
    if let Some(not_destroyed) = data["notDestroyed"].as_object()
        && let Some(err) = not_destroyed.get(contact_id.as_str())
    {
        let err_type = err["type"].as_str().unwrap_or("unknown");
        let err_desc = err["description"].as_str().unwrap_or("");
        return Err(MissiveError::Jmap(JmapErrorKind::ContactOperationFailed {
            operation: "ContactCard/set destroy".to_string(),
            message: format!("{err_type}: {err_desc}"),
        }));
    }

    trace!("delete_contact: deleted {contact_id}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn make_card(id: &str, full_name: &str, email: &str) -> JsContactCard {
        let mut emails = HashMap::new();
        if !email.is_empty() {
            emails.insert(
                "e1".to_string(),
                ContactEmail {
                    address: email.to_string(),
                    contexts: Some(HashMap::from([("work".to_string(), true)])),
                    pref: Some(1),
                    label: None,
                },
            );
        }

        JsContactCard {
            id: Some(id.to_string()),
            name: Some(ContactName {
                full: Some(full_name.to_string()),
                components: None,
            }),
            emails: if emails.is_empty() {
                None
            } else {
                Some(emails)
            },
            ..Default::default()
        }
    }

    fn make_card_with_name_components(
        id: &str,
        given: &str,
        surname: &str,
    ) -> JsContactCard {
        JsContactCard {
            id: Some(id.to_string()),
            name: Some(ContactName {
                full: None,
                components: Some(vec![
                    NameComponent {
                        value: given.to_string(),
                        kind: Some("given".to_string()),
                    },
                    NameComponent {
                        value: surname.to_string(),
                        kind: Some("surname".to_string()),
                    },
                ]),
            }),
            ..Default::default()
        }
    }

    // --- extract_full_name ---

    #[test]
    fn full_name_from_full_name_field() {
        let card = make_card("1", "Alice Smith", "");
        assert_eq!(extract_full_name(&card), "Alice Smith");
    }

    #[test]
    fn full_name_from_name_components() {
        let card = make_card_with_name_components("1", "Bob", "Jones");
        assert_eq!(extract_full_name(&card), "Bob Jones");
    }

    #[test]
    fn full_name_empty_card() {
        let card = JsContactCard::default();
        assert_eq!(extract_full_name(&card), "");
    }

    #[test]
    fn full_name_prefers_full_name_field() {
        let mut card = make_card_with_name_components("1", "Bob", "Jones");
        card.name.as_mut().unwrap().full = Some("Robert Jones III".to_string());
        assert_eq!(extract_full_name(&card), "Robert Jones III");
    }

    // --- extract_primary_email ---

    #[test]
    fn primary_email_picks_preferred() {
        let mut map = HashMap::new();
        map.insert(
            "e1".to_string(),
            ContactEmail {
                address: "work@example.com".to_string(),
                contexts: None,
                pref: Some(2),
                label: None,
            },
        );
        map.insert(
            "e2".to_string(),
            ContactEmail {
                address: "pref@example.com".to_string(),
                contexts: None,
                pref: Some(1),
                label: None,
            },
        );
        assert_eq!(extract_primary_email(&Some(map)), "pref@example.com");
    }

    #[test]
    fn primary_email_none() {
        assert_eq!(extract_primary_email(&None), "");
    }

    // --- extract_organization ---

    #[test]
    fn organization_from_map() {
        let mut map = HashMap::new();
        map.insert(
            "o1".to_string(),
            ContactOrganization {
                name: Some("Acme Inc".to_string()),
                units: None,
            },
        );
        assert_eq!(extract_organization(&Some(map)), "Acme Inc");
    }

    #[test]
    fn organization_none() {
        assert_eq!(extract_organization(&None), "");
    }

    // --- context_label ---

    #[test]
    fn context_label_work() {
        let ctx = Some(HashMap::from([("work".to_string(), true)]));
        assert_eq!(context_label(&ctx), "Work");
    }

    #[test]
    fn context_label_personal() {
        let ctx = Some(HashMap::from([("private".to_string(), true)]));
        assert_eq!(context_label(&ctx), "Private");
    }

    #[test]
    fn context_label_none() {
        assert_eq!(context_label(&None), "");
    }

    // --- format_address ---

    #[test]
    fn format_address_full() {
        let addr = ContactAddress {
            street: Some(vec![AddressComponent {
                value: "123 Main St".to_string(),
                kind: None,
            }]),
            locality: Some("Springfield".to_string()),
            region: Some("IL".to_string()),
            postcode: Some("62701".to_string()),
            country: Some("USA".to_string()),
            country_code: None,
            contexts: None,
        };
        assert_eq!(
            format_address(&addr),
            "123 Main St, Springfield, IL, 62701, USA"
        );
    }

    #[test]
    fn format_address_partial() {
        let addr = ContactAddress {
            street: None,
            locality: Some("Denver".to_string()),
            region: Some("CO".to_string()),
            postcode: None,
            country: None,
            country_code: None,
            contexts: None,
        };
        assert_eq!(format_address(&addr), "Denver, CO");
    }

    // --- contact_card_to_summary ---

    #[test]
    fn summary_from_card() {
        let card = make_card("abc", "Jane Doe", "jane@example.com");
        let summary = contact_card_to_summary(&card).unwrap();
        assert_eq!(summary.id.as_str(), "abc");
        assert_eq!(summary.full_name, "Jane Doe");
        assert_eq!(summary.primary_email, "jane@example.com");
    }

    #[test]
    fn summary_requires_id() {
        let card = JsContactCard {
            name: Some(ContactName {
                full: Some("No ID".to_string()),
                components: None,
            }),
            ..Default::default()
        };
        assert!(contact_card_to_summary(&card).is_none());
    }

    // --- contact_card_to_detail ---

    #[test]
    fn detail_from_card_with_components() {
        let mut card = make_card_with_name_components("d1", "Alice", "Wonderland");
        card.name.as_mut().unwrap().full = Some("Alice Wonderland".to_string());
        let detail = contact_card_to_detail(&card).unwrap();
        assert_eq!(detail.given_name, "Alice");
        assert_eq!(detail.surname, "Wonderland");
        assert_eq!(detail.full_name, "Alice Wonderland");
    }

    // --- ContactSummary methods ---

    #[test]
    fn initials_two_words() {
        let s = ContactSummary {
            id: ContactId::from("1"),
            full_name: "John Doe".to_string(),
            primary_email: String::new(),
        };
        assert_eq!(s.initials(), "JD");
    }

    #[test]
    fn initials_single_word() {
        let s = ContactSummary {
            id: ContactId::from("1"),
            full_name: "Madonna".to_string(),
            primary_email: String::new(),
        };
        assert_eq!(s.initials(), "M");
    }

    #[test]
    fn sort_letter_alpha() {
        let s = ContactSummary {
            id: ContactId::from("1"),
            full_name: "alice".to_string(),
            primary_email: String::new(),
        };
        assert_eq!(s.sort_letter(), 'A');
    }

    #[test]
    fn sort_letter_non_alpha() {
        let s = ContactSummary {
            id: ContactId::from("1"),
            full_name: "123 Company".to_string(),
            primary_email: String::new(),
        };
        assert_eq!(s.sort_letter(), 'C');
    }

    // --- group_contacts_alphabetically ---

    #[test]
    fn group_contacts_empty() {
        let groups = group_contacts_alphabetically(vec![]);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_contacts_multiple_letters() {
        let contacts = vec![
            ContactSummary {
                id: ContactId::from("1"),
                full_name: "Bob".to_string(),
                primary_email: String::new(),
            },
            ContactSummary {
                id: ContactId::from("2"),
                full_name: "Alice".to_string(),
                primary_email: String::new(),
            },
            ContactSummary {
                id: ContactId::from("3"),
                full_name: "Amy".to_string(),
                primary_email: String::new(),
            },
        ];
        let groups = group_contacts_alphabetically(contacts);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].letter, 'A');
        assert_eq!(groups[0].contacts.len(), 2);
        assert_eq!(groups[1].letter, 'B');
        assert_eq!(groups[1].contacts.len(), 1);
    }

    // --- form_to_jscontact ---

    #[test]
    fn form_to_card_basic() {
        let form = ContactFormData {
            given_name: "Jane".to_string(),
            surname: "Doe".to_string(),
            email: "jane@example.com".to_string(),
            ..Default::default()
        };
        let card = form_to_jscontact(&form);
        assert_eq!(card.name.as_ref().unwrap().full.as_deref(), Some("Jane Doe"));
        let emails = card.emails.as_ref().unwrap();
        assert_eq!(emails["e1"].address, "jane@example.com");
    }

    #[test]
    fn form_to_card_empty_fields_are_none() {
        let form = ContactFormData::default();
        let card = form_to_jscontact(&form);
        assert!(card.emails.is_none());
        assert!(card.phones.is_none());
        assert!(card.addresses.is_none());
        assert!(card.organizations.is_none());
        assert!(card.titles.is_none());
        assert!(card.notes.is_none());
    }

    #[test]
    fn form_to_card_with_address() {
        let form = ContactFormData {
            street: "123 Main St".to_string(),
            city: "Denver".to_string(),
            state: "CO".to_string(),
            postal_code: "80202".to_string(),
            ..Default::default()
        };
        let card = form_to_jscontact(&form);
        let addrs = card.addresses.as_ref().unwrap();
        let addr = &addrs["a1"];
        assert_eq!(addr.locality.as_deref(), Some("Denver"));
        assert_eq!(addr.region.as_deref(), Some("CO"));
    }

    // --- titlecase_first ---

    #[test]
    fn titlecase_empty() {
        assert_eq!(titlecase_first(""), "");
    }

    #[test]
    fn titlecase_lowercase() {
        assert_eq!(titlecase_first("work"), "Work");
    }

    #[test]
    fn titlecase_already_upper() {
        assert_eq!(titlecase_first("Work"), "Work");
    }

    // --- Live JMAP integration tests ---

    async fn live_connect() -> jmap_client::client::Client {
        dotenvy::dotenv().ok();
        let username = std::env::var("EMAIL_USERNAME").expect("EMAIL_USERNAME");
        let password = std::env::var("EMAIL_PASSWORD").expect("EMAIL_PASSWORD");
        jmap_client::client::Client::new()
            .credentials((username.as_str(), password.as_str()))
            .follow_redirects(["mail.govcraft.ai"])
            .connect("https://mail.govcraft.ai")
            .await
            .expect("Failed to connect to JMAP server")
    }

    #[tokio::test]
    #[ignore = "requires live JMAP server"]
    async fn live_fetch_contacts() {
        let client = live_connect().await;
        let (contacts, total) = fetch_contacts(&client, 0, 50, None)
            .await
            .expect("fetch_contacts failed");
        println!("Got {} contacts, total: {total:?}", contacts.len());
        for c in &contacts {
            println!("  - {} <{}> [{}]", c.full_name, c.primary_email, c.initials());
        }
        assert!(!contacts.is_empty(), "Expected at least one contact");
    }

    #[tokio::test]
    #[ignore = "requires live JMAP server"]
    async fn live_fetch_contact_detail() {
        let client = live_connect().await;
        let (contacts, _) = fetch_contacts(&client, 0, 1, None)
            .await
            .expect("fetch_contacts failed");
        assert!(!contacts.is_empty(), "Need at least one contact");

        let detail = fetch_contact_detail(&client, &contacts[0].id)
            .await
            .expect("fetch_contact_detail failed");
        println!(
            "Detail: {} (given={}, surname={}, emails={})",
            detail.full_name,
            detail.given_name,
            detail.surname,
            detail.emails.len()
        );
        assert!(!detail.full_name.is_empty());
    }
}
