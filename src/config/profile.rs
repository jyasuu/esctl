use serde::{Deserialize, Serialize};

/// A named connection profile stored in ~/.esctl/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Elasticsearch host URL
    pub host: String,

    /// Auth method: "basic" | "apikey" | "none"
    #[serde(default = "default_auth")]
    pub auth: String,

    /// Username for basic auth (password stored in OS keyring)
    pub username: Option<String>,

    /// API key (consider using keyring instead of plain text)
    pub api_key: Option<String>,
}

fn default_auth() -> String {
    "none".to_string()
}
