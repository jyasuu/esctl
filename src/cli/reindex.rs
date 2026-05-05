use anyhow::Result;
use clap::Args;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

#[derive(Args, Debug)]
pub struct ReindexArgs {
    /// Source index
    #[arg(long)]
    pub src: String,

    /// Destination index
    #[arg(long)]
    pub dst: String,

    /// Optional filter query (file or - for stdin)
    #[arg(long)]
    pub query: Option<String>,

    /// Wait for completion
    #[arg(long)]
    pub wait: bool,
}

pub async fn handle(args: ReindexArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let query = if let Some(ref q) = args.query {
        stdin::read_json(q).await?
    } else {
        serde_json::json!({ "match_all": {} })
    };

    let body = serde_json::json!({
        "source": {
            "index": args.src,
            "query": query,
        },
        "dest": {
            "index": args.dst,
        }
    });

    let path = if args.wait {
        "/_reindex?wait_for_completion=true"
    } else {
        "/_reindex"
    };

    let resp = client.post(path, &body).await?;
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}
