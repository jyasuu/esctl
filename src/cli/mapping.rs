use anyhow::Result;
use clap::{Args, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

// ── Mappings ─────────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum MappingCommands {
    /// Get the mapping for an index
    Get { index: String },
    /// Update the mapping for an index (file or - for stdin)
    Put {
        index: String,
        #[arg(long)]
        body: String,
    },
}

pub async fn handle(cmd: MappingCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        MappingCommands::Get { index } => {
            let resp = client.get(&format!("/{}/_mapping", index)).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        MappingCommands::Put { index, body } => {
            let payload = stdin::read_json(&body).await?;
            let resp = client
                .put(&format!("/{}/_mapping", index), &payload)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
