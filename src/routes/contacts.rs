use acton_service::prelude::*;
use acton_service::session::{FlashMessage, FlashMessages, Session};

use crate::config::MissiveConfig;
use crate::contacts::{self, ContactDetail, ContactFormData, ContactGroup, ContactId};
use crate::error::MissiveError;
use crate::jmap::SearchQuery;
use crate::session::AuthenticatedClient;

// ---------------------------------------------------------------------------
// Template structs
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/contact_list.html")]
struct ContactListTemplate {
    groups: Vec<ContactGroup>,
    search: Option<SearchQuery>,
}

#[derive(Template)]
#[template(path = "partials/contact_detail.html")]
struct ContactDetailTemplate {
    contact: ContactDetail,
}

#[derive(Template)]
#[template(path = "partials/contact_form.html")]
struct ContactFormTemplate {
    contact: Option<ContactDetail>,
    is_edit: bool,
    edit_id: String,
}

#[derive(Template)]
#[template(path = "partials/contact_empty_state.html")]
struct ContactEmptyStateTemplate;

// ---------------------------------------------------------------------------
// Query / form params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ContactListParams {
    #[serde(default)]
    pub search: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn list_contacts(
    State(state): State<AppState<MissiveConfig>>,
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Query(params): Query<ContactListParams>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let search = params.search.as_deref().and_then(SearchQuery::new);
    let page_size = state.config().custom.page_size;

    let (summaries, _total) =
        contacts::fetch_contacts(&client, 0, page_size * 10, search.as_ref()).await?;

    let groups = contacts::group_contacts_alphabetically(summaries);

    Ok(HtmlTemplate::page(ContactListTemplate {
        groups,
        search,
    }))
}

pub async fn get_contact(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Path(id): Path<ContactId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let contact = contacts::fetch_contact_detail(&client, &id).await?;
    Ok(HtmlTemplate::page(ContactDetailTemplate { contact }))
}

pub async fn new_contact_form(
    AuthenticatedClient(_, _, _): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    Ok(HtmlTemplate::page(ContactFormTemplate {
        contact: None,
        is_edit: false,
        edit_id: String::new(),
    }))
}

pub async fn edit_contact_form(
    AuthenticatedClient(client, _, _): AuthenticatedClient,
    Path(id): Path<ContactId>,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    let contact = contacts::fetch_contact_detail(&client, &id).await?;
    let edit_id = id.to_string();
    Ok(HtmlTemplate::page(ContactFormTemplate {
        contact: Some(contact),
        is_edit: true,
        edit_id,
    }))
}

pub async fn create_contact(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Form(form): Form<ContactFormData>,
) -> std::result::Result<Response, MissiveError> {
    let created_id = contacts::create_contact(&client, &form).await?;
    let contact = contacts::fetch_contact_detail(&client, &created_id).await?;

    push_flash(&session, FlashMessage::success("Contact created")).await;

    Ok(HtmlTemplate::page(ContactDetailTemplate { contact })
        .with_hx_trigger("contactsUpdated, flashUpdated")
        .into_response())
}

pub async fn update_contact(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<ContactId>,
    Form(form): Form<ContactFormData>,
) -> std::result::Result<Response, MissiveError> {
    contacts::update_contact(&client, &id, &form).await?;
    let contact = contacts::fetch_contact_detail(&client, &id).await?;

    push_flash(&session, FlashMessage::success("Contact updated")).await;

    Ok(HtmlTemplate::page(ContactDetailTemplate { contact })
        .with_hx_trigger("contactsUpdated, flashUpdated")
        .into_response())
}

pub async fn delete_contact(
    AuthenticatedClient(client, _, session): AuthenticatedClient,
    Path(id): Path<ContactId>,
) -> std::result::Result<Response, MissiveError> {
    contacts::delete_contact(&client, &id).await?;

    push_flash(&session, FlashMessage::success("Contact deleted")).await;

    Ok(HtmlTemplate::page(ContactEmptyStateTemplate)
        .with_hx_trigger("contactsUpdated, flashUpdated")
        .into_response())
}

pub async fn cancel_form(
    AuthenticatedClient(_, _, _): AuthenticatedClient,
) -> std::result::Result<impl IntoResponse, MissiveError> {
    Ok(HtmlTemplate::page(ContactEmptyStateTemplate))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn push_flash(session: &Session, message: FlashMessage) {
    let description = format!("{:?}: {}", message.kind, message.message);
    match FlashMessages::push(session, message).await {
        Ok(()) => trace!("flash message pushed: {description}"),
        Err(e) => error!("failed to push flash message ({description}): {e}"),
    }
}
