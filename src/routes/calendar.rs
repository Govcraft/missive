use acton_service::prelude::*;
use acton_service::session::{FlashMessage, FlashMessages, Session};
use askama::Template;
use chrono::{Datelike, Local, NaiveDate};

use crate::calendar::{
    self, CalendarEventId, CalendarMonth, EventDetail, EventFormData, EventSummary, DAY_NAMES,
};
use crate::error::MissiveError;
use crate::session::AuthenticatedClient;

// ---------------------------------------------------------------------------
// Template structs
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/calendar_mini.html")]
struct CalendarMiniTemplate {
    month: CalendarMonth,
}

#[derive(Template)]
#[template(path = "partials/event_list.html")]
struct EventListTemplate {
    events: Vec<EventSummary>,
    date_heading: String,
}

#[derive(Template)]
#[template(path = "partials/event_detail.html")]
struct EventDetailTemplate {
    event: EventDetail,
}

#[derive(Template)]
#[template(path = "partials/event_form.html")]
struct EventFormTemplate {
    event: Option<EventDetail>,
    is_edit: bool,
    edit_id: String,
}

#[derive(Template)]
#[template(path = "partials/event_empty_state.html")]
struct EventEmptyStateTemplate;

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct MonthParams {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub month: Option<u32>,
}

#[derive(Deserialize)]
pub struct EventListParams {
    #[serde(default)]
    pub date: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn get_month(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Query(params): Query<MonthParams>,
) -> std::result::Result<Response, MissiveError> {
    let today = Local::now().date_naive();
    let year = params.year.unwrap_or_else(|| today.year());
    let month = params.month.unwrap_or_else(|| today.month());

    // Get first and last day of the month for event query
    let first = format!("{year}-{month:02}-01");
    let last = last_day_of_month(year, month);

    let events = calendar::fetch_events_for_range(&client, &first, &last).await?;
    let event_dates = calendar::event_dates_set(&events);

    let calendar_month = calendar::build_calendar_month(year, month, None, &event_dates);

    // Build month name heading for event list
    let heading = NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| d.format("%B %Y").to_string())
        .unwrap_or_else(|| format!("{year}-{month:02}"));

    // Render calendar grid + OOB event list in one response
    let cal_html = CalendarMiniTemplate {
        month: calendar_month,
    }
    .render()
    .map_err(|e| MissiveError::HttpResponse(e.to_string()))?;

    let events_html = EventListTemplate {
        events,
        date_heading: heading,
    }
    .render()
    .map_err(|e| MissiveError::HttpResponse(e.to_string()))?;

    let combined = format!(
        "{cal_html}<div id=\"event-directory\" hx-swap-oob=\"innerHTML\">{events_html}</div>"
    );

    Ok(Html(combined).into_response())
}

pub async fn list_events(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Query(params): Query<EventListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let today = Local::now().date_naive();

    let (after, before, heading) = if let Some(ref date_str) = params.date {
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            (
                date_str.clone(),
                date_str.clone(),
                date.format("%B %-d, %Y").to_string(),
            )
        } else {
            // Invalid date, fall back to current month
            let first = format!("{}-{:02}-01", today.year(), today.month());
            let last = last_day_of_month(today.year(), today.month());
            (first, last, today.format("%B %Y").to_string())
        }
    } else {
        // No date specified — show current month
        let first = format!("{}-{:02}-01", today.year(), today.month());
        let last = last_day_of_month(today.year(), today.month());
        (first, last, today.format("%B %Y").to_string())
    };

    let events = calendar::fetch_events_for_range(&client, &after, &before).await?;

    Ok(HtmlTemplate::page(EventListTemplate {
        events,
        date_heading: heading,
    }))
}

pub async fn get_event(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Path(id): Path<CalendarEventId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let event = calendar::fetch_event_detail(&client, &id).await?;
    Ok(HtmlTemplate::page(EventDetailTemplate { event }))
}

pub async fn new_event_form(
    AuthenticatedClient(_, _, _): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    Ok(HtmlTemplate::page(EventFormTemplate {
        event: None,
        is_edit: false,
        edit_id: String::new(),
    }))
}

pub async fn edit_event_form(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Path(id): Path<CalendarEventId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let event = calendar::fetch_event_detail(&client, &id).await?;
    let edit_id = id.to_string();
    Ok(HtmlTemplate::page(EventFormTemplate {
        event: Some(event),
        is_edit: true,
        edit_id,
    }))
}

pub async fn create_event(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Form(form): Form<EventFormData>,
) -> std::result::Result<Response, MissiveError> {
    let created_id = calendar::create_event(&client, &form).await?;
    let event = calendar::fetch_event_detail(&client, &created_id).await?;

    push_flash(&session, FlashMessage::success("Event created")).await;

    Ok(HtmlTemplate::page(EventDetailTemplate { event })
        .with_hx_trigger("calendarUpdated, flashUpdated")
        .into_response())
}

pub async fn update_event(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<CalendarEventId>,
    Form(form): Form<EventFormData>,
) -> std::result::Result<Response, MissiveError> {
    calendar::update_event(&client, &id, &form).await?;
    let event = calendar::fetch_event_detail(&client, &id).await?;

    push_flash(&session, FlashMessage::success("Event updated")).await;

    Ok(HtmlTemplate::page(EventDetailTemplate { event })
        .with_hx_trigger("calendarUpdated, flashUpdated")
        .into_response())
}

pub async fn delete_event(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<CalendarEventId>,
) -> std::result::Result<Response, MissiveError> {
    calendar::delete_event(&client, &id).await?;

    push_flash(&session, FlashMessage::success("Event deleted")).await;

    Ok(HtmlTemplate::page(EventEmptyStateTemplate)
        .with_hx_trigger("calendarUpdated, flashUpdated")
        .into_response())
}

pub async fn cancel_form(
    AuthenticatedClient(_, _, _): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    Ok(HtmlTemplate::page(EventEmptyStateTemplate))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn last_day_of_month(year: i32, month: u32) -> String {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_of_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .unwrap_or_else(|| Local::now().date_naive());
    let last = first_of_next - chrono::Duration::days(1);
    last.format("%Y-%m-%d").to_string()
}

async fn push_flash(session: &Session, message: FlashMessage) {
    let description = format!("{:?}: {}", message.kind, message.message);
    match FlashMessages::push(session, message).await {
        Ok(()) => trace!("flash message pushed: {description}"),
        Err(e) => error!("failed to push flash message ({description}): {e}"),
    }
}
