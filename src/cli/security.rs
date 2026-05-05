use anyhow::Result;
use clap::{Args, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

// ── Users ─────────────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum UserCommands {
    /// List all users
    List,
    /// Get a specific user
    Get { username: String },
    /// Create or update a user
    Create(UserCreateArgs),
    /// Delete a user
    Delete { username: String },
}

#[derive(Args, Debug)]
pub struct UserCreateArgs {
    pub username: String,
    #[arg(long)]
    pub roles: String,  // comma-separated
    #[arg(long)]
    pub password: String,
}

// ── Roles ─────────────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum RoleCommands {
    /// List all roles
    List,
    /// Get a specific role
    Get { name: String },
    /// Create or update a role (body: file or - for stdin)
    Create {
        name: String,
        #[arg(long)]
        body: String,
    },
    /// Delete a role
    Delete { name: String },
}

// ── API Keys ──────────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum ApikeyCommands {
    /// Create an API key
    Create(ApikeyCreateArgs),
    /// List API keys
    List,
    /// Invalidate an API key
    Invalidate { id: String },
}

#[derive(Args, Debug)]
pub struct ApikeyCreateArgs {
    #[arg(long)]
    pub name: String,
    /// Expiration, e.g. "30d"
    #[arg(long)]
    pub expiry: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

pub async fn handle_user(cmd: UserCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        UserCommands::List => {
            let resp = client.get("/_security/user").await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        UserCommands::Get { username } => {
            let resp = client
                .get(&format!("/_security/user/{}", username))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        UserCommands::Create(args) => {
            let roles: Vec<&str> = args.roles.split(',').map(str::trim).collect();
            let body = serde_json::json!({
                "password": args.password,
                "roles": roles,
            });
            let resp = client
                .put(&format!("/_security/user/{}", args.username), &body)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        UserCommands::Delete { username } => {
            let resp = client
                .delete(&format!("/_security/user/{}", username))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}

pub async fn handle_role(cmd: RoleCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        RoleCommands::List => {
            let resp = client.get("/_security/role").await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        RoleCommands::Get { name } => {
            let resp = client
                .get(&format!("/_security/role/{}", name))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        RoleCommands::Create { name, body } => {
            let payload = stdin::read_json(&body).await?;
            let resp = client
                .put(&format!("/_security/role/{}", name), &payload)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        RoleCommands::Delete { name } => {
            let resp = client
                .delete(&format!("/_security/role/{}", name))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}

pub async fn handle_apikey(cmd: ApikeyCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        ApikeyCommands::Create(args) => {
            let mut body = serde_json::json!({ "name": args.name });
            if let Some(expiry) = args.expiry {
                body["expiration"] = serde_json::json!(expiry);
            }
            let resp = client.post("/_security/api_key", &body).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        ApikeyCommands::List => {
            let resp = client
                .get("/_security/api_key?with_limited_by=true")
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        ApikeyCommands::Invalidate { id } => {
            let body = serde_json::json!({ "ids": [id] });
            let resp = client
                .delete_with_body("/_security/api_key", &body)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
