use anyhow::Result;
use clap::{Args, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum IndexCommands {
    /// List indices (supports glob pattern)
    List {
        #[arg(default_value = "*")]
        pattern: String,
    },
    /// Create an index
    Create(CreateArgs),
    /// Delete an index
    Delete {
        name: String,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Open a closed index
    Open { name: String },
    /// Close an index
    Close { name: String },
    /// Index stats
    Stats { name: String },
}

#[derive(Args, Debug)]
pub struct CreateArgs {
    pub name: String,
    #[arg(long, default_value = "1")]
    pub shards: u32,
    #[arg(long, default_value = "1")]
    pub replicas: u32,
    /// JSON body (file path or - for stdin)
    #[arg(long)]
    pub body: Option<String>,
}

pub async fn handle(cmd: IndexCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        IndexCommands::List { pattern } => {
            let resp = client
                .get(&format!("/_cat/indices/{}?format=json&s=index", pattern))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        IndexCommands::Create(args) => {
            let body = if let Some(ref src) = args.body {
                stdin::read_json(src).await?
            } else {
                serde_json::json!({
                    "settings": {
                        "number_of_shards": args.shards,
                        "number_of_replicas": args.replicas,
                    }
                })
            };
            let resp = client.put(&format!("/{}", args.name), &body).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        IndexCommands::Delete { name, yes } => {
            if !yes {
                print!("Delete index '{}'? [y/N] ", name);
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let resp = client.delete(&format!("/{}", name)).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        IndexCommands::Open { name } => {
            let resp = client
                .post(&format!("/{}/_open", name), &serde_json::Value::Null)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        IndexCommands::Close { name } => {
            let resp = client
                .post(&format!("/{}/_close", name), &serde_json::Value::Null)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        IndexCommands::Stats { name } => {
            let resp = client.get(&format!("/{}/_stats", name)).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
