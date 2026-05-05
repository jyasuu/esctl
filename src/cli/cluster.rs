use anyhow::Result;
use clap::Subcommand;

use crate::client::EsClient;
use crate::config::AppConfig;

#[derive(Subcommand, Debug)]
pub enum ClusterCommands {
    /// Cluster health overview
    Health {
        /// Detail level: cluster | indices | shards
        #[arg(long, default_value = "cluster")]
        level: String,
    },
    /// Basic cluster info
    Info,
    /// Cluster-wide stats
    Stats,
    /// List nodes
    Nodes {
        #[arg(long)]
        verbose: bool,
    },
}

pub async fn handle(cmd: ClusterCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        ClusterCommands::Health { level } => {
            let resp = client
                .get(&format!("/_cluster/health?level={}", level))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        ClusterCommands::Info => {
            let resp = client.get("/").await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        ClusterCommands::Stats => {
            let resp = client.get("/_cluster/stats").await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        ClusterCommands::Nodes { verbose } => {
            let path = if verbose {
                "/_nodes?pretty"
            } else {
                "/_cat/nodes?format=json"
            };
            let resp = client.get(path).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
