use acton_service::prelude::*;

#[derive(Default, Serialize, Deserialize)]
pub struct PostalSession {
    pub username: Option<String>,
    pub password: Option<String>,
}

pub fn get_credentials(session: &TypedSession<PostalSession>) -> Option<(String, String)> {
    let data = session.data();
    match (&data.username, &data.password) {
        (Some(u), Some(p)) => Some((u.clone(), p.clone())),
        _ => None,
    }
}
