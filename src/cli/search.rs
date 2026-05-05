use anyhow::Result;
use clap::{Args, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// Run a search against an index
    Run(SearchArgs),
    /// Scroll through all results of a query
    Scroll(ScrollArgs),
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub index: String,

    /// Simple query string (maps to query_string query)
    #[arg(long, short = 'q')]
    pub query: Option<String>,

    /// Full DSL query body (file path or - for stdin)
    #[arg(long)]
    pub dsl: Option<String>,

    /// Aggregations body (file path or - for stdin)
    #[arg(long)]
    pub agg: Option<String>,

    #[arg(long, default_value = "10")]
    pub size: u64,

    #[arg(long, default_value = "0")]
    pub from: u64,

    /// Sort field, e.g. "timestamp:desc"
    #[arg(long)]
    pub sort: Option<String>,
}

#[derive(Args, Debug)]
pub struct ScrollArgs {
    pub index: String,
    /// Full DSL query body (file path or - for stdin)
    #[arg(long)]
    pub dsl: Option<String>,
    #[arg(long, default_value = "1m")]
    pub scroll: String,
    #[arg(long, default_value = "100")]
    pub size: u64,
}

pub async fn handle(cmd: SearchCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        SearchCommands::Run(args) => run_search(args, client, cfg).await,
        SearchCommands::Scroll(args) => run_scroll(args, client, cfg).await,
    }
}

async fn run_search(args: SearchArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let mut body = if let Some(ref dsl) = args.dsl {
        stdin::read_json(dsl).await?
    } else {
        serde_json::json!({})
    };

    // Simple query string shorthand
    if let Some(q) = args.query {
        body["query"] = serde_json::json!({ "query_string": { "query": q } });
    }

    // Merge aggregations if provided
    if let Some(ref agg) = args.agg {
        let agg_body = stdin::read_json(agg).await?;
        body["aggs"] = agg_body;
    }

    body["size"] = serde_json::json!(args.size);
    body["from"] = serde_json::json!(args.from);

    if let Some(sort) = args.sort {
        let parts: Vec<&str> = sort.splitn(2, ':').collect();
        let dir = parts.get(1).copied().unwrap_or("asc");
        body["sort"] = serde_json::json!([{ parts[0]: { "order": dir } }]);
    }

    let resp = client
        .post(&format!("/{}/_search", args.index), &body)
        .await?;
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn run_scroll(args: ScrollArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let body = if let Some(ref dsl) = args.dsl {
        let mut b = stdin::read_json(dsl).await?;
        b["size"] = serde_json::json!(args.size);
        b
    } else {
        serde_json::json!({ "query": { "match_all": {} }, "size": args.size })
    };

    let path = format!("/{}/_search?scroll={}", args.index, args.scroll);
    let mut resp = client.post(&path, &body).await?;

    let mut total = 0usize;
    loop {
        let hits = resp["hits"]["hits"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if hits.is_empty() {
            break;
        }
        total += hits.len();
        cfg.output.print(&serde_json::Value::Array(hits), cfg.no_color);

        let scroll_id = resp["_scroll_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing _scroll_id"))?
            .to_string();

        resp = client
            .post(
                "/_search/scroll",
                &serde_json::json!({
                    "scroll": args.scroll,
                    "scroll_id": scroll_id,
                }),
            )
            .await?;
    }

    eprintln!("Scrolled {} documents.", total);
    Ok(())
}
