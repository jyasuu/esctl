use anyhow::Result;
use clap::Subcommand;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum SettingsCommands {
    /// Get settings for an index
    Get {
        index: String,
        #[arg(long)]
        include_defaults: bool,
    },
    /// Update settings for an index (file or - for stdin)
    Put {
        index: String,
        #[arg(long)]
        body: String,
    },
}

pub async fn handle(cmd: SettingsCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        SettingsCommands::Get {
            index,
            include_defaults,
        } => {
            let path = if include_defaults {
                format!("/{}/_settings?include_defaults=true", index)
            } else {
                format!("/{}/_settings", index)
            };
            let resp = client.get(&path).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        SettingsCommands::Put { index, body } => {
            let payload = stdin::read_json(&body).await?;
            let resp = client
                .put(&format!("/{}/_settings", index), &payload)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
