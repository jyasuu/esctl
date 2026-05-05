use anyhow::Result;
use clap::{Args, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum DocCommands {
    /// Get a document by ID
    Get { index: String, id: String },
    /// Index (create/replace) a document
    Index(DocIndexArgs),
    /// Update a document
    Update {
        index: String,
        id: String,
        /// JSON body (file path or - for stdin)
        #[arg(long)]
        body: String,
    },
    /// Delete a document by ID
    Delete { index: String, id: String },
    /// Bulk index documents from NDJSON (file or stdin)
    Bulk {
        index: String,
        /// NDJSON file path or - for stdin
        source: Option<String>,
    },
    /// Write a single document (reads JSON from stdin or file)
    Write {
        index: String,
        #[arg(long)]
        id: Option<String>,
        /// JSON body (file path or - for stdin)
        #[arg(long)]
        body: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct DocIndexArgs {
    pub index: String,
    #[arg(long)]
    pub id: Option<String>,
    /// JSON body (file path or - for stdin)
    #[arg(long)]
    pub body: String,
}

pub async fn handle(cmd: DocCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        DocCommands::Get { index, id } => {
            let resp = client.get(&format!("/{}/{}/_doc/{}", index, index, id)).await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        DocCommands::Index(args) => {
            let body = stdin::read_json(&args.body).await?;
            let resp = if let Some(id) = args.id {
                client.put(&format!("/{index}/_doc/{id}", index = args.index, id = id), &body).await?
            } else {
                client.post(&format!("/{index}/_doc", index = args.index), &body).await?
            };
            cfg.output.print(&resp, cfg.no_color);
        }
        DocCommands::Update { index, id, body } => {
            let doc = stdin::read_json(&body).await?;
            let payload = serde_json::json!({ "doc": doc });
            let resp = client
                .post(&format!("/{}/_update/{}", index, id), &payload)
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        DocCommands::Delete { index, id } => {
            let resp = client
                .delete(&format!("/{index}/_doc/{id}", index = index, id = id))
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        DocCommands::Bulk { index, source } => {
            let src = source.unwrap_or_else(|| "-".to_string());
            let ndjson = stdin::read_ndjson(&src).await?;
            let resp = client
                .post_raw(&format!("/{}/_bulk", index), ndjson, "application/x-ndjson")
                .await?;
            cfg.output.print(&resp, cfg.no_color);
        }
        DocCommands::Write { index, id, body } => {
            let src = body.unwrap_or_else(|| "-".to_string());
            let doc = stdin::read_json(&src).await?;
            let resp = if let Some(id) = id {
                client.put(&format!("/{index}/_doc/{id}", index = index, id = id), &doc).await?
            } else {
                client.post(&format!("/{index}/_doc", index = index), &doc).await?
            };
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}
