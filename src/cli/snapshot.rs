use anyhow::Result;
use clap::{Args, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum SnapshotCommands {
    /// Snapshot repository management
    #[command(subcommand)]
    Repo(RepoCommands),

    /// List snapshots in a repository
    List { repo: String },

    /// Create a snapshot
    Create(SnapshotCreateArgs),

    /// Restore a snapshot
    Restore(SnapshotRestoreArgs),

    /// Delete a snapshot
    Delete { repo: String, name: String },

    /// Get snapshot status
    Status {
        repo: Option<String>,
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum RepoCommands {
    /// List all snapshot repositories
    List,
    /// Create a snapshot repository
    Create {
        name: String,
        #[arg(long, default_value = "fs")]
        r#type: String,
        /// Settings JSON (file or - for stdin)
        #[arg(long)]
        settings: String,
    },
    /// Delete a repository
    Delete { name: String },
}

#[derive(Args, Debug)]
pub struct SnapshotCreateArgs {
    pub repo: String,
    pub name: String,
    #[arg(long)]
    pub indices: Option<String>,
    /// Wait for completion
    #[arg(long)]
    pub wait: bool,
}

#[derive(Args, Debug)]
pub struct SnapshotRestoreArgs {
    pub repo: String,
    pub name: String,
    #[arg(long)]
    pub indices: Option<String>,
    #[arg(long)]
    pub wait: bool,
}

pub async fn handle(cmd: SnapshotCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        SnapshotCommands::Repo(r) => handle_repo(r, client, cfg).await,
        SnapshotCommands::List { repo } => {
            let resp = client
                .get(&format!("/_snapshot/{}/_all", repo))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        SnapshotCommands::Create(args) => {
            let mut body = serde_json::json!({});
            if let Some(ref indices) = args.indices {
                body["indices"] = serde_json::json!(indices);
            }
            let wait = if args.wait { "?wait_for_completion=true" } else { "" };
            let resp = client
                .put(
                    &format!("/_snapshot/{}/{}{}",args.repo, args.name, wait),
                    &body,
                )
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        SnapshotCommands::Restore(args) => {
            let mut body = serde_json::json!({});
            if let Some(ref indices) = args.indices {
                body["indices"] = serde_json::json!(indices);
            }
            let wait = if args.wait { "?wait_for_completion=true" } else { "" };
            let resp = client
                .post(
                    &format!("/_snapshot/{}/{}/_restore{}", args.repo, args.name, wait),
                    &body,
                )
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        SnapshotCommands::Delete { repo, name } => {
            let resp = client
                .delete(&format!("/_snapshot/{}/{}", repo, name))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        SnapshotCommands::Status { repo, name } => {
            let path = match (repo, name) {
                (Some(r), Some(n)) => format!("/_snapshot/{}/{}/_status", r, n),
                (Some(r), None) => format!("/_snapshot/{}/_status", r),
                _ => "/_snapshot/_status".to_string(),
            };
            let resp = client.get(&path).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}

async fn handle_repo(cmd: RepoCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        RepoCommands::List => {
            let resp = client.get("/_snapshot/_all").await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        RepoCommands::Create { name, r#type, settings } => {
            let s = stdin::read_json(&settings).await?;
            let body = serde_json::json!({ "type": r#type, "settings": s });
            let resp = client.put(&format!("/_snapshot/{}", name), &body).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        RepoCommands::Delete { name } => {
            let resp = client.delete(&format!("/_snapshot/{}", name)).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
