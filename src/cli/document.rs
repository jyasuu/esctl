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
pub enum DocCommands {
    /// Get a document by ID
    Get {
        index: String,
        id: String,
        /// Return only _source (no metadata)
        #[arg(long)]
        source_only: bool,
        /// Comma-separated list of fields to include
        #[arg(long)]
        fields: Option<String>,
    },

    /// Check if a document exists (exit 0 = yes, 1 = no)
    Exists { index: String, id: String },

    /// Index (create/replace) a document
    Index(DocIndexArgs),

    /// Create a document — fails if ID already exists
    Create(DocCreateArgs),

    /// Update a document (partial doc or script)
    Update(DocUpdateArgs),

    /// Delete a document by ID
    Delete {
        index: String,
        id: String,
        /// Only delete if sequence number matches
        #[arg(long)]
        if_seq_no: Option<u64>,
        /// Only delete if primary term matches
        #[arg(long)]
        if_primary_term: Option<u64>,
    },

    /// Get multiple documents by ID (mget)
    Mget {
        index: String,
        /// Document IDs
        #[arg(required = true)]
        ids: Vec<String>,
        /// Return only _source
        #[arg(long)]
        source_only: bool,
    },

    /// Count documents matching a query
    Count {
        index: String,
        /// Simple query string
        #[arg(long, short = 'q')]
        query: Option<String>,
        /// Full DSL query (file or - for stdin)
        #[arg(long)]
        dsl: Option<String>,
    },

    /// Delete documents matching a query
    DeleteByQuery {
        index: String,
        /// Simple query string
        #[arg(long, short = 'q')]
        query: Option<String>,
        /// Full DSL query (file or - for stdin)
        #[arg(long)]
        dsl: Option<String>,
        /// Wait for completion before returning
        #[arg(long)]
        wait: bool,
        /// Slice for parallelism
        #[arg(long, default_value = "1")]
        slices: u32,
    },

    /// Update documents matching a query using a Painless script
    UpdateByQuery {
        index: String,
        /// Painless script source, e.g. "ctx._source.status = 'active'"
        #[arg(long)]
        script: Option<String>,
        /// Script + query body (file or - for stdin)
        #[arg(long)]
        dsl: Option<String>,
        #[arg(long)]
        wait: bool,
        #[arg(long, default_value = "1")]
        slices: u32,
    },

    /// Bulk index from NDJSON file or stdin
    Bulk(BulkArgs),

    /// Write a single document from stdin or file (alias for index --body -)
    Write {
        index: String,
        #[arg(long)]
        id: Option<String>,
        /// JSON body (file or - for stdin, default: stdin)
        #[arg(long)]
        body: Option<String>,
        /// Fail if the document already exists
        #[arg(long)]
        create: bool,
    },

    /// Pipe many JSON objects into an index (JSON array or NDJSON)
    Ingest {
        index: String,
        /// Source (file or - for stdin)
        #[arg(long)]
        body: Option<String>,
        /// Batch size per _bulk request
        #[arg(long, default_value = "500")]
        batch: usize,
        /// ID field to extract from each document (optional)
        #[arg(long)]
        id_field: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct DocIndexArgs {
    pub index: String,
    #[arg(long)]
    pub id: Option<String>,
    /// JSON body (file or - for stdin)
    #[arg(long)]
    pub body: String,
    /// Pipeline to run the document through
    #[arg(long)]
    pub pipeline: Option<String>,
    /// Routing value
    #[arg(long)]
    pub routing: Option<String>,
}

#[derive(Args, Debug)]
pub struct DocCreateArgs {
    pub index: String,
    pub id: String,
    #[arg(long)]
    pub body: String,
}

#[derive(Args, Debug)]
pub struct DocUpdateArgs {
    pub index: String,
    pub id: String,
    /// Partial document body OR {"script":{...}} (file or - for stdin)
    #[arg(long)]
    pub body: String,
    /// Treat body as a Painless script source string (not JSON)
    #[arg(long)]
    pub script: Option<String>,
    /// Number of retries on version conflict
    #[arg(long, default_value = "3")]
    pub retry_on_conflict: u32,
    /// Upsert body — create doc if not found (file or - for stdin)
    #[arg(long)]
    pub upsert: Option<String>,
}

#[derive(Args, Debug)]
pub struct BulkArgs {
    pub index: String,
    /// NDJSON file or - for stdin
    pub source: Option<String>,
    /// Batch size (lines per request); 0 = single request
    #[arg(long, default_value = "0")]
    pub batch: usize,
    /// Show per-item errors (up to N)
    #[arg(long, default_value = "10")]
    pub show_errors: usize,
    /// Pipeline to apply
    #[arg(long)]
    pub pipeline: Option<String>,
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn handle(cmd: DocCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        DocCommands::Get { index, id, source_only, fields } =>
            handle_get(client, cfg, &index, &id, source_only, fields.as_deref()).await,
        DocCommands::Exists { index, id } =>
            handle_exists(client, &index, &id).await,
        DocCommands::Index(a) =>
            handle_index(client, cfg, a).await,
        DocCommands::Create(a) =>
            handle_create(client, cfg, a).await,
        DocCommands::Update(a) =>
            handle_update(client, cfg, a).await,
        DocCommands::Delete { index, id, if_seq_no, if_primary_term } =>
            handle_delete(client, cfg, &index, &id, if_seq_no, if_primary_term).await,
        DocCommands::Mget { index, ids, source_only } =>
            handle_mget(client, cfg, &index, &ids, source_only).await,
        DocCommands::Count { index, query, dsl } =>
            handle_count(client, cfg, &index, query.as_deref(), dsl.as_deref()).await,
        DocCommands::DeleteByQuery { index, query, dsl, wait, slices } =>
            handle_delete_by_query(client, cfg, &index, query.as_deref(), dsl.as_deref(), wait, slices).await,
        DocCommands::UpdateByQuery { index, script, dsl, wait, slices } =>
            handle_update_by_query(client, cfg, &index, script.as_deref(), dsl.as_deref(), wait, slices).await,
        DocCommands::Bulk(a) =>
            handle_bulk(client, cfg, a).await,
        DocCommands::Write { index, id, body, create } =>
            handle_write(client, cfg, &index, id.as_deref(), body.as_deref(), create).await,
        DocCommands::Ingest { index, body, batch, id_field } =>
            handle_ingest(client, cfg, &index, body.as_deref(), batch, id_field.as_deref()).await,
    }
}

// ── get ───────────────────────────────────────────────────────────────────────

async fn handle_get(
    client: &EsClient, cfg: &AppConfig,
    index: &str, id: &str, source_only: bool, fields: Option<&str>,
) -> Result<()> {
    let path = if source_only {
        format!("/{}/_source/{}", index, id)
    } else if let Some(f) = fields {
        format!("/{}/_doc/{}?_source_includes={}", index, id, f)
    } else {
        format!("/{}/_doc/{}", index, id)
    };

    let resp = client.get(&path).await?;

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => {
            if source_only {
                cfg.output.print(&resp, cfg.no_color);
            } else {
                print_doc_table(&resp, cfg.no_color);
            }
        }
    }
    Ok(())
}

fn print_doc_table(doc: &Value, _no_color: bool) {
    println!();
    println!("  {:<20} {}", "Field".dimmed(), "Value".dimmed());
    println!("  {}", "─".repeat(60).dimmed());
    println!("  {:<20} {}", "_index",  doc["_index"].as_str().unwrap_or("-"));
    println!("  {:<20} {}", "_id",     doc["_id"].as_str().unwrap_or("-"));
    println!("  {:<20} {}", "_version",doc["_version"].as_u64().unwrap_or(0));
    println!("  {:<20} {}", "_seq_no", doc["_seq_no"].as_u64().unwrap_or(0));
    println!();
    println!("  {}", "── _source ──".dimmed());

    if let Some(source) = doc["_source"].as_object() {
        let mut keys: Vec<&String> = source.keys().collect();
        keys.sort();
        for k in keys {
            let v = format_cell_value(&source[k]);
            println!("  {:<20} {}", k, v);
        }
    }
    println!();
}

// ── exists ────────────────────────────────────────────────────────────────────

async fn handle_exists(client: &EsClient, index: &str, id: &str) -> Result<()> {
    // HEAD request — ES returns 200 or 404
    // We model this as GET and check for 404 error
    match client.get(&format!("/{}/_doc/{}?_source=false", index, id)).await {
        Ok(_) => {
            println!("exists");
            std::process::exit(0);
        }
        Err(e) if e.to_string().contains("404") => {
            println!("not found");
            std::process::exit(1);
        }
        Err(e) => return Err(e),
    }
}

// ── index ─────────────────────────────────────────────────────────────────────

async fn handle_index(client: &EsClient, cfg: &AppConfig, args: DocIndexArgs) -> Result<()> {
    let body = stdin::read_json(&args.body).await?;

    let mut path = if let Some(ref id) = args.id {
        format!("/{}/_doc/{}", args.index, id)
    } else {
        format!("/{}/_doc", args.index)
    };

    let mut params = Vec::new();
    if let Some(ref p) = args.pipeline { params.push(format!("pipeline={}", p)); }
    if let Some(ref r) = args.routing  { params.push(format!("routing={}", r)); }
    if !params.is_empty() { path.push_str(&format!("?{}", params.join("&"))); }

    let resp = if args.id.is_some() {
        client.put(&path, &body).await?
    } else {
        client.post(&path, &body).await?
    };

    print_write_result(&resp, cfg);
    Ok(())
}

// ── create ────────────────────────────────────────────────────────────────────

async fn handle_create(client: &EsClient, cfg: &AppConfig, args: DocCreateArgs) -> Result<()> {
    let body = stdin::read_json(&args.body).await?;
    // _create endpoint fails with 409 if doc already exists
    let path = format!("/{}/_create/{}", args.index, args.id);
    let resp = client.put(&path, &body).await?;
    print_write_result(&resp, cfg);
    Ok(())
}

// ── update ────────────────────────────────────────────────────────────────────

async fn handle_update(client: &EsClient, cfg: &AppConfig, args: DocUpdateArgs) -> Result<()> {
    let mut payload = if let Some(ref script_src) = args.script {
        // Inline script shorthand
        serde_json::json!({
            "script": {
                "source": script_src,
                "lang":   "painless",
            }
        })
    } else {
        // Body is either {"doc":{...}} or {"script":{...}} already
        let raw = stdin::read_json(&args.body).await?;
        // If the body doesn't have "doc" or "script" at top level, wrap as partial doc
        if raw.get("doc").is_none() && raw.get("script").is_none() {
            serde_json::json!({ "doc": raw })
        } else {
            raw
        }
    };

    // Upsert
    if let Some(ref upsert_src) = args.upsert {
        let upsert_doc = stdin::read_json(upsert_src).await?;
        payload["upsert"] = upsert_doc;
    }

    let path = format!(
        "/{}/_update/{}?retry_on_conflict={}",
        args.index, args.id, args.retry_on_conflict
    );
    let resp = client.post(&path, &payload).await?;
    print_write_result(&resp, cfg);
    Ok(())
}

// ── delete ────────────────────────────────────────────────────────────────────

async fn handle_delete(
    client: &EsClient, cfg: &AppConfig,
    index: &str, id: &str,
    if_seq_no: Option<u64>, if_primary_term: Option<u64>,
) -> Result<()> {
    let mut path = format!("/{}/_doc/{}", index, id);
    let mut params = Vec::new();
    if let Some(s) = if_seq_no      { params.push(format!("if_seq_no={}", s)); }
    if let Some(p) = if_primary_term { params.push(format!("if_primary_term={}", p)); }
    if !params.is_empty() { path.push_str(&format!("?{}", params.join("&"))); }

    let resp = client.delete(&path).await?;

    let result = resp["result"].as_str().unwrap_or("-");
    match result {
        "deleted" => eprintln!("{} Deleted {}/{}", "✓".green(), index, id),
        "not_found" => eprintln!("{} Document not found: {}/{}", "⚠".yellow(), index, id),
        other => eprintln!("{} Result: {}", "→".cyan(), other),
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── mget ─────────────────────────────────────────────────────────────────────

async fn handle_mget(
    client: &EsClient, cfg: &AppConfig,
    index: &str, ids: &[String], source_only: bool,
) -> Result<()> {
    let docs: Vec<Value> = ids.iter()
        .map(|id| serde_json::json!({ "_id": id }))
        .collect();
    let body = serde_json::json!({ "docs": docs });

    let path = if source_only {
        format!("/{}/_mget?_source=true", index)
    } else {
        format!("/{}/_mget", index)
    };

    let resp = client.post(&path, &body).await?;

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_mget_table(&resp, cfg.no_color),
    }
    Ok(())
}

fn print_mget_table(resp: &Value, no_color: bool) {
    let docs = match resp["docs"].as_array() {
        Some(d) => d,
        None    => { println!("No documents."); return; }
    };

    println!();
    println!("  {:<30} {:<8} {:<10}", "_id".dimmed(), "found".dimmed(), "version".dimmed());
    println!("  {}", "─".repeat(52).dimmed());

    for doc in docs {
        let id      = doc["_id"].as_str().unwrap_or("-");
        let found   = doc["found"].as_bool().unwrap_or(false);
        let version = doc["_version"].as_u64().map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        let found_str = if no_color || !found {
            found.to_string()
        } else {
            "true".green().to_string()
        };
        println!("  {:<30} {:<8} {:<10}", id, found_str, version);
    }
    println!();
}

// ── count ─────────────────────────────────────────────────────────────────────

async fn handle_count(
    client: &EsClient, cfg: &AppConfig,
    index: &str, query: Option<&str>, dsl: Option<&str>,
) -> Result<()> {
    let body = build_query_body(query, dsl).await?;
    let path = format!("/{}/_count", index);
    let resp = client.post(&path, &body).await?;

    let count = resp["count"].as_u64().unwrap_or(0);
    match cfg.output {
        OutputFormat::Table => {
            println!();
            println!("  {} {}", "Count:".bold(), format_number(count));
            println!();
        }
        OutputFormat::Json => cfg.output.print(&resp, cfg.no_color),
    }
    Ok(())
}

// ── delete-by-query ───────────────────────────────────────────────────────────

async fn handle_delete_by_query(
    client: &EsClient, cfg: &AppConfig,
    index: &str, query: Option<&str>, dsl: Option<&str>,
    wait: bool, slices: u32,
) -> Result<()> {
    let body = build_query_body(query, dsl).await?;

    let mut path = format!("/{}/_delete_by_query", index);
    let mut params = vec![];
    if wait   { params.push("wait_for_completion=true".to_string()); }
    if slices > 1 { params.push(format!("slices={}", slices)); }
    if !params.is_empty() { path.push_str(&format!("?{}", params.join("&"))); }

    eprintln!("{} Running delete-by-query on '{}'…", "→".cyan(), index.bold());
    let resp = client.post(&path, &body).await?;

    if let Some(deleted) = resp["deleted"].as_u64() {
        eprintln!("{} Deleted {} document(s).", "✓".green(), format_number(deleted));
    }
    if let Some(failures) = resp["failures"].as_array() {
        if !failures.is_empty() {
            eprintln!("{} {} failure(s).", "⚠".yellow(), failures.len());
        }
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── update-by-query ───────────────────────────────────────────────────────────

async fn handle_update_by_query(
    client: &EsClient, cfg: &AppConfig,
    index: &str, script: Option<&str>, dsl: Option<&str>,
    wait: bool, slices: u32,
) -> Result<()> {
    let mut body = if let Some(src) = dsl {
        stdin::read_json(src).await?
    } else {
        serde_json::json!({ "query": { "match_all": {} } })
    };

    if let Some(script_src) = script {
        body["script"] = serde_json::json!({
            "source": script_src,
            "lang":   "painless",
        });
    }

    let mut path = format!("/{}/_update_by_query", index);
    let mut params = vec![];
    if wait   { params.push("wait_for_completion=true".to_string()); }
    if slices > 1 { params.push(format!("slices={}", slices)); }
    if !params.is_empty() { path.push_str(&format!("?{}", params.join("&"))); }

    eprintln!("{} Running update-by-query on '{}'…", "→".cyan(), index.bold());
    let resp = client.post(&path, &body).await?;

    if let Some(updated) = resp["updated"].as_u64() {
        eprintln!("{} Updated {} document(s).", "✓".green(), format_number(updated));
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── bulk ──────────────────────────────────────────────────────────────────────

async fn handle_bulk(client: &EsClient, cfg: &AppConfig, args: BulkArgs) -> Result<()> {
    let src = args.source.as_deref().unwrap_or("-");
    let ndjson = stdin::read_ndjson(src).await?;

    // Optionally prepend index to each action line if missing
    let ndjson = ensure_index_in_bulk(&ndjson, &args.index);

    let total_ops = stdin::count_bulk_ops(&ndjson);
    eprintln!(
        "{} Bulk-indexing {} operation(s) into '{}'…",
        "→".cyan(), total_ops, args.index.bold()
    );

    if args.batch == 0 || total_ops <= args.batch {
        // Single request
        let path = build_bulk_path(&args.index, args.pipeline.as_deref());
        let resp = client.post_raw(&path, ndjson, "application/x-ndjson").await?;
        print_bulk_result(&resp, args.show_errors, cfg);
    } else {
        // Batched requests with progress bar
        run_batched_bulk(client, cfg, &ndjson, &args).await?;
    }
    Ok(())
}

async fn run_batched_bulk(
    client: &EsClient, cfg: &AppConfig,
    ndjson: &str, args: &BulkArgs,
) -> Result<()> {
    let all_lines: Vec<&str> = ndjson.lines().filter(|l| !l.trim().is_empty()).collect();
    let total_pairs = all_lines.len() / 2;
    let batch_pairs = args.batch;

    let pb = ProgressBar::new(total_pairs as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} docs ({eta})")
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
    );

    let mut total_indexed = 0u64;
    let mut total_errors  = 0usize;
    let mut chunk_start   = 0;

    while chunk_start < all_lines.len() {
        let end = (chunk_start + batch_pairs * 2).min(all_lines.len());
        let chunk = all_lines[chunk_start..end].join("\n") + "\n";
        let ops   = (end - chunk_start) / 2;

        let path = build_bulk_path(&args.index, args.pipeline.as_deref());
        let resp  = client.post_raw(&path, chunk, "application/x-ndjson").await?;
        let (_, errors, _) = stdin::parse_bulk_response(&resp);

        total_indexed += ops as u64;
        total_errors  += errors;
        pb.inc(ops as u64);
        chunk_start = end;
    }

    pb.finish_and_clear();
    eprintln!(
        "{} Bulk complete: {} indexed, {} error(s).",
        if total_errors == 0 { "✓".green() } else { "⚠".yellow() },
        format_number(total_indexed),
        total_errors,
    );
    Ok(())
}

fn build_bulk_path(index: &str, pipeline: Option<&str>) -> String {
    let base = format!("/{}/_bulk", index);
    match pipeline {
        Some(p) => format!("{}?pipeline={}", base, p),
        None    => base,
    }
}

/// If action lines don't already specify `_index`, inject the provided index name.
fn ensure_index_in_bulk(ndjson: &str, index: &str) -> String {
    let mut out = String::new();
    let mut is_action = true;
    for line in ndjson.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        if is_action {
            // Try to parse as JSON; if it's missing _index, inject it
            if let Ok(mut action) = serde_json::from_str::<Value>(trimmed) {
                for op in &["index", "create", "update", "delete"] {
                    if action[op].is_object() {
                        if action[op]["_index"].is_null() {
                            action[*op]["_index"] = Value::String(index.to_string());
                        }
                        break;
                    }
                }
                out.push_str(&serde_json::to_string(&action).unwrap_or_default());
            } else {
                out.push_str(trimmed);
            }
            out.push('\n');
            is_action = false;
        } else {
            out.push_str(trimmed);
            out.push('\n');
            is_action = true;
        }
    }
    out
}

fn print_bulk_result(resp: &Value, show_errors: usize, cfg: &AppConfig) {
    let (total, errors, failed) = stdin::parse_bulk_response(resp);
    let took = resp["took"].as_u64().unwrap_or(0);

    if errors == 0 {
        eprintln!(
            "{} {} doc(s) indexed in {}ms.",
            "✓".green(), format_number(total as u64), took
        );
    } else {
        eprintln!(
            "{} {}/{} doc(s) indexed, {} error(s) — took {}ms.",
            "⚠".yellow(),
            format_number((total - errors) as u64),
            format_number(total as u64),
            errors,
            took
        );
        // Show first N failed items
        for item in failed.iter().take(show_errors) {
            for op in &["index", "create", "update", "delete"] {
                if let Some(obj) = item[op].as_object() {
                    if let Some(err) = obj.get("error") {
                        let id     = obj.get("_id").and_then(Value::as_str).unwrap_or("-");
                        let reason = err.pointer("/reason").and_then(Value::as_str).unwrap_or("unknown");
                        eprintln!("  {} id={}: {}", "✗".red(), id, reason);
                    }
                }
            }
        }
        if failed.len() > show_errors {
            eprintln!("  … and {} more errors", failed.len() - show_errors);
        }
    }

    match cfg.output {
        OutputFormat::Json => cfg.output.print(resp, cfg.no_color),
        OutputFormat::Table => {} // summary already printed above
    }
}

// ── write ─────────────────────────────────────────────────────────────────────

async fn handle_write(
    client: &EsClient, cfg: &AppConfig,
    index: &str, id: Option<&str>, body: Option<&str>, create: bool,
) -> Result<()> {
    let src = body.unwrap_or("-");
    let doc = stdin::read_json(src).await?;

    let resp = match (id, create) {
        (Some(id), true) => {
            client.put(&format!("/{}/_create/{}", index, id), &doc).await?
        }
        (Some(id), false) => {
            client.put(&format!("/{}/_doc/{}", index, id), &doc).await?
        }
        (None, _) => {
            client.post(&format!("/{}/_doc", index), &doc).await?
        }
    };

    print_write_result(&resp, cfg);
    Ok(())
}

// ── ingest (batch pipe) ───────────────────────────────────────────────────────

async fn handle_ingest(
    client: &EsClient, cfg: &AppConfig,
    index: &str, body: Option<&str>, batch: usize, id_field: Option<&str>,
) -> Result<()> {
    let src  = body.unwrap_or("-");
    let docs = stdin::read_json_stream(src).await?;

    if docs.is_empty() {
        eprintln!("{} No documents to ingest.", "⚠".yellow());
        return Ok(());
    }

    eprintln!(
        "{} Ingesting {} document(s) into '{}' (batch={})…",
        "→".cyan(), docs.len(), index.bold(), batch
    );

    let pb = ProgressBar::new(docs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
    );

    let mut total_errors = 0usize;
    let mut total_ok     = 0u64;

    for chunk in docs.chunks(batch) {
        let pairs: Vec<(Option<String>, Value)> = chunk.iter().map(|doc| {
            let id = id_field.and_then(|f| doc[f].as_str().map(String::from));
            (id, doc.clone())
        }).collect();

        let ndjson = stdin::build_bulk_ndjson_with_ids(index, &pairs);
        let resp   = client.post_raw(
            &format!("/{}/_bulk", index),
            ndjson,
            "application/x-ndjson",
        ).await?;

        let (_, errors, _) = stdin::parse_bulk_response(&resp);
        total_errors += errors;
        total_ok     += (chunk.len() - errors) as u64;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_and_clear();
    eprintln!(
        "{} Ingest complete: {} ok, {} error(s).",
        if total_errors == 0 { "✓".green() } else { "⚠".yellow() },
        format_number(total_ok),
        total_errors,
    );
    let _ = cfg; // output already printed via progress
    Ok(())
}

// ── shared helpers ────────────────────────────────────────────────────────────

async fn build_query_body(
    query: Option<&str>, dsl: Option<&str>,
) -> Result<Value> {
    if let Some(src) = dsl {
        return stdin::read_json(src).await;
    }
    if let Some(q) = query {
        return Ok(serde_json::json!({
            "query": { "query_string": { "query": q } }
        }));
    }
    Ok(serde_json::json!({ "query": { "match_all": {} } }))
}

fn print_write_result(resp: &Value, cfg: &AppConfig) {
    let result  = resp["result"].as_str().unwrap_or("-");
    let id      = resp["_id"].as_str().unwrap_or("-");
    let version = resp["_version"].as_u64().unwrap_or(0);

    match result {
        "created" => eprintln!("{} Created  id={} v={}", "✓".green(),  id, version),
        "updated" => eprintln!("{} Updated  id={} v={}", "✓".green(),  id, version),
        "deleted" => eprintln!("{} Deleted  id={} v={}", "✓".green(),  id, version),
        "noop"    => eprintln!("{} No-op    id={}",      "–".dimmed(), id),
        other     => eprintln!("{} {}  id={} v={}", "→".cyan(), other, id, version),
    }

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(resp, cfg.no_color),
        OutputFormat::Table => {} // summary already on stderr
    }
}

fn format_cell_value(v: &Value) -> String {
    match v {
        Value::String(s) => {
            if s.len() > 80 { format!("{}…", &s[..79]) } else { s.clone() }
        }
        Value::Null   => "-".into(),
        other         => {
            let s = serde_json::to_string(other).unwrap_or_default();
            if s.len() > 80 { format!("{}…", &s[..79]) } else { s }
        }
    }
}

pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out.chars().rev().collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0),         "0");
        assert_eq!(format_number(1_000),     "1,000");
        assert_eq!(format_number(1_234_567), "1,234,567");
    }

    #[test]
    fn test_format_cell_value_truncates() {
        let long = "x".repeat(100);
        let v    = Value::String(long);
        let out  = format_cell_value(&v);
        assert!(out.ends_with('…'));
        assert!(out.len() <= 81);
    }

    #[test]
    fn test_format_cell_value_short() {
        let v = Value::String("hello".into());
        assert_eq!(format_cell_value(&v), "hello");
    }

    #[test]
    fn test_format_cell_value_null() {
        assert_eq!(format_cell_value(&Value::Null), "-");
    }

    #[test]
    fn test_build_bulk_path_no_pipeline() {
        let path = build_bulk_path("my-index", None);
        assert_eq!(path, "/my-index/_bulk");
    }

    #[test]
    fn test_build_bulk_path_with_pipeline() {
        let path = build_bulk_path("my-index", Some("my-pipeline"));
        assert_eq!(path, "/my-index/_bulk?pipeline=my-pipeline");
    }

    #[test]
    fn test_ensure_index_in_bulk_injects() {
        let ndjson = "{\"index\":{}}\n{\"name\":\"alice\"}\n";
        let result = ensure_index_in_bulk(ndjson, "target");
        assert!(result.contains("\"_index\":\"target\""));
        assert!(result.contains("alice"));
    }

    #[test]
    fn test_ensure_index_in_bulk_preserves_existing() {
        let ndjson = "{\"index\":{\"_index\":\"original\"}}\n{\"name\":\"bob\"}\n";
        let result = ensure_index_in_bulk(ndjson, "target");
        // Should NOT overwrite existing _index
        assert!(result.contains("\"_index\":\"original\""));
    }

    #[test]
    fn test_ensure_index_in_bulk_handles_all_ops() {
        for op in &["create", "update", "delete"] {
            let ndjson = format!("{{\"{op}\":{{}}}}\n{{\"x\":1}}\n");
            let result = ensure_index_in_bulk(&ndjson, "idx");
            assert!(result.contains("\"_index\":\"idx\""), "failed for op: {}", op);
        }
    }

    #[tokio::test]
    async fn test_build_query_body_match_all() {
        let body = build_query_body(None, None).await.unwrap();
        assert!(body.pointer("/query/match_all").is_some());
    }

    #[tokio::test]
    async fn test_build_query_body_query_string() {
        let body = build_query_body(Some("status:active"), None).await.unwrap();
        let q = body.pointer("/query/query_string/query")
            .and_then(Value::as_str).unwrap();
        assert_eq!(q, "status:active");
    }

    #[tokio::test]
    async fn test_build_query_body_dsl_from_file() {
        let path = "/tmp/esctl_test_dsl.json";
        tokio::fs::write(path, b"{\"query\":{\"term\":{\"status\":\"active\"}}}").await.unwrap();
        let body = build_query_body(None, Some(path)).await.unwrap();
        assert!(body.pointer("/query/term").is_some());
    }

    #[test]
    fn test_delete_path_no_params() {
        let index = "my-index";
        let id    = "doc-1";
        let path  = format!("/{}/_doc/{}", index, id);
        assert_eq!(path, "/my-index/_doc/doc-1");
    }

    #[test]
    fn test_delete_path_with_seq_no() {
        let mut path = "/my-index/_doc/doc-1".to_string();
        let mut params: Vec<String> = vec![];
        let if_seq_no: Option<u64> = Some(5);
        let if_primary_term: Option<u64> = Some(1);
        if let Some(s) = if_seq_no      { params.push(format!("if_seq_no={}", s)); }
        if let Some(p) = if_primary_term { params.push(format!("if_primary_term={}", p)); }
        if !params.is_empty() { path.push_str(&format!("?{}", params.join("&"))); }
        assert_eq!(path, "/my-index/_doc/doc-1?if_seq_no=5&if_primary_term=1");
    }

    #[test]
    fn test_update_wraps_partial_doc() {
        let raw = serde_json::json!({"name": "alice"});
        let payload = if raw.get("doc").is_none() && raw.get("script").is_none() {
            serde_json::json!({ "doc": raw })
        } else {
            raw
        };
        assert!(payload.get("doc").is_some());
        assert_eq!(payload["doc"]["name"], "alice");
    }

    #[test]
    fn test_update_preserves_existing_doc_key() {
        let raw = serde_json::json!({"doc": {"name": "bob"}});
        let payload = if raw.get("doc").is_none() && raw.get("script").is_none() {
            serde_json::json!({ "doc": raw })
        } else {
            raw.clone()
        };
        // Should NOT double-wrap
        assert_eq!(payload["doc"]["name"], "bob");
    }

    #[test]
    fn test_mget_body_structure() {
        let ids = vec!["id-1".to_string(), "id-2".to_string()];
        let docs: Vec<Value> = ids.iter()
            .map(|id| serde_json::json!({ "_id": id }))
            .collect();
        let body = serde_json::json!({ "docs": docs });
        assert_eq!(body["docs"].as_array().unwrap().len(), 2);
        assert_eq!(body["docs"][0]["_id"], "id-1");
    }
}
