pub mod profile;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::cli::Cli;
use crate::output::OutputFormat;
pub use profile::Profile;

/// Full application configuration, loaded from ~/.esctl/config.toml
/// and then overlaid with CLI flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Name of the active profile
    #[serde(default = "default_profile_name")]
    pub default_profile: String,

    /// All named profiles
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,

    // ── Runtime fields (not serialized) ──────────────────────────────────────
    /// Active host (resolved from profile + CLI override)
    #[serde(skip)]
    pub host: Option<String>,

    #[serde(skip)]
    pub username: Option<String>,

    #[serde(skip)]
    pub password: Option<String>,

    #[serde(skip)]
    pub api_key: Option<String>,

    #[serde(skip)]
    pub output: OutputFormat,

    #[serde(skip)]
    pub no_color: bool,

    #[serde(skip)]
    pub tls_skip_verify: bool,

    #[serde(skip)]
    pub timeout_secs: Option<u64>,
}

fn default_profile_name() -> String {
    "default".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_profile: "default".to_string(),
            profiles: HashMap::new(),
            host: Some("http://localhost:9200".to_string()),
            username: None,
            password: None,
            api_key: None,
            output: OutputFormat::Json,
            no_color: false,
            tls_skip_verify: false,
            timeout_secs: None,
        }
    }
}

impl AppConfig {
    /// Path to the config file: ~/.esctl/config.toml
    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".esctl")
            .join("config.toml")
    }

    /// Load config from disk. Missing file → default config.
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;

        let mut cfg: Self = toml::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;

        // Hydrate runtime fields from the default profile
        cfg.hydrate_from_active_profile();
        Ok(cfg)
    }

    /// Save current config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Apply CLI flag overrides on top of the loaded config.
    pub fn apply_cli_overrides(&mut self, cli: &Cli) {
        // Profile switch
        if let Some(ref p) = cli.profile {
            self.default_profile = p.clone();
            self.hydrate_from_active_profile();
        }
        if let Some(ref h) = cli.host     { self.host     = Some(h.clone()); }
        if let Some(ref u) = cli.user     { self.username = Some(u.clone()); }
        if let Some(ref p) = cli.password { self.password = Some(p.clone()); }
        if let Some(ref k) = cli.api_key  { self.api_key  = Some(k.clone()); }

        self.output      = cli.output.clone();
        self.no_color    = cli.no_color;
        self.tls_skip_verify = cli.insecure;
        if cli.timeout.is_some() {
            self.timeout_secs = cli.timeout;
        }
    }

    /// Return the active host, falling back to localhost.
    pub fn active_host(&self) -> String {
        self.host
            .clone()
            .unwrap_or_else(|| "http://localhost:9200".to_string())
    }

    // ── Profile helpers ───────────────────────────────────────────────────────

    pub fn list_profiles(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = self
            .profiles
            .keys()
            .map(|k| (k.clone(), k == &self.default_profile))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn get_profile(&self, name: &str) -> Result<&Profile> {
        self.profiles
            .get(name)
            .ok_or_else(|| anyhow!("profile '{}' not found", name))
    }

    pub fn add_profile(&mut self, args: &crate::cli::profile::AddArgs) -> Result<()> {
        let profile = Profile {
            host: args.host.clone(),
            auth: args.auth.clone(),
            username: args.user.clone(),
            api_key: args.api_key.clone(),
        };
        self.profiles.insert(args.name.clone(), profile);
        Ok(())
    }

    pub fn remove_profile(&mut self, name: &str) -> Result<()> {
        if self.profiles.remove(name).is_none() {
            return Err(anyhow!("profile '{}' not found", name));
        }
        if self.default_profile == name {
            self.default_profile = "default".to_string();
        }
        Ok(())
    }

    pub fn set_default_profile(&mut self, name: &str) -> Result<()> {
        if !self.profiles.contains_key(name) {
            return Err(anyhow!("profile '{}' not found", name));
        }
        self.default_profile = name.to_string();
        Ok(())
    }

    /// Copy host/auth fields from the active profile into runtime fields.
    fn hydrate_from_active_profile(&mut self) {
        if let Some(p) = self.profiles.get(&self.default_profile).cloned() {
            self.host     = Some(p.host.clone());
            self.username = p.username.clone();
            self.api_key  = p.api_key.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_host() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.active_host(), "http://localhost:9200");
    }

    #[test]
    fn test_add_and_get_profile() {
        let mut cfg = AppConfig::default();
        let args = crate::cli::profile::AddArgs {
            name: "test".to_string(),
            host: "http://es.local:9200".to_string(),
            auth: "basic".to_string(),
            user: Some("alice".to_string()),
            api_key: None,
        };
        cfg.add_profile(&args).unwrap();
        let p = cfg.get_profile("test").unwrap();
        assert_eq!(p.host, "http://es.local:9200");
        assert_eq!(p.username, Some("alice".to_string()));
    }

    #[test]
    fn test_remove_profile() {
        let mut cfg = AppConfig::default();
        let args = crate::cli::profile::AddArgs {
            name: "temp".to_string(),
            host: "http://x:9200".to_string(),
            auth: "none".to_string(),
            user: None,
            api_key: None,
        };
        cfg.add_profile(&args).unwrap();
        cfg.remove_profile("temp").unwrap();
        assert!(cfg.get_profile("temp").is_err());
    }

    #[test]
    fn test_remove_nonexistent_profile_errors() {
        let mut cfg = AppConfig::default();
        assert!(cfg.remove_profile("ghost").is_err());
    }

    #[test]
    fn test_set_default_profile() {
        let mut cfg = AppConfig::default();
        let args = crate::cli::profile::AddArgs {
            name: "prod".to_string(),
            host: "https://prod:9243".to_string(),
            auth: "apikey".to_string(),
            user: None,
            api_key: Some("key123".to_string()),
        };
        cfg.add_profile(&args).unwrap();
        cfg.set_default_profile("prod").unwrap();
        assert_eq!(cfg.default_profile, "prod");
    }

    #[test]
    fn test_set_default_nonexistent_errors() {
        let mut cfg = AppConfig::default();
        assert!(cfg.set_default_profile("nope").is_err());
    }

    #[test]
    fn test_list_profiles_sorted() {
        let mut cfg = AppConfig::default();
        for name in &["zoo", "alpha", "middle"] {
            let args = crate::cli::profile::AddArgs {
                name: name.to_string(),
                host: "http://x:9200".to_string(),
                auth: "none".to_string(),
                user: None, api_key: None,
            };
            cfg.add_profile(&args).unwrap();
        }
        let names: Vec<String> = cfg.list_profiles().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["alpha", "middle", "zoo"]);
    }

    #[test]
    fn test_hydrate_from_active_profile() {
        let mut cfg = AppConfig::default();
        let args = crate::cli::profile::AddArgs {
            name: "local".to_string(),
            host: "http://127.0.0.1:9200".to_string(),
            auth: "basic".to_string(),
            user: Some("bob".to_string()),
            api_key: None,
        };
        cfg.add_profile(&args).unwrap();
        cfg.default_profile = "local".to_string();
        cfg.hydrate_from_active_profile();
        assert_eq!(cfg.host, Some("http://127.0.0.1:9200".to_string()));
        assert_eq!(cfg.username, Some("bob".to_string()));
    }
}
