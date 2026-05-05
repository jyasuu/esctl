use anyhow::Result;
use reqwest::RequestBuilder;

use crate::config::AppConfig;

/// Resolved authentication for a single request session.
pub enum Auth {
    None,
    Basic { user: String, password: String },
    ApiKey(String),
}

impl Auth {
    pub fn from_config(cfg: &AppConfig) -> Result<Self> {
        // API key takes priority
        if let Some(ref key) = cfg.api_key {
            return Ok(Auth::ApiKey(key.clone()));
        }

        if let Some(ref user) = cfg.username {
            let password = cfg
                .password
                .clone()
                .or_else(|| {
                    // Try OS keyring
                    keyring::Entry::new("esctl", user)
                        .ok()
                        .and_then(|e| e.get_password().ok())
                })
                .unwrap_or_default();

            return Ok(Auth::Basic {
                user: user.clone(),
                password,
            });
        }

        Ok(Auth::None)
    }

    /// Apply authentication headers/params to a request builder.
    pub fn apply(&self, req: RequestBuilder) -> RequestBuilder {
        match self {
            Auth::None => req,
            Auth::Basic { user, password } => req.basic_auth(user, Some(password)),
            Auth::ApiKey(key) => {
                req.header("Authorization", format!("ApiKey {}", key))
            }
        }
    }
}
