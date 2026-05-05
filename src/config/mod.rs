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

        self.output   = cli.output.clone();
        self.no_color = cli.no_color;
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
