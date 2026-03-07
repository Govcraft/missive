use acton_service::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MissiveConfig {
    pub jmap_url: String,
}
