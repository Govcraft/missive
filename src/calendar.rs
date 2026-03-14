use std::collections::{HashMap, HashSet};

use acton_service::prelude::trace;
use chrono::{Datelike, Local, NaiveDate};
use jmap_client::client::Client;
use serde::{Deserialize, Serialize};

use crate::error::{JmapErrorKind, MissiveError};
use crate::jmap_raw::{self, USING_CALENDARS};

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

define_id!(CalendarEventId);

// ---------------------------------------------------------------------------
// JSCalendar wire types (serde — raw JMAP JSON shapes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsCalendarEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_without_time: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_ids: Option<HashMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<HashMap<String, EventLocation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<HashMap<String, EventParticipant>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_busy_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub location_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventParticipant {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_address: Option<String>,
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub participant_type: Option<String>,
}

// ---------------------------------------------------------------------------
// View types (for templates)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EventSummary {
    pub id: CalendarEventId,
    pub title: String,
    pub time_display: String,
    pub is_all_day: bool,
    pub location: String,
    pub start_date: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
pub struct EventDetail {
    pub id: CalendarEventId,
    pub title: String,
    pub description: String,
    pub date_display: String,
    pub time_display: String,
    pub duration_display: String,
    pub is_all_day: bool,
    pub location: String,
    pub participants: Vec<ParticipantInfo>,
    pub free_busy_status: String,
    // For pre-filling the edit form
    pub start_date: String,
    pub start_time: String,
    pub duration_hours: String,
    pub duration_minutes: String,
}

#[derive(Debug, Clone)]
pub struct ParticipantInfo {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct CalendarDay {
    pub date: NaiveDate,
    pub day: u32,
    pub is_current_month: bool,
    pub is_today: bool,
    pub is_selected: bool,
    pub has_events: bool,
}

#[derive(Debug, Clone)]
pub struct CalendarMonth {
    pub month_name: String,
    pub days: Vec<CalendarDay>,
    pub prev_year: i32,
    pub prev_month: u32,
    pub next_year: i32,
    pub next_month: u32,
}

pub const DAY_NAMES: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

// ---------------------------------------------------------------------------
// Form data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventFormData {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub duration_hours: String,
    #[serde(default)]
    pub duration_minutes: String,
    #[serde(default)]
    pub is_all_day: String,
    #[serde(default)]
    pub location: String,
}

// ---------------------------------------------------------------------------
// Pure conversion functions
// ---------------------------------------------------------------------------

pub fn parse_duration_display(duration: &str) -> String {
    if duration.is_empty() {
        return String::new();
    }
    // Parse ISO 8601 duration: P1D, PT30M, PT1H, PT1H30M, etc.
    let s = duration.trim_start_matches('P');
    if s.contains('D') {
        let days = s.trim_end_matches('D').trim_end_matches('T');
        if let Ok(n) = days.parse::<u32>() {
            return if n == 1 {
                "All day".to_string()
            } else {
                format!("{n} days")
            };
        }
    }
    let t = s.trim_start_matches('T');
    let mut hours = 0u32;
    let mut minutes = 0u32;
    if let Some(h_pos) = t.find('H') {
        hours = t[..h_pos].parse().unwrap_or(0);
        let rest = &t[h_pos + 1..];
        if let Some(m_pos) = rest.find('M') {
            minutes = rest[..m_pos].parse().unwrap_or(0);
        }
    } else if let Some(m_pos) = t.find('M') {
        minutes = t[..m_pos].parse().unwrap_or(0);
    }

    match (hours, minutes) {
        (0, 0) => String::new(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

fn parse_duration_parts(duration: &str) -> (String, String) {
    let s = duration.trim_start_matches('P');
    if s.contains('D') {
        return ("24".to_string(), "0".to_string());
    }
    let t = s.trim_start_matches('T');
    let mut hours = 0u32;
    let mut minutes = 0u32;
    if let Some(h_pos) = t.find('H') {
        hours = t[..h_pos].parse().unwrap_or(0);
        let rest = &t[h_pos + 1..];
        if let Some(m_pos) = rest.find('M') {
            minutes = rest[..m_pos].parse().unwrap_or(0);
        }
    } else if let Some(m_pos) = t.find('M') {
        minutes = t[..m_pos].parse().unwrap_or(0);
    }
    (hours.to_string(), minutes.to_string())
}

pub fn parse_start_display(start: &str, show_without_time: bool) -> (String, String, Option<NaiveDate>) {
    // start is like "2025-06-19T00:00:00" or "2025-06-23T20:00:00"
    let date_part = start.get(..10).unwrap_or(start);
    let time_part = start.get(11..16).unwrap_or("");

    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok();
    let date_display = date
        .map(|d| d.format("%B %-d, %Y").to_string())
        .unwrap_or_else(|| date_part.to_string());

    let time_display = if show_without_time || time_part.is_empty() {
        String::new()
    } else {
        format_time_12h(time_part)
    };

    (date_display, time_display, date)
}

fn format_time_12h(time_24h: &str) -> String {
    let parts: Vec<&str> = time_24h.split(':').collect();
    if parts.len() < 2 {
        return time_24h.to_string();
    }
    let hour: u32 = parts[0].parse().unwrap_or(0);
    let minute: u32 = parts[1].parse().unwrap_or(0);
    let (display_hour, period) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    if minute == 0 {
        format!("{display_hour} {period}")
    } else {
        format!("{display_hour}:{minute:02} {period}")
    }
}

pub fn extract_location(locations: &Option<HashMap<String, EventLocation>>) -> String {
    let Some(map) = locations else {
        return String::new();
    };
    map.values()
        .find_map(|l| l.name.clone())
        .unwrap_or_default()
}

fn build_participants(
    participants: &Option<HashMap<String, EventParticipant>>,
) -> Vec<ParticipantInfo> {
    let Some(map) = participants else {
        return Vec::new();
    };
    map.values()
        .map(|p| {
            let email = p
                .calendar_address
                .as_deref()
                .unwrap_or("")
                .trim_start_matches("mailto:")
                .to_string();
            ParticipantInfo {
                name: p.name.clone().unwrap_or_default(),
                email,
            }
        })
        .collect()
}

pub fn event_card_to_summary(card: &JsCalendarEvent) -> Option<EventSummary> {
    let id = card.id.as_ref()?;
    let title = card.title.clone().unwrap_or_default();
    let is_all_day = card.show_without_time.unwrap_or(false)
        || card.duration.as_deref() == Some("P1D");
    let start = card.start.as_deref().unwrap_or("");
    let (_, time_display, start_date) = parse_start_display(start, is_all_day);

    Some(EventSummary {
        id: CalendarEventId::from(id.as_str()),
        title,
        time_display,
        is_all_day,
        location: extract_location(&card.locations),
        start_date,
    })
}

pub fn event_card_to_detail(card: &JsCalendarEvent) -> Option<EventDetail> {
    let id = card.id.as_ref()?;
    let title = card.title.clone().unwrap_or_default();
    let is_all_day = card.show_without_time.unwrap_or(false)
        || card.duration.as_deref() == Some("P1D");
    let start = card.start.as_deref().unwrap_or("");
    let (date_display, time_display, _) = parse_start_display(start, is_all_day);
    let duration_display = card
        .duration
        .as_deref()
        .map(parse_duration_display)
        .unwrap_or_default();
    let (duration_hours, duration_minutes) = card
        .duration
        .as_deref()
        .map(parse_duration_parts)
        .unwrap_or_default();

    let start_date_raw = start.get(..10).unwrap_or("").to_string();
    let start_time_raw = start.get(11..16).unwrap_or("").to_string();

    Some(EventDetail {
        id: CalendarEventId::from(id.as_str()),
        title,
        description: card.description.clone().unwrap_or_default(),
        date_display,
        time_display,
        duration_display,
        is_all_day,
        location: extract_location(&card.locations),
        participants: build_participants(&card.participants),
        free_busy_status: card.free_busy_status.clone().unwrap_or_default(),
        start_date: start_date_raw,
        start_time: start_time_raw,
        duration_hours,
        duration_minutes,
    })
}

pub fn build_calendar_month(
    year: i32,
    month: u32,
    selected_date: Option<NaiveDate>,
    event_dates: &HashSet<NaiveDate>,
) -> CalendarMonth {
    let today = Local::now().date_naive();
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today));

    let month_name = first_of_month.format("%B %Y").to_string();

    // Find the Monday on or before the first of the month
    let start_weekday = first_of_month.weekday();
    let days_from_monday = start_weekday.num_days_from_monday();
    let grid_start = first_of_month - chrono::Duration::days(days_from_monday as i64);

    // Build 6 weeks (42 days) to fill the grid
    let mut days = Vec::with_capacity(42);
    for i in 0..42 {
        let date = grid_start + chrono::Duration::days(i);
        days.push(CalendarDay {
            date,
            day: date.day(),
            is_current_month: date.month() == month && date.year() == year,
            is_today: date == today,
            is_selected: selected_date == Some(date),
            has_events: event_dates.contains(&date),
        });
    }

    // Previous/next month for navigation
    let (prev_year, prev_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    CalendarMonth {
        month_name,
        days,
        prev_year,
        prev_month,
        next_year,
        next_month,
    }
}

pub fn form_to_jscalendar(form: &EventFormData, calendar_id: &str) -> JsCalendarEvent {
    let is_all_day = form.is_all_day == "on";

    let start = if is_all_day {
        format!("{}T00:00:00", form.start_date.trim())
    } else {
        let time = if form.start_time.trim().is_empty() {
            "00:00"
        } else {
            form.start_time.trim()
        };
        format!("{}T{}:00", form.start_date.trim(), time)
    };

    let duration = if is_all_day {
        "P1D".to_string()
    } else {
        let hours: u32 = form.duration_hours.trim().parse().unwrap_or(0);
        let minutes: u32 = form.duration_minutes.trim().parse().unwrap_or(0);
        if hours == 0 && minutes == 0 {
            "PT1H".to_string()
        } else if minutes == 0 {
            format!("PT{hours}H")
        } else if hours == 0 {
            format!("PT{minutes}M")
        } else {
            format!("PT{hours}H{minutes}M")
        }
    };

    let locations = if form.location.trim().is_empty() {
        None
    } else {
        let mut map = HashMap::new();
        map.insert(
            "loc1".to_string(),
            EventLocation {
                name: Some(form.location.trim().to_string()),
                location_type: Some("Location".to_string()),
            },
        );
        Some(map)
    };

    JsCalendarEvent {
        title: Some(form.title.trim().to_string()),
        description: if form.description.trim().is_empty() {
            None
        } else {
            Some(form.description.trim().to_string())
        },
        start: Some(start),
        duration: Some(duration),
        show_without_time: if is_all_day { Some(true) } else { None },
        calendar_ids: Some(HashMap::from([(calendar_id.to_string(), true)])),
        locations,
        ..Default::default()
    }
}

/// Extract event dates from a list of summaries for calendar dot indicators.
pub fn event_dates_set(events: &[EventSummary]) -> HashSet<NaiveDate> {
    events.iter().filter_map(|e| e.start_date).collect()
}

// ---------------------------------------------------------------------------
// JMAP calendar operations
// ---------------------------------------------------------------------------

async fn fetch_default_calendar_id(client: &Client) -> Result<String, MissiveError> {
    let account_id = client.default_account_id();
    let request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "Calendar/get",
            serde_json::json!({
                "properties": ["id", "isDefault"]
            }),
            "c0",
        )],
    );

    let response = jmap_raw::send_raw_jmap_request(client, &request).await?;
    let data = jmap_raw::extract_method_response(&response, "c0")?;

    let list = data["list"].as_array().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "Calendar/get".to_string(),
            message: "no list in response".to_string(),
        })
    })?;

    list.iter()
        .find(|item| item["isDefault"].as_bool() == Some(true))
        .or_else(|| list.first())
        .and_then(|item| item["id"].as_str())
        .map(String::from)
        .ok_or_else(|| {
            MissiveError::Jmap(JmapErrorKind::QueryFailed {
                method: "Calendar/get".to_string(),
                message: "no calendars found".to_string(),
            })
        })
}

pub async fn fetch_events_for_range(
    client: &Client,
    after: &str,
    before: &str,
) -> Result<Vec<EventSummary>, MissiveError> {
    let account_id = client.default_account_id();

    // Step 1: CalendarEvent/query with date filter
    let query_request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "CalendarEvent/query",
            serde_json::json!({
                "filter": {
                    "after": format!("{after}T00:00:00Z"),
                    "before": format!("{before}T23:59:59Z")
                },
                "limit": 200
            }),
            "q0",
        )],
    );

    let query_response = jmap_raw::send_raw_jmap_request(client, &query_request).await?;
    let query_data = jmap_raw::extract_method_response(&query_response, "q0")?;

    let ids: Vec<String> = query_data["ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: CalendarEvent/get
    let get_request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "CalendarEvent/get",
            serde_json::json!({
                "ids": ids,
                "properties": ["id", "title", "start", "duration", "showWithoutTime", "locations", "timeZone"]
            }),
            "g0",
        )],
    );

    let get_response = jmap_raw::send_raw_jmap_request(client, &get_request).await?;
    let get_data = jmap_raw::extract_method_response(&get_response, "g0")?;

    let list = get_data["list"].as_array().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "CalendarEvent/get".to_string(),
            message: "no list in response".to_string(),
        })
    })?;

    let mut events: Vec<EventSummary> = list
        .iter()
        .filter_map(|item| {
            let card: JsCalendarEvent = serde_json::from_value(item.clone()).ok()?;
            event_card_to_summary(&card)
        })
        .collect();

    // Sort by start date
    events.sort_by(|a, b| a.start_date.cmp(&b.start_date));

    trace!("fetch_events_for_range: {} events", events.len());
    Ok(events)
}

pub async fn fetch_event_detail(
    client: &Client,
    event_id: &CalendarEventId,
) -> Result<EventDetail, MissiveError> {
    let account_id = client.default_account_id();
    let request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "CalendarEvent/get",
            serde_json::json!({
                "ids": [event_id.as_str()],
                "properties": [
                    "id", "title", "description", "start", "duration",
                    "timeZone", "showWithoutTime", "status", "calendarIds",
                    "locations", "participants", "freeBusyStatus",
                    "created", "updated"
                ]
            }),
            "g0",
        )],
    );

    let response = jmap_raw::send_raw_jmap_request(client, &request).await?;
    let data = jmap_raw::extract_method_response(&response, "g0")?;

    let list = data["list"].as_array().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::NotFound {
            resource: "CalendarEvent".to_string(),
            id: event_id.to_string(),
        })
    })?;

    let item = list.first().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::NotFound {
            resource: "CalendarEvent".to_string(),
            id: event_id.to_string(),
        })
    })?;

    let card: JsCalendarEvent = serde_json::from_value(item.clone()).map_err(|e| {
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "CalendarEvent/get".to_string(),
            message: format!("deserialization failed: {e}"),
        })
    })?;

    event_card_to_detail(&card).ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::NotFound {
            resource: "CalendarEvent".to_string(),
            id: event_id.to_string(),
        })
    })
}

pub async fn create_event(
    client: &Client,
    form: &EventFormData,
) -> Result<CalendarEventId, MissiveError> {
    let account_id = client.default_account_id();
    let calendar_id = fetch_default_calendar_id(client).await?;
    let event = form_to_jscalendar(form, &calendar_id);

    let request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "CalendarEvent/set",
            serde_json::json!({
                "create": {
                    "new1": event
                }
            }),
            "s0",
        )],
    );

    let response = jmap_raw::send_raw_jmap_request(client, &request).await?;
    let data = jmap_raw::extract_method_response(&response, "s0")?;

    if let Some(not_created) = data["notCreated"].as_object()
        && let Some(err) = not_created.get("new1")
    {
        let err_type = err["type"].as_str().unwrap_or("unknown");
        let err_desc = err["description"].as_str().unwrap_or("");
        return Err(MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "CalendarEvent/set create".to_string(),
            message: format!("{err_type}: {err_desc}"),
        }));
    }

    let created_id = data["created"]["new1"]["id"].as_str().ok_or_else(|| {
        MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "CalendarEvent/set create".to_string(),
            message: "no id in created response".to_string(),
        })
    })?;

    trace!("create_event: created {created_id}");
    Ok(CalendarEventId::from(created_id))
}

pub async fn update_event(
    client: &Client,
    event_id: &CalendarEventId,
    form: &EventFormData,
) -> Result<(), MissiveError> {
    let account_id = client.default_account_id();
    let calendar_id = fetch_default_calendar_id(client).await?;
    let event = form_to_jscalendar(form, &calendar_id);

    let request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "CalendarEvent/set",
            serde_json::json!({
                "update": {
                    event_id.as_str(): event
                }
            }),
            "s0",
        )],
    );

    let response = jmap_raw::send_raw_jmap_request(client, &request).await?;
    let data = jmap_raw::extract_method_response(&response, "s0")?;

    if let Some(not_updated) = data["notUpdated"].as_object()
        && let Some(err) = not_updated.get(event_id.as_str())
    {
        let err_type = err["type"].as_str().unwrap_or("unknown");
        let err_desc = err["description"].as_str().unwrap_or("");
        return Err(MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "CalendarEvent/set update".to_string(),
            message: format!("{err_type}: {err_desc}"),
        }));
    }

    trace!("update_event: updated {event_id}");
    Ok(())
}

pub async fn delete_event(
    client: &Client,
    event_id: &CalendarEventId,
) -> Result<(), MissiveError> {
    let account_id = client.default_account_id();

    let request = jmap_raw::build_jmap_request(
        account_id,
        USING_CALENDARS,
        vec![(
            "CalendarEvent/set",
            serde_json::json!({
                "destroy": [event_id.as_str()]
            }),
            "s0",
        )],
    );

    let response = jmap_raw::send_raw_jmap_request(client, &request).await?;
    let data = jmap_raw::extract_method_response(&response, "s0")?;

    if let Some(not_destroyed) = data["notDestroyed"].as_object()
        && let Some(err) = not_destroyed.get(event_id.as_str())
    {
        let err_type = err["type"].as_str().unwrap_or("unknown");
        let err_desc = err["description"].as_str().unwrap_or("");
        return Err(MissiveError::Jmap(JmapErrorKind::QueryFailed {
            method: "CalendarEvent/set destroy".to_string(),
            message: format!("{err_type}: {err_desc}"),
        }));
    }

    trace!("delete_event: deleted {event_id}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // --- parse_duration_display ---

    #[test]
    fn duration_all_day() {
        assert_eq!(parse_duration_display("P1D"), "All day");
    }

    #[test]
    fn duration_multi_day() {
        assert_eq!(parse_duration_display("P3D"), "3 days");
    }

    #[test]
    fn duration_30_min() {
        assert_eq!(parse_duration_display("PT30M"), "30 min");
    }

    #[test]
    fn duration_1_hour() {
        assert_eq!(parse_duration_display("PT1H"), "1 hr");
    }

    #[test]
    fn duration_1h30m() {
        assert_eq!(parse_duration_display("PT1H30M"), "1 hr 30 min");
    }

    #[test]
    fn duration_empty() {
        assert_eq!(parse_duration_display(""), "");
    }

    // --- parse_start_display ---

    #[test]
    fn start_with_time() {
        let (date, time, naive) = parse_start_display("2025-06-23T20:00:00", false);
        assert_eq!(date, "June 23, 2025");
        assert_eq!(time, "8 PM");
        assert!(naive.is_some());
    }

    #[test]
    fn start_all_day() {
        let (date, time, _) = parse_start_display("2025-06-19T00:00:00", true);
        assert_eq!(date, "June 19, 2025");
        assert_eq!(time, "");
    }

    #[test]
    fn start_with_minutes() {
        let (_, time, _) = parse_start_display("2025-06-23T14:30:00", false);
        assert_eq!(time, "2:30 PM");
    }

    #[test]
    fn start_midnight() {
        let (_, time, _) = parse_start_display("2025-06-23T00:00:00", false);
        assert_eq!(time, "12 AM");
    }

    #[test]
    fn start_noon() {
        let (_, time, _) = parse_start_display("2025-06-23T12:00:00", false);
        assert_eq!(time, "12 PM");
    }

    // --- extract_location ---

    #[test]
    fn location_from_map() {
        let mut map = HashMap::new();
        map.insert(
            "loc1".to_string(),
            EventLocation {
                name: Some("Conference Room".to_string()),
                location_type: None,
            },
        );
        assert_eq!(extract_location(&Some(map)), "Conference Room");
    }

    #[test]
    fn location_none() {
        assert_eq!(extract_location(&None), "");
    }

    // --- event_card_to_summary ---

    #[test]
    fn summary_from_timed_event() {
        let card = JsCalendarEvent {
            id: Some("e1".to_string()),
            title: Some("Team Meeting".to_string()),
            start: Some("2025-06-23T14:00:00".to_string()),
            duration: Some("PT1H".to_string()),
            show_without_time: Some(false),
            ..Default::default()
        };
        let summary = event_card_to_summary(&card).unwrap();
        assert_eq!(summary.title, "Team Meeting");
        assert_eq!(summary.time_display, "2 PM");
        assert!(!summary.is_all_day);
    }

    #[test]
    fn summary_from_all_day_event() {
        let card = JsCalendarEvent {
            id: Some("e2".to_string()),
            title: Some("Holiday".to_string()),
            start: Some("2025-12-25T00:00:00".to_string()),
            duration: Some("P1D".to_string()),
            show_without_time: Some(true),
            ..Default::default()
        };
        let summary = event_card_to_summary(&card).unwrap();
        assert!(summary.is_all_day);
        assert_eq!(summary.time_display, "");
    }

    #[test]
    fn summary_requires_id() {
        let card = JsCalendarEvent {
            title: Some("No ID".to_string()),
            ..Default::default()
        };
        assert!(event_card_to_summary(&card).is_none());
    }

    // --- build_calendar_month ---

    #[test]
    fn calendar_month_march_2026() {
        let events = HashSet::new();
        let month = build_calendar_month(2026, 3, None, &events);
        assert_eq!(month.month_name, "March 2026");
        assert_eq!(month.days.len(), 42);
        // March 1, 2026 is a Sunday — grid starts on Monday Feb 23
        assert_eq!(month.days[0].day, 23);
        assert!(!month.days[0].is_current_month);
        // Find March 1
        let march_1 = month.days.iter().find(|d| d.day == 1 && d.is_current_month);
        assert!(march_1.is_some());
        assert_eq!(month.prev_year, 2026);
        assert_eq!(month.prev_month, 2);
        assert_eq!(month.next_year, 2026);
        assert_eq!(month.next_month, 4);
    }

    #[test]
    fn calendar_month_january_wrap() {
        let events = HashSet::new();
        let month = build_calendar_month(2026, 1, None, &events);
        assert_eq!(month.prev_year, 2025);
        assert_eq!(month.prev_month, 12);
    }

    #[test]
    fn calendar_month_december_wrap() {
        let events = HashSet::new();
        let month = build_calendar_month(2025, 12, None, &events);
        assert_eq!(month.next_year, 2026);
        assert_eq!(month.next_month, 1);
    }

    #[test]
    fn calendar_month_event_dots() {
        let mut events = HashSet::new();
        events.insert(NaiveDate::from_ymd_opt(2026, 3, 14).unwrap());
        let month = build_calendar_month(2026, 3, None, &events);
        let march_14 = month
            .days
            .iter()
            .find(|d| d.day == 14 && d.is_current_month)
            .unwrap();
        assert!(march_14.has_events);
    }

    // --- form_to_jscalendar ---

    #[test]
    fn form_to_timed_event() {
        let form = EventFormData {
            title: "Standup".to_string(),
            start_date: "2026-03-14".to_string(),
            start_time: "09:00".to_string(),
            duration_hours: "0".to_string(),
            duration_minutes: "30".to_string(),
            ..Default::default()
        };
        let event = form_to_jscalendar(&form, "cal1");
        assert_eq!(event.title.as_deref(), Some("Standup"));
        assert_eq!(event.start.as_deref(), Some("2026-03-14T09:00:00"));
        assert_eq!(event.duration.as_deref(), Some("PT30M"));
        assert!(event.show_without_time.is_none());
        assert!(event.calendar_ids.as_ref().unwrap().contains_key("cal1"));
    }

    #[test]
    fn form_to_all_day_event() {
        let form = EventFormData {
            title: "Holiday".to_string(),
            start_date: "2026-12-25".to_string(),
            is_all_day: "on".to_string(),
            ..Default::default()
        };
        let event = form_to_jscalendar(&form, "cal1");
        assert_eq!(event.duration.as_deref(), Some("P1D"));
        assert_eq!(event.show_without_time, Some(true));
    }

    // --- format_time_12h ---

    #[test]
    fn format_time_am() {
        assert_eq!(format_time_12h("09:15"), "9:15 AM");
    }

    #[test]
    fn format_time_pm() {
        assert_eq!(format_time_12h("13:00"), "1 PM");
    }

    #[test]
    fn format_time_midnight() {
        assert_eq!(format_time_12h("00:00"), "12 AM");
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
    async fn live_fetch_events() {
        let client = live_connect().await;
        let events = fetch_events_for_range(&client, "2025-01-01", "2026-12-31")
            .await
            .expect("fetch_events_for_range failed");
        println!("Got {} events", events.len());
        for e in &events {
            println!(
                "  - {} | {} | {}",
                e.title, e.time_display, e.location
            );
        }
        assert!(!events.is_empty(), "Expected at least one event");
    }

    #[tokio::test]
    #[ignore = "requires live JMAP server"]
    async fn live_fetch_july_events_with_dates() {
        let client = live_connect().await;
        let events = fetch_events_for_range(&client, "2025-07-01", "2025-07-31")
            .await
            .expect("fetch_events failed");
        println!("July 2025: {} events", events.len());
        let dates = event_dates_set(&events);
        println!("Event dates: {:?}", dates);
        for e in &events {
            println!(
                "  - {} | start_date={:?} | time={}",
                e.title, e.start_date, e.time_display
            );
        }
        assert!(!events.is_empty(), "Expected July events");
        assert!(!dates.is_empty(), "Expected event dates set to be non-empty");
    }

    #[tokio::test]
    #[ignore = "requires live JMAP server"]
    async fn live_fetch_event_detail() {
        let client = live_connect().await;
        let events = fetch_events_for_range(&client, "2025-01-01", "2026-12-31")
            .await
            .expect("fetch_events failed");
        assert!(!events.is_empty(), "Need at least one event");

        let detail = fetch_event_detail(&client, &events[0].id)
            .await
            .expect("fetch_event_detail failed");
        println!(
            "Detail: {} | {} {} | dur={} | loc={} | participants={}",
            detail.title,
            detail.date_display,
            detail.time_display,
            detail.duration_display,
            detail.location,
            detail.participants.len()
        );
        assert!(!detail.title.is_empty());
    }
}
