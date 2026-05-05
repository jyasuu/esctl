use anyhow::Result;
use clap::Subcommand;

use crate::client::EsClient;
use crate::config::AppConfig;

#[derive(Subcommand, Debug)]
pub enum AliasCommands {
    /// List aliases (optionally filter by index)
    List {
        #[arg(long)]
        index: Option<String>,
    },
    /// Add an alias to an index
    Add { alias: String, index: String },
    /// Remove an alias from an index
    Remove { alias: String, index: String },
}

pub async fn handle(cmd: AliasCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        AliasCommands::List { index } => {
            let path = match index {
                Some(i) => format!("/{}/_alias?format=json", i),
                None    => "/_cat/aliases?format=json".to_string(),
            };
            let resp = client.get(&path).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        AliasCommands::Add { alias, index } => {
            let body = serde_json::json!({
                "actions": [{ "add": { "index": index, "alias": alias } }]
            });
            let resp = client.post("/_aliases", &body).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        AliasCommands::Remove { alias, index } => {
            let body = serde_json::json!({
                "actions": [{ "remove": { "index": index, "alias": alias } }]
            });
            let resp = client.post("/_aliases", &body).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
