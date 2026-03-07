use acton_service::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PostalConfig {
    pub jmap_url: String,
}
