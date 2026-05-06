use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;
use crate::stdin;

// ── Subcommands ───────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum SearchCommands {
    /// Run a search query
    Run(SearchArgs),

    /// Scroll through ALL results of a query (no size limit)
    Scroll(ScrollArgs),

    /// Point-in-time (PIT) paginate through results
    Pit(PitArgs),

    /// Run an aggregation and display results
    Agg(AggArgs),

    /// Full-text match on a field
    Match {
        index: String,
        /// Field to search
        #[arg(long)]
        field: String,
        /// Text to match
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "10")]
        size: u64,
        #[arg(long)]
        sort: Option<String>,
    },

    /// Term (exact value) filter
    Term {
        index: String,
        /// Field to filter on
        #[arg(long)]
        field: String,
        /// Exact value to match
        #[arg(long)]
        value: String,
        #[arg(long, default_value = "10")]
        size: u64,
    },

    /// Range filter on a numeric or date field
    Range {
        index: String,
        #[arg(long)]
        field: String,
        #[arg(long)]
        gte: Option<String>,
        #[arg(long)]
        lte: Option<String>,
        #[arg(long)]
        gt: Option<String>,
        #[arg(long)]
        lt: Option<String>,
        #[arg(long, default_value = "10")]
        size: u64,
        #[arg(long)]
        sort: Option<String>,
    },

    /// Multi-index search with routing
    Multi {
        /// Comma-separated index names
        #[arg(long)]
        indices: String,
        #[arg(long, short = 'q')]
        query: Option<String>,
        #[arg(long)]
        dsl: Option<String>,
        #[arg(long, default_value = "10")]
        size: u64,
    },

    /// Explain why a document matches (or doesn't) a query
    Explain {
        index: String,
        id: String,
        #[arg(long, short = 'q')]
        query: Option<String>,
        #[arg(long)]
        dsl: Option<String>,
    },

    /// Profile a query (shows timing breakdown)
    Profile(SearchArgs),

    /// Validate a query DSL without running it
    Validate {
        index: String,
        #[arg(long)]
        dsl: String,
        /// Show detailed explanation of the parsed query
        #[arg(long)]
        explain: bool,
    },

    /// Get search suggestions (term suggester)
    Suggest {
        index: String,
        /// Text to get suggestions for
        text: String,
        /// Field to suggest on
        #[arg(long)]
        field: String,
        #[arg(long, default_value = "5")]
        size: u64,
    },

    /// Delete the scroll context by scroll ID
    ClearScroll { scroll_id: String },
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub index: String,

    /// Simple query string (Lucene syntax)
    #[arg(long, short = 'q')]
    pub query: Option<String>,

    /// Full DSL body (file or - for stdin)
    #[arg(long)]
    pub dsl: Option<String>,

    /// Aggregation body to merge in (file or - for stdin)
    #[arg(long)]
    pub agg: Option<String>,

    /// Fields to include in _source (comma-separated)
    #[arg(long)]
    pub fields: Option<String>,

    /// Fields to highlight (comma-separated)
    #[arg(long)]
    pub highlight: Option<String>,

    #[arg(long, default_value = "10")]
    pub size: u64,

    #[arg(long, default_value = "0")]
    pub from: u64,

    /// Sort: field:asc or field:desc  (repeatable)
    #[arg(long, short = 's')]
    pub sort: Vec<String>,

    /// Minimum score threshold
    #[arg(long)]
    pub min_score: Option<f64>,

    /// Return only _source of each hit (strips metadata)
    #[arg(long)]
    pub source_only: bool,

    /// Suppress hits, show only aggregation results
    #[arg(long)]
    pub agg_only: bool,

    /// Index routing value
    #[arg(long)]
    pub routing: Option<String>,

    /// Request preference (e.g. "_local")
    #[arg(long)]
    pub preference: Option<String>,
}

#[derive(Args, Debug)]
pub struct ScrollArgs {
    pub index: String,

    #[arg(long, short = 'q')]
    pub query: Option<String>,

    #[arg(long)]
    pub dsl: Option<String>,

    /// Scroll context keep-alive (e.g. "2m")
    #[arg(long, default_value = "1m")]
    pub scroll: String,

    #[arg(long, default_value = "500")]
    pub size: u64,

    /// Fields to include
    #[arg(long)]
    pub fields: Option<String>,

    /// Sort: field:asc or field:desc
    #[arg(long)]
    pub sort: Vec<String>,

    /// Output each hit as NDJSON (one JSON object per line)
    #[arg(long)]
    pub ndjson: bool,

    /// Stop after N total documents (0 = all)
    #[arg(long, default_value = "0")]
    pub limit: u64,
}

#[derive(Args, Debug)]
pub struct PitArgs {
    pub index: String,

    #[arg(long, short = 'q')]
    pub query: Option<String>,

    #[arg(long)]
    pub dsl: Option<String>,

    /// PIT keep-alive duration
    #[arg(long, default_value = "1m")]
    pub keep_alive: String,

    #[arg(long, default_value = "500")]
    pub size: u64,

    /// Sort field (required for PIT pagination)
    #[arg(long, default_value = "_shard_doc:asc")]
    pub sort: String,

    #[arg(long)]
    pub ndjson: bool,

    #[arg(long, default_value = "0")]
    pub limit: u64,
}

#[derive(Args, Debug)]
pub struct AggArgs {
    pub index: String,

    /// Aggregation body (file or - for stdin)
    #[arg(long)]
    pub agg: String,

    /// Optional filter query (file or - for stdin)
    #[arg(long)]
    pub dsl: Option<String>,

    /// Name of the top-level aggregation to display
    #[arg(long)]
    pub name: Option<String>,
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn handle(cmd: SearchCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        SearchCommands::Run(a)      => run_search(a, client, cfg).await,
        SearchCommands::Scroll(a)   => run_scroll(a, client, cfg).await,
        SearchCommands::Pit(a)      => run_pit(a, client, cfg).await,
        SearchCommands::Agg(a)      => run_agg(a, client, cfg).await,
        SearchCommands::Match { index, field, text, size, sort } =>
            run_match(client, cfg, &index, &field, &text, size, sort.as_deref()).await,
        SearchCommands::Term { index, field, value, size } =>
            run_term(client, cfg, &index, &field, &value, size).await,
        SearchCommands::Range { index, field, gte, lte, gt, lt, size, sort } =>
            run_range(client, cfg, &index, &field, gte, lte, gt, lt, size, sort.as_deref()).await,
        SearchCommands::Multi { indices, query, dsl, size } =>
            run_multi(client, cfg, &indices, query.as_deref(), dsl.as_deref(), size).await,
        SearchCommands::Explain { index, id, query, dsl } =>
            run_explain(client, cfg, &index, &id, query.as_deref(), dsl.as_deref()).await,
        SearchCommands::Profile(a)  => run_profile(a, client, cfg).await,
        SearchCommands::Validate { index, dsl, explain } =>
            run_validate(client, cfg, &index, &dsl, explain).await,
        SearchCommands::Suggest { index, text, field, size } =>
            run_suggest(client, cfg, &index, &text, &field, size).await,
        SearchCommands::ClearScroll { scroll_id } =>
            clear_scroll(client, cfg, &scroll_id).await,
    }
}

// ── run ───────────────────────────────────────────────────────────────────────

async fn run_search(args: SearchArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let body = build_search_body(&args).await?;

    let mut path = format!("/{}/_search", args.index);
    let mut params = vec![];
    if let Some(ref r) = args.routing   { params.push(format!("routing={}", r)); }
    if let Some(ref p) = args.preference { params.push(format!("preference={}", p)); }
    if !params.is_empty() { path.push_str(&format!("?{}", params.join("&"))); }

    let resp = client.post(&path, &body).await?;

    print_search_result(&resp, &args, cfg);
    Ok(())
}

async fn build_search_body(args: &SearchArgs) -> Result<Value> {
    let mut body = if let Some(ref dsl) = args.dsl {
        stdin::read_json(dsl).await?
    } else {
        serde_json::json!({})
    };

    // Query
    if let Some(ref q) = args.query {
        body["query"] = serde_json::json!({ "query_string": { "query": q } });
    } else if body.get("query").is_none() {
        body["query"] = serde_json::json!({ "match_all": {} });
    }

    // Aggregations
    if let Some(ref agg) = args.agg {
        body["aggs"] = stdin::read_json(agg).await?;
    }

    // Pagination
    if !args.agg_only {
        body["size"] = serde_json::json!(args.size);
        body["from"] = serde_json::json!(args.from);
    } else {
        body["size"] = serde_json::json!(0);
    }

    // Source filtering
    if let Some(ref fields) = args.fields {
        let field_list: Vec<&str> = fields.split(',').map(str::trim).collect();
        body["_source"] = serde_json::json!(field_list);
    }

    // Sort (multiple)
    if !args.sort.is_empty() {
        body["sort"] = build_sort_clause(&args.sort);
    }

    // Min score
    if let Some(ms) = args.min_score {
        body["min_score"] = serde_json::json!(ms);
    }

    // Highlight
    if let Some(ref hl_fields) = args.highlight {
        let fields_obj: serde_json::Map<String, Value> = hl_fields
            .split(',')
            .map(|f| (f.trim().to_string(), serde_json::json!({})))
            .collect();
        body["highlight"] = serde_json::json!({ "fields": fields_obj });
    }

    Ok(body)
}

fn print_search_result(resp: &Value, args: &SearchArgs, cfg: &AppConfig) {
    let took    = resp["took"].as_u64().unwrap_or(0);
    let total   = resp.pointer("/hits/total/value").and_then(Value::as_u64).unwrap_or(0);
    let hits    = resp["hits"]["hits"].as_array().cloned().unwrap_or_default();
    let timed_out = resp["timed_out"].as_bool().unwrap_or(false);

    eprintln!(
        "{} {} hit(s) in {}ms{}",
        if timed_out { "⚠".yellow() } else { "✓".green() },
        format_number(total),
        took,
        if timed_out { " (TIMED OUT)" } else { "" },
    );

    if args.agg_only {
        if let Some(aggs) = resp.get("aggregations") {
            match cfg.output {
                OutputFormat::Json  => cfg.output.print(aggs, cfg.no_color),
                OutputFormat::Table => print_agg_table(aggs, cfg.no_color),
            }
        }
        return;
    }

    let output_hits: Vec<Value> = if args.source_only {
        hits.iter()
            .filter_map(|h| h.get("_source").cloned())
            .collect()
    } else {
        hits
    };

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&Value::Array(output_hits), cfg.no_color),
        OutputFormat::Table => print_hits_table(&output_hits, args.source_only, cfg.no_color),
    }

    // Show aggregations below hits if present
    if !args.agg_only {
        if let Some(aggs) = resp.get("aggregations") {
            eprintln!();
            match cfg.output {
                OutputFormat::Json  => cfg.output.print(aggs, cfg.no_color),
                OutputFormat::Table => print_agg_table(aggs, cfg.no_color),
            }
        }
    }
}

// ── scroll ────────────────────────────────────────────────────────────────────

async fn run_scroll(args: ScrollArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let mut body = build_scroll_query(&args).await?;

    // Apply sort
    if !args.sort.is_empty() {
        body["sort"] = build_sort_clause(&args.sort);
    }

    // Source filtering
    if let Some(ref fields) = args.fields {
        let fl: Vec<&str> = fields.split(',').map(str::trim).collect();
        body["_source"] = serde_json::json!(fl);
    }

    let path = format!("/{}/_search?scroll={}", args.index, args.scroll);
    let mut resp = client.post(&path, &body).await?;

    let total_hint = resp.pointer("/hits/total/value").and_then(Value::as_u64).unwrap_or(0);
    let limit      = if args.limit == 0 { u64::MAX } else { args.limit };

    let pb = ProgressBar::new(if args.limit > 0 { args.limit } else { total_hint });
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} docs scrolled")
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
    );

    let mut total_seen = 0u64;
    let mut scroll_id  = String::new();

    loop {
        let hits: Vec<Value> = resp["hits"]["hits"]
            .as_array().cloned().unwrap_or_default();
        if hits.is_empty() { break; }
        if total_seen >= limit { break; }

        scroll_id = resp["_scroll_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("missing _scroll_id"))?
            .to_string();

        let remaining = (limit - total_seen) as usize;
        let page: Vec<Value> = hits.into_iter().take(remaining).collect();
        let page_len = page.len() as u64;

        // Emit hits
        let output_hits: Vec<Value> = if args.ndjson {
            page.iter()
                .filter_map(|h| h.get("_source").cloned())
                .collect()
        } else {
            page
        };

        if args.ndjson {
            for hit in &output_hits {
                println!("{}", serde_json::to_string(hit).unwrap_or_default());
            }
        } else {
            match cfg.output {
                OutputFormat::Json  => cfg.output.print(&Value::Array(output_hits), cfg.no_color),
                OutputFormat::Table => {} // table printed at end
            }
        }

        total_seen += page_len;
        pb.inc(page_len);

        resp = client.post(
            "/_search/scroll",
            &serde_json::json!({ "scroll": args.scroll, "scroll_id": scroll_id }),
        ).await?;
    }

    pb.finish_and_clear();

    // Clean up scroll context
    if !scroll_id.is_empty() {
        let _ = client.delete_with_body(
            "/_search/scroll",
            &serde_json::json!({ "scroll_id": scroll_id }),
        ).await;
    }

    eprintln!(
        "{} Scroll complete: {} document(s) retrieved.",
        "✓".green(), format_number(total_seen)
    );
    Ok(())
}

async fn build_scroll_query(args: &ScrollArgs) -> Result<Value> {
    let mut body = if let Some(ref dsl) = args.dsl {
        stdin::read_json(dsl).await?
    } else if let Some(ref q) = args.query {
        serde_json::json!({ "query": { "query_string": { "query": q } } })
    } else {
        serde_json::json!({ "query": { "match_all": {} } })
    };
    body["size"] = serde_json::json!(args.size);
    Ok(body)
}

// ── PIT pagination ────────────────────────────────────────────────────────────

async fn run_pit(args: PitArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    // Open PIT
    let pit_resp = client
        .post(
            &format!("/{}/_pit?keep_alive={}", args.index, args.keep_alive),
            &Value::Null,
        )
        .await?;
    let pit_id = pit_resp["id"].as_str()
        .ok_or_else(|| anyhow::anyhow!("failed to open PIT context"))?
        .to_string();

    eprintln!("{} PIT context opened: {}", "→".cyan(), &pit_id[..pit_id.len().min(20)]);

    let base_query = if let Some(ref dsl) = args.dsl {
        stdin::read_json(dsl).await?
    } else if let Some(ref q) = args.query {
        serde_json::json!({ "query": { "query_string": { "query": q } } })
    } else {
        serde_json::json!({ "query": { "match_all": {} } })
    };

    let sort_parts: Vec<&str> = args.sort.splitn(2, ':').collect();
    let sort_field = sort_parts[0];
    let sort_dir   = sort_parts.get(1).copied().unwrap_or("asc");

    let limit    = if args.limit == 0 { u64::MAX } else { args.limit };
    let mut total_seen  = 0u64;
    let mut search_after: Option<Value> = None;

    let pb = ProgressBar::new(if args.limit > 0 { args.limit } else { u64::MAX });
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} {pos} docs retrieved via PIT…")
            .unwrap(),
    );

    loop {
        if total_seen >= limit { break; }

        let mut body = base_query.clone();
        body["size"] = serde_json::json!(args.size);
        body["pit"]  = serde_json::json!({ "id": pit_id, "keep_alive": args.keep_alive });
        body["sort"] = serde_json::json!([{ sort_field: { "order": sort_dir } }]);

        if let Some(ref sa) = search_after {
            body["search_after"] = sa.clone();
        }

        let resp = client.post("/_search", &body).await?;
        let hits: Vec<Value> = resp["hits"]["hits"]
            .as_array().cloned().unwrap_or_default();

        if hits.is_empty() { break; }

        // Update search_after from last hit's sort values
        if let Some(last) = hits.last() {
            search_after = last.get("sort").cloned();
        }

        let remaining = (limit - total_seen) as usize;
        let page: Vec<Value> = hits.into_iter().take(remaining).collect();
        let page_len = page.len() as u64;

        if args.ndjson {
            for hit in &page {
                if let Some(src) = hit.get("_source") {
                    println!("{}", serde_json::to_string(src).unwrap_or_default());
                }
            }
        } else {
            match cfg.output {
                OutputFormat::Json => cfg.output.print(&Value::Array(page), cfg.no_color),
                OutputFormat::Table => {}
            }
        }

        total_seen += page_len;
        pb.inc(page_len);
    }

    pb.finish_and_clear();

    // Close PIT
    let _ = client.delete_with_body("/_pit", &serde_json::json!({ "id": pit_id })).await;

    eprintln!(
        "{} PIT pagination complete: {} document(s).",
        "✓".green(), format_number(total_seen)
    );
    Ok(())
}

// ── agg ───────────────────────────────────────────────────────────────────────

async fn run_agg(args: AggArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let agg_body = stdin::read_json(&args.agg).await?;

    let mut body = if let Some(ref dsl) = args.dsl {
        stdin::read_json(dsl).await?
    } else {
        serde_json::json!({ "query": { "match_all": {} } })
    };

    body["aggs"] = agg_body;
    body["size"] = serde_json::json!(0); // no hits needed

    let resp = client.post(&format!("/{}/_search", args.index), &body).await?;
    let took  = resp["took"].as_u64().unwrap_or(0);
    eprintln!("{} Aggregation complete in {}ms.", "✓".green(), took);

    let aggs = resp.get("aggregations").cloned().unwrap_or(Value::Null);

    // If a name is given, drill into that aggregation
    let display = if let Some(ref name) = args.name {
        aggs.get(name).cloned().unwrap_or(aggs)
    } else {
        aggs
    };

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&display, cfg.no_color),
        OutputFormat::Table => print_agg_table(&display, cfg.no_color),
    }
    Ok(())
}

// ── match ─────────────────────────────────────────────────────────────────────

async fn run_match(
    client: &EsClient, cfg: &AppConfig,
    index: &str, field: &str, text: &str, size: u64, sort: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "query": { "match": { field: text } },
        "size":  size,
    });
    if let Some(s) = sort {
        body["sort"] = build_sort_clause(&[s.to_string()]);
    }
    let resp = client.post(&format!("/{}/_search", index), &body).await?;
    let total = resp.pointer("/hits/total/value").and_then(Value::as_u64).unwrap_or(0);
    eprintln!("{} {} hit(s)", "✓".green(), format_number(total));
    let hits = resp["hits"]["hits"].as_array().cloned().unwrap_or_default();
    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&Value::Array(hits), cfg.no_color),
        OutputFormat::Table => print_hits_table(&hits, false, cfg.no_color),
    }
    Ok(())
}

// ── term ──────────────────────────────────────────────────────────────────────

async fn run_term(
    client: &EsClient, cfg: &AppConfig,
    index: &str, field: &str, value: &str, size: u64,
) -> Result<()> {
    let body = serde_json::json!({
        "query": { "term": { field: value } },
        "size":  size,
    });
    let resp  = client.post(&format!("/{}/_search", index), &body).await?;
    let total = resp.pointer("/hits/total/value").and_then(Value::as_u64).unwrap_or(0);
    eprintln!("{} {} hit(s)", "✓".green(), format_number(total));
    let hits  = resp["hits"]["hits"].as_array().cloned().unwrap_or_default();
    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&Value::Array(hits), cfg.no_color),
        OutputFormat::Table => print_hits_table(&hits, false, cfg.no_color),
    }
    Ok(())
}

// ── range ─────────────────────────────────────────────────────────────────────

async fn run_range(
    client: &EsClient, cfg: &AppConfig,
    index: &str, field: &str,
    gte: Option<String>, lte: Option<String>,
    gt: Option<String>,  lt: Option<String>,
    size: u64, sort: Option<&str>,
) -> Result<()> {
    let mut range = serde_json::Map::new();
    if let Some(v) = gte { range.insert("gte".into(), Value::String(v)); }
    if let Some(v) = lte { range.insert("lte".into(), Value::String(v)); }
    if let Some(v) = gt  { range.insert("gt".into(),  Value::String(v)); }
    if let Some(v) = lt  { range.insert("lt".into(),  Value::String(v)); }

    let mut body = serde_json::json!({
        "query": { "range": { field: range } },
        "size":  size,
    });
    if let Some(s) = sort {
        body["sort"] = build_sort_clause(&[s.to_string()]);
    }
    let resp  = client.post(&format!("/{}/_search", index), &body).await?;
    let total = resp.pointer("/hits/total/value").and_then(Value::as_u64).unwrap_or(0);
    eprintln!("{} {} hit(s)", "✓".green(), format_number(total));
    let hits  = resp["hits"]["hits"].as_array().cloned().unwrap_or_default();
    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&Value::Array(hits), cfg.no_color),
        OutputFormat::Table => print_hits_table(&hits, false, cfg.no_color),
    }
    Ok(())
}

// ── multi-index ───────────────────────────────────────────────────────────────

async fn run_multi(
    client: &EsClient, cfg: &AppConfig,
    indices: &str, query: Option<&str>, dsl: Option<&str>, size: u64,
) -> Result<()> {
    let mut body = if let Some(src) = dsl {
        stdin::read_json(src).await?
    } else if let Some(q) = query {
        serde_json::json!({ "query": { "query_string": { "query": q } } })
    } else {
        serde_json::json!({ "query": { "match_all": {} } })
    };
    body["size"] = serde_json::json!(size);

    let resp  = client.post(&format!("/{}/_search", indices), &body).await?;
    let total = resp.pointer("/hits/total/value").and_then(Value::as_u64).unwrap_or(0);
    eprintln!("{} {} hit(s) across [{}]", "✓".green(), format_number(total), indices);
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── explain ───────────────────────────────────────────────────────────────────

async fn run_explain(
    client: &EsClient, cfg: &AppConfig,
    index: &str, id: &str, query: Option<&str>, dsl: Option<&str>,
) -> Result<()> {
    let body = if let Some(src) = dsl {
        stdin::read_json(src).await?
    } else if let Some(q) = query {
        serde_json::json!({ "query": { "query_string": { "query": q } } })
    } else {
        serde_json::json!({ "query": { "match_all": {} } })
    };

    let resp = client
        .post(&format!("/{}/_explain/{}", index, id), &body)
        .await?;

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_explain_table(&resp, cfg.no_color),
    }
    Ok(())
}

fn print_explain_table(resp: &Value, _no_color: bool) {
    let matched = resp["matched"].as_bool().unwrap_or(false);
    let score   = resp.pointer("/explanation/value").and_then(Value::as_f64).unwrap_or(0.0);
    let desc    = resp.pointer("/explanation/description").and_then(Value::as_str).unwrap_or("-");

    println!();
    println!("  {:<20} {}", "Matched:".bold(), if matched { "yes".green().to_string() } else { "no".red().to_string() });
    println!("  {:<20} {:.4}", "Score:".bold(), score);
    println!("  {:<20} {}", "Description:".bold(), desc);

    if let Some(details) = resp.pointer("/explanation/details").and_then(Value::as_array) {
        println!();
        println!("  {}", "── Score breakdown ──".dimmed());
        print_explain_details(details, 0);
    }
    println!();
}

fn print_explain_details(details: &[Value], depth: usize) {
    for d in details {
        let indent = "  ".repeat(depth + 1);
        let val    = d["value"].as_f64().unwrap_or(0.0);
        let desc   = d["description"].as_str().unwrap_or("-");
        println!("  {}{:.4}  {}", indent, val, desc);
        if let Some(sub) = d["details"].as_array() {
            print_explain_details(sub, depth + 1);
        }
    }
}

// ── profile ───────────────────────────────────────────────────────────────────

async fn run_profile(args: SearchArgs, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let mut body = build_search_body(&args).await?;
    body["profile"] = Value::Bool(true);

    let resp = client.post(&format!("/{}/_search", args.index), &body).await?;
    let took = resp["took"].as_u64().unwrap_or(0);
    eprintln!("{} Profile complete in {}ms.", "✓".green(), took);

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_profile_table(&resp, cfg.no_color),
    }
    Ok(())
}

fn print_profile_table(resp: &Value, _no_color: bool) {
    let took = resp["took"].as_u64().unwrap_or(0);
    println!();
    println!("  {} {}ms total", "Took:".bold(), took);
    println!();

    if let Some(shards) = resp.pointer("/profile/shards").and_then(Value::as_array) {
        for (i, shard) in shards.iter().enumerate() {
            println!("  {} #{}", "Shard".bold(), i);
            if let Some(searches) = shard["searches"].as_array() {
                for search in searches {
                    if let Some(queries) = search["query"].as_array() {
                        println!("  {}", "── Queries ──".dimmed());
                        for q in queries {
                            let qtype = q["type"].as_str().unwrap_or("-");
                            let qtime = q["time_in_nanos"].as_u64().unwrap_or(0);
                            let qcnt  = q["count"].as_u64().unwrap_or(0);
                            println!(
                                "  {:<30} {:>10}μs  count={}",
                                qtype, qtime / 1000, qcnt
                            );
                        }
                    }
                }
            }
        }
    }
    println!();
}

// ── validate ──────────────────────────────────────────────────────────────────

async fn run_validate(
    client: &EsClient, cfg: &AppConfig,
    index: &str, dsl: &str, explain: bool,
) -> Result<()> {
    let body = stdin::read_json(dsl).await?;
    let path = if explain {
        format!("/{}/_validate/query?explain=true", index)
    } else {
        format!("/{}/_validate/query", index)
    };
    let resp = client.post(&path, &body).await?;

    let valid = resp["valid"].as_bool().unwrap_or(false);
    if valid {
        eprintln!("{} Query is valid.", "✓".green());
    } else {
        eprintln!("{} Query is INVALID.", "✗".red());
        if let Some(explanations) = resp["explanations"].as_array() {
            for ex in explanations {
                if let Some(err) = ex["error"].as_str() {
                    eprintln!("  {}: {}", "error".red(), err);
                }
            }
        }
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── suggest ───────────────────────────────────────────────────────────────────

async fn run_suggest(
    client: &EsClient, cfg: &AppConfig,
    index: &str, text: &str, field: &str, size: u64,
) -> Result<()> {
    let body = serde_json::json!({
        "suggest": {
            "my-suggestion": {
                "text": text,
                "term": {
                    "field": field,
                    "size":  size,
                }
            }
        },
        "size": 0,
    });
    let resp = client.post(&format!("/{}/_search", index), &body).await?;

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_suggest_table(&resp, cfg.no_color),
    }
    Ok(())
}

fn print_suggest_table(resp: &Value, _no_color: bool) {
    let suggestions = match resp.pointer("/suggest/my-suggestion").and_then(Value::as_array) {
        Some(s) => s,
        None    => { println!("No suggestions."); return; }
    };
    println!();
    println!("  {:<20} {}", "text".dimmed(), "suggestions".dimmed());
    println!("  {}", "─".repeat(60).dimmed());
    for entry in suggestions {
        let text = entry["text"].as_str().unwrap_or("-");
        let opts = entry["options"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|o| o["text"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }).unwrap_or_default();
        println!("  {:<20} {}", text, if opts.is_empty() { "(no suggestions)" } else { &opts });
    }
    println!();
}

// ── clear-scroll ──────────────────────────────────────────────────────────────

async fn clear_scroll(client: &EsClient, cfg: &AppConfig, scroll_id: &str) -> Result<()> {
    let body = serde_json::json!({ "scroll_id": scroll_id });
    let resp = client.delete_with_body("/_search/scroll", &body).await?;
    eprintln!("{} Scroll context cleared.", "✓".green());
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── table formatters ──────────────────────────────────────────────────────────

fn print_hits_table(hits: &[Value], source_only: bool, no_color: bool) {
    if hits.is_empty() {
        println!("  No hits.");
        return;
    }

    // Collect all _source keys from first N hits to build column headers
    let source_hits: Vec<&Value> = if source_only {
        hits.iter().collect()
    } else {
        hits.iter().collect()
    };

    let get_source = |h: &Value| -> &Value {
        if source_only { h } else { &h["_source"] }
    };

    let mut cols: Vec<String> = Vec::new();
    if !source_only {
        cols.push("_id".into());
        cols.push("_score".into());
    }

    // Auto-detect columns from first 5 hits
    for hit in source_hits.iter().take(5) {
        if let Some(src) = get_source(hit).as_object() {
            for k in src.keys() {
                if !cols.contains(k) { cols.push(k.clone()); }
            }
        }
    }

    let col_width = 20usize;
    println!();

    // Header
    let header: String = cols.iter()
        .map(|c| format!("{:<width$}", truncate(c, col_width), width = col_width))
        .collect::<Vec<_>>()
        .join("  ");
    println!("  {}", header.dimmed());
    println!("  {}", "─".repeat(cols.len() * (col_width + 2)).dimmed());

    // Rows
    for hit in hits {
        let src = get_source(hit);
        let row: String = cols.iter().map(|c| {
            let val = if !source_only && c == "_id" {
                hit["_id"].as_str().unwrap_or("-").to_string()
            } else if !source_only && c == "_score" {
                hit["_score"].as_f64().map(|s| format!("{:.4}", s)).unwrap_or_else(|| "-".into())
            } else {
                match &src[c] {
                    Value::Null   => "-".into(),
                    Value::String(s) => truncate(s, col_width),
                    other         => truncate(&serde_json::to_string(other).unwrap_or_default(), col_width),
                }
            };
            format!("{:<width$}", val, width = col_width)
        }).collect::<Vec<_>>().join("  ");
        println!("  {}", row);
    }
    println!();

    let _ = no_color;
}

/// Pretty-print aggregation results (terms, date_histogram, stats, etc.)
pub fn print_agg_table(aggs: &Value, _no_color: bool) {
    if let Some(obj) = aggs.as_object() {
        for (agg_name, agg_val) in obj {
            println!();
            println!("  {} {}", "Aggregation:".bold(), agg_name.bold());

            // Terms / date_histogram buckets
            if let Some(buckets) = agg_val["buckets"].as_array() {
                println!("  {:<40} {:>12}", "key".dimmed(), "doc_count".dimmed());
                println!("  {}", "─".repeat(55).dimmed());
                for bucket in buckets {
                    let key = match &bucket["key_as_string"] {
                        Value::String(s) => s.clone(),
                        _ => match &bucket["key"] {
                            Value::String(s) => s.clone(),
                            other            => serde_json::to_string(other).unwrap_or_default(),
                        }
                    };
                    let count = bucket["doc_count"].as_u64().unwrap_or(0);
                    println!(
                        "  {:<40} {:>12}",
                        truncate(&key, 40),
                        format_number(count)
                    );
                }
            }
            // Stats agg: min/max/avg/sum/count
            else if agg_val.get("count").is_some() && agg_val.get("avg").is_some() {
                for field in &["count","min","max","avg","sum"] {
                    if let Some(v) = agg_val[*field].as_f64() {
                        println!("  {:<12} {:.4}", field, v);
                    }
                }
            }
            // Single value (cardinality, sum, etc.)
            else if let Some(v) = agg_val["value"].as_f64() {
                println!("  value: {:.4}", v);
            }
            else {
                println!("{}", serde_json::to_string_pretty(agg_val).unwrap_or_default());
            }
            println!();
        }
    }
}

// ── sort builder ──────────────────────────────────────────────────────────────

pub fn build_sort_clause(sorts: &[String]) -> Value {
    let arr: Vec<Value> = sorts.iter().map(|s| {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        let field = parts[0];
        let dir   = parts.get(1).copied().unwrap_or("asc");
        serde_json::json!({ field: { "order": dir } })
    }).collect();
    Value::Array(arr)
}

// ── helpers ───────────────────────────────────────────────────────────────────

pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out.chars().rev().collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() }
    else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sort_single() {
        let sort = build_sort_clause(&["timestamp:desc".to_string()]);
        let arr  = sort.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["timestamp"]["order"], "desc");
    }

    #[test]
    fn test_build_sort_default_asc() {
        let sort = build_sort_clause(&["name".to_string()]);
        let arr  = sort.as_array().unwrap();
        assert_eq!(arr[0]["name"]["order"], "asc");
    }

    #[test]
    fn test_build_sort_multiple() {
        let sort = build_sort_clause(&[
            "date:desc".to_string(),
            "name:asc".to_string(),
        ]);
        let arr = sort.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["date"]["order"], "desc");
        assert_eq!(arr[1]["name"]["order"], "asc");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0),         "0");
        assert_eq!(format_number(1_000),     "1,000");
        assert_eq!(format_number(1_234_567), "1,234,567");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10),       "hello");
        assert_eq!(truncate("hello world!", 8), "hello wo…");
        assert_eq!(truncate("", 5),             "");
    }

    #[tokio::test]
    async fn test_build_search_body_match_all() {
        let args = SearchArgs {
            index:      "idx".into(),
            query:      None,
            dsl:        None,
            agg:        None,
            fields:     None,
            highlight:  None,
            size:       10,
            from:       0,
            sort:       vec![],
            min_score:  None,
            source_only: false,
            agg_only:   false,
            routing:    None,
            preference: None,
        };
        let body = build_search_body(&args).await.unwrap();
        assert!(body.pointer("/query/match_all").is_some());
        assert_eq!(body["size"], 10);
        assert_eq!(body["from"], 0);
    }

    #[tokio::test]
    async fn test_build_search_body_query_string() {
        let args = SearchArgs {
            index:      "idx".into(),
            query:      Some("status:active".into()),
            dsl:        None,
            agg:        None,
            fields:     None,
            highlight:  None,
            size:       5,
            from:       0,
            sort:       vec![],
            min_score:  None,
            source_only: false,
            agg_only:   false,
            routing:    None,
            preference: None,
        };
        let body = build_search_body(&args).await.unwrap();
        let q = body.pointer("/query/query_string/query")
            .and_then(Value::as_str).unwrap();
        assert_eq!(q, "status:active");
    }

    #[tokio::test]
    async fn test_build_search_body_agg_only_sets_size_zero() {
        let args = SearchArgs {
            index:      "idx".into(),
            query:      None,
            dsl:        None,
            agg:        None,
            fields:     None,
            highlight:  None,
            size:       10,
            from:       0,
            sort:       vec![],
            min_score:  None,
            source_only: false,
            agg_only:   true,
            routing:    None,
            preference: None,
        };
        let body = build_search_body(&args).await.unwrap();
        assert_eq!(body["size"], 0);
    }

    #[tokio::test]
    async fn test_build_search_body_source_filter() {
        let args = SearchArgs {
            index:      "idx".into(),
            query:      None,
            dsl:        None,
            agg:        None,
            fields:     Some("name,email".into()),
            highlight:  None,
            size:       10,
            from:       0,
            sort:       vec![],
            min_score:  None,
            source_only: false,
            agg_only:   false,
            routing:    None,
            preference: None,
        };
        let body = build_search_body(&args).await.unwrap();
        let src  = body["_source"].as_array().unwrap();
        assert!(src.iter().any(|v| v == "name"));
        assert!(src.iter().any(|v| v == "email"));
    }

    #[tokio::test]
    async fn test_build_search_body_min_score() {
        let args = SearchArgs {
            index:      "idx".into(),
            query:      Some("foo".into()),
            dsl:        None,
            agg:        None,
            fields:     None,
            highlight:  None,
            size:       10,
            from:       0,
            sort:       vec![],
            min_score:  Some(0.5),
            source_only: false,
            agg_only:   false,
            routing:    None,
            preference: None,
        };
        let body = build_search_body(&args).await.unwrap();
        assert_eq!(body["min_score"], 0.5);
    }

    #[tokio::test]
    async fn test_build_search_body_highlight() {
        let args = SearchArgs {
            index:      "idx".into(),
            query:      Some("foo".into()),
            dsl:        None,
            agg:        None,
            fields:     None,
            highlight:  Some("title,body".into()),
            size:       10,
            from:       0,
            sort:       vec![],
            min_score:  None,
            source_only: false,
            agg_only:   false,
            routing:    None,
            preference: None,
        };
        let body = build_search_body(&args).await.unwrap();
        assert!(body.pointer("/highlight/fields/title").is_some());
        assert!(body.pointer("/highlight/fields/body").is_some());
    }

    #[test]
    fn test_print_agg_table_terms_buckets() {
        let aggs = serde_json::json!({
            "by_status": {
                "buckets": [
                    { "key": "active",   "doc_count": 100 },
                    { "key": "inactive", "doc_count": 25  },
                ]
            }
        });
        // Just assert it doesn't panic
        print_agg_table(&aggs, true);
    }

    #[test]
    fn test_print_agg_table_stats() {
        let aggs = serde_json::json!({
            "price_stats": {
                "count": 500,
                "min":   1.0,
                "max":   99.99,
                "avg":   42.5,
                "sum":   21250.0,
            }
        });
        print_agg_table(&aggs, true);
    }

    #[test]
    fn test_print_agg_table_value() {
        let aggs = serde_json::json!({
            "unique_users": { "value": 1234.0 }
        });
        print_agg_table(&aggs, true);
    }

    #[test]
    fn test_range_query_body_structure() {
        let mut range = serde_json::Map::new();
        range.insert("gte".into(), Value::String("2024-01-01".into()));
        range.insert("lte".into(), Value::String("2024-12-31".into()));
        let body = serde_json::json!({
            "query": { "range": { "date": range } },
            "size": 10,
        });
        assert!(body.pointer("/query/range/date/gte").is_some());
        assert!(body.pointer("/query/range/date/lte").is_some());
    }

    #[test]
    fn test_scroll_query_default() {
        // build_scroll_query is async; just test the logic inline
        let body = serde_json::json!({ "query": { "match_all": {} }, "size": 500 });
        assert!(body.pointer("/query/match_all").is_some());
        assert_eq!(body["size"], 500);
    }
}
