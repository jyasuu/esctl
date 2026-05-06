use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;
use serde_json::Value;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;
use crate::stdin;

// ── Subcommands ───────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum IndexCommands {
    /// List indices (glob pattern supported)
    List {
        #[arg(default_value = "*")]
        pattern: String,
        #[arg(long, default_value = "index")]
        sort: String,
        #[arg(long, default_value = "all", value_parser = ["open","closed","all"])]
        health: String,
        /// Show system indices (starting with .)
        #[arg(long)]
        system: bool,
    },
    /// Create an index
    Create(CreateArgs),
    /// Delete one or more indices
    Delete(DeleteArgs),
    /// Open a closed index
    Open {
        name: String,
        #[arg(long, default_value = "1")]
        wait_for_active_shards: String,
    },
    /// Close an index
    Close {
        name: String,
        #[arg(long)]
        yes: bool,
    },
    /// Show index stats (docs, store, search, indexing)
    Stats {
        name: String,
        #[arg(long)]
        summary: bool,
    },
    /// Show index info (settings + mapping combined)
    Info { name: String },
    /// Force-merge an index to reduce segment count
    Forcemerge {
        name: String,
        #[arg(long, default_value = "1")]
        max_num_segments: u32,
        #[arg(long)]
        wait: bool,
    },
    /// Refresh an index (make recent writes searchable immediately)
    Refresh { name: String },
    /// Flush an index (fsync to disk)
    Flush { name: String },
    /// Clear index cache
    ClearCache {
        name: String,
        /// field | query | request  or "all"
        #[arg(long, default_value = "all")]
        cache: String,
    },
    /// Clone an index (source must be read-only)
    Clone {
        source: String,
        target: String,
        #[arg(long, default_value = "1")]
        shards: u32,
        #[arg(long, default_value = "1")]
        replicas: u32,
    },
    /// Shrink an index (reduce primary shard count)
    Shrink {
        source: String,
        target: String,
        #[arg(long)]
        shards: u32,
        #[arg(long, default_value = "1")]
        replicas: u32,
    },
    /// Show index recovery status
    Recovery { name: String },
    /// Show index segments info
    Segments { name: String },
}

#[derive(Args, Debug)]
pub struct CreateArgs {
    pub name: String,
    #[arg(long, default_value = "1")]
    pub shards: u32,
    #[arg(long, default_value = "1")]
    pub replicas: u32,
    /// Full settings/mappings body (file path or - for stdin)
    #[arg(long)]
    pub body: Option<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    #[arg(required = true)]
    pub names: Vec<String>,
    #[arg(long)]
    pub yes: bool,
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn handle(cmd: IndexCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        IndexCommands::List { pattern, sort, health, system } =>
            handle_list(client, cfg, &pattern, &sort, &health, system).await,
        IndexCommands::Create(a) => handle_create(client, cfg, a).await,
        IndexCommands::Delete(a) => handle_delete(client, cfg, a).await,
        IndexCommands::Open { name, wait_for_active_shards } =>
            handle_open(client, cfg, &name, &wait_for_active_shards).await,
        IndexCommands::Close { name, yes } => handle_close(client, cfg, &name, yes).await,
        IndexCommands::Stats { name, summary } => handle_stats(client, cfg, &name, summary).await,
        IndexCommands::Info { name } => handle_info(client, cfg, &name).await,
        IndexCommands::Forcemerge { name, max_num_segments, wait } =>
            handle_forcemerge(client, cfg, &name, max_num_segments, wait).await,
        IndexCommands::Refresh { name } => {
            let resp = client.post(&format!("/{}/_refresh", name), &Value::Null).await?;
            eprintln!("{} Refreshed '{}'.", "✓".green(), name.bold());
            cfg.output.print(&resp, cfg.no_color);
            Ok(())
        }
        IndexCommands::Flush { name } => {
            let resp = client.post(&format!("/{}/_flush", name), &Value::Null).await?;
            eprintln!("{} Flushed '{}'.", "✓".green(), name.bold());
            cfg.output.print(&resp, cfg.no_color);
            Ok(())
        }
        IndexCommands::ClearCache { name, cache } =>
            handle_clear_cache(client, cfg, &name, &cache).await,
        IndexCommands::Clone { source, target, shards, replicas } =>
            handle_clone(client, cfg, &source, &target, shards, replicas).await,
        IndexCommands::Shrink { source, target, shards, replicas } =>
            handle_shrink(client, cfg, &source, &target, shards, replicas).await,
        IndexCommands::Recovery { name } => {
            let resp = client.get(&format!("/{}/_recovery?human", name)).await?;
            cfg.output.print(&resp, cfg.no_color);
            Ok(())
        }
        IndexCommands::Segments { name } => {
            let resp = client.get(&format!("/{}/_segments", name)).await?;
            cfg.output.print(&resp, cfg.no_color);
            Ok(())
        }
    }
}

// ── list ──────────────────────────────────────────────────────────────────────

async fn handle_list(
    client: &EsClient, cfg: &AppConfig,
    pattern: &str, sort: &str, health: &str, system: bool,
) -> Result<()> {
    let path = format!(
        "/_cat/indices/{}?format=json&s={}&expand_wildcards=all",
        pattern, sort
    );
    let resp = client.get(&path).await?;
    let filtered = filter_indices(resp, health, system);

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&filtered, cfg.no_color),
        OutputFormat::Table => print_index_table(&filtered, cfg.no_color),
    }
    Ok(())
}

pub fn filter_indices(resp: Value, health: &str, system: bool) -> Value {
    let arr = match resp.as_array() {
        Some(a) => a.clone(),
        None    => return resp,
    };
    let filtered: Vec<Value> = arr.into_iter().filter(|idx| {
        let status     = idx["status"].as_str().unwrap_or("open");
        let pass_status = match health {
            "open"   => status == "open",
            "closed" => status == "close",
            _        => true,
        };
        let name       = idx["index"].as_str().unwrap_or("");
        let pass_sys   = system || !name.starts_with('.');
        pass_status && pass_sys
    }).collect();
    Value::Array(filtered)
}

fn print_index_table(v: &Value, no_color: bool) {
    let arr = match v.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => { println!("No indices found."); return; }
    };
    println!();
    println!(
        "  {:<42} {:<8} {:<8} {:>10} {:>12} {:>8}",
        "index", "health", "status", "docs", "store", "pri/rep"
    );
    println!("  {}", "─".repeat(96).dimmed());
    for idx in arr {
        let name   = idx["index"].as_str().unwrap_or("-");
        let health = idx["health"].as_str().unwrap_or("-");
        let status = idx["status"].as_str().unwrap_or("-");
        let docs   = idx["docs.count"].as_str().unwrap_or("-");
        let store  = idx["store.size"].as_str().unwrap_or("-");
        let pri    = idx["pri"].as_str().unwrap_or("-");
        let rep    = idx["rep"].as_str().unwrap_or("-");

        let health_str = if no_color { health.to_string() } else {
            match health {
                "green"  => health.green().to_string(),
                "yellow" => health.yellow().to_string(),
                "red"    => health.red().bold().to_string(),
                other    => other.dimmed().to_string(),
            }
        };
        let status_str = if no_color || status == "open" {
            status.to_string()
        } else {
            status.dimmed().to_string()
        };
        println!(
            "  {:<42} {:<8} {:<8} {:>10} {:>12} {:>8}",
            truncate(name, 42), health_str, status_str,
            docs, store, format!("{}/{}", pri, rep),
        );
    }
    println!();
    println!("  {} {}", arr.len(), if arr.len() == 1 { "index" } else { "indices" });
    println!();
}

// ── create ────────────────────────────────────────────────────────────────────

async fn handle_create(client: &EsClient, cfg: &AppConfig, args: CreateArgs) -> Result<()> {
    let body = if let Some(ref src) = args.body {
        stdin::read_json(src).await?
    } else {
        serde_json::json!({
            "settings": {
                "number_of_shards":   args.shards,
                "number_of_replicas": args.replicas,
            }
        })
    };
    eprintln!("{} Creating index '{}'…", "→".cyan(), args.name.bold());
    let resp = client.put(&format!("/{}", args.name), &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Index '{}' created.", "✓".green(), args.name.bold());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── delete ────────────────────────────────────────────────────────────────────

async fn handle_delete(client: &EsClient, cfg: &AppConfig, args: DeleteArgs) -> Result<()> {
    let names_str = args.names.join(", ");
    if !args.yes {
        eprint!(
            "{} Delete {}? [y/N] ",
            "⚠".yellow(),
            if args.names.len() == 1 {
                format!("index '{}'", names_str)
            } else {
                format!("indices: {}", names_str)
            }
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }
    let path = format!("/{}", args.names.join(","));
    let resp = client.delete(&path).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Deleted: {}", "✓".green(), names_str);
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── open / close ──────────────────────────────────────────────────────────────

async fn handle_open(
    client: &EsClient, cfg: &AppConfig, name: &str, wait_for_active_shards: &str,
) -> Result<()> {
    eprintln!("{} Opening index '{}'…", "→".cyan(), name.bold());
    let path = format!("/{}/_open?wait_for_active_shards={}", name, wait_for_active_shards);
    let resp = client.post(&path, &Value::Null).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Index '{}' opened.", "✓".green(), name.bold());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_close(client: &EsClient, cfg: &AppConfig, name: &str, yes: bool) -> Result<()> {
    if !yes {
        eprint!(
            "{} Close index '{}' (unavailable for reads/writes)? [y/N] ",
            "⚠".yellow(), name.bold()
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }
    let resp = client.post(&format!("/{}/_close", name), &Value::Null).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Index '{}' closed.", "✓".green(), name.bold());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── stats ─────────────────────────────────────────────────────────────────────

async fn handle_stats(
    client: &EsClient, cfg: &AppConfig, name: &str, summary: bool,
) -> Result<()> {
    let resp = client.get(&format!("/{}/_stats?human", name)).await?;
    if !summary {
        cfg.output.print(&resp, cfg.no_color);
        return Ok(());
    }
    match cfg.output {
        OutputFormat::Json  => { let s = build_stats_summary(name, &resp); cfg.output.print(&s, cfg.no_color); }
        OutputFormat::Table => print_stats_table(name, &resp, cfg.no_color),
    }
    Ok(())
}

pub fn build_stats_summary(name: &str, v: &Value) -> Value {
    let ptr = |p: &str| v.pointer(&format!("/_all/indices/{}/primaries/{}", name, p)).cloned().unwrap_or(Value::Null);
    serde_json::json!({
        "index":          name,
        "docs_count":     ptr("docs/count"),
        "docs_deleted":   ptr("docs/deleted"),
        "store_size":     ptr("store/size_in_bytes"),
        "indexing_total": ptr("indexing/index_total"),
        "search_total":   ptr("search/query_total"),
        "search_time_ms": ptr("search/query_time_in_millis"),
        "merges_total":   ptr("merges/total"),
        "segments":       ptr("segments/count"),
    })
}

fn print_stats_table(name: &str, v: &Value, _no_color: bool) {
    let u64v = |p: &str| -> String {
        v.pointer(&format!("/_all/indices/{}/primaries/{}", name, p))
            .and_then(Value::as_u64).map(format_number).unwrap_or_else(|| "-".into())
    };
    let bytes = |p: &str| -> String {
        v.pointer(&format!("/_all/indices/{}/primaries/{}", name, p))
            .and_then(Value::as_u64).map(human_bytes).unwrap_or_else(|| "-".into())
    };
    println!();
    println!("  {} {}", "Index:".bold(), name);
    println!();
    println!("  {:<32} {}", "Docs (primary):",      u64v("docs/count"));
    println!("  {:<32} {}", "Docs deleted:",         u64v("docs/deleted"));
    println!("  {:<32} {}", "Store (primary):",      bytes("store/size_in_bytes"));
    println!("  {:<32} {}", "Indexing ops:",         u64v("indexing/index_total"));
    println!("  {:<32} {}", "Search queries:",       u64v("search/query_total"));
    println!("  {:<32} {}ms", "Search time:",        u64v("search/query_time_in_millis"));
    println!("  {:<32} {}", "Merges:",               u64v("merges/total"));
    println!("  {:<32} {}", "Segments:",             u64v("segments/count"));
    println!();
}

// ── info ──────────────────────────────────────────────────────────────────────

async fn handle_info(client: &EsClient, cfg: &AppConfig, name: &str) -> Result<()> {
    let (settings_resp, mapping_resp) = tokio::try_join!(
        client.get(&format!("/{}/_settings", name)),
        client.get(&format!("/{}/_mapping", name)),
    )?;
    match cfg.output {
        OutputFormat::Json  => {
            let combined = serde_json::json!({ "settings": settings_resp, "mapping": mapping_resp });
            cfg.output.print(&combined, cfg.no_color);
        }
        OutputFormat::Table => print_info_table(name, &settings_resp, &mapping_resp),
    }
    Ok(())
}

fn print_info_table(name: &str, settings: &Value, mapping: &Value) {
    let s = |p: &str| -> &str {
        settings.pointer(&format!("/{}/settings/index/{}", name, p))
            .and_then(Value::as_str).unwrap_or("-")
    };
    println!();
    println!("  {} {}", "Index:".bold(), name.bold());
    println!();
    println!("  {}", "── Settings ──".dimmed());
    println!("  {:<30} {}", "Shards (primary):", s("number_of_shards"));
    println!("  {:<30} {}", "Replicas:",          s("number_of_replicas"));
    println!("  {:<30} {}", "UUID:",              s("uuid"));
    println!("  {:<30} {}", "Provided name:",     s("provided_name"));

    let field_count = count_mapping_fields(mapping, name);
    println!();
    println!("  {}", "── Mapping ──".dimmed());
    println!("  {:<30} {}", "Mapped fields:", field_count);

    let fields_path = format!("/{}/mappings/properties", name);
    if let Some(props) = mapping.pointer(&fields_path).and_then(Value::as_object) {
        println!();
        println!("  {:<32} {}", "field".dimmed(), "type".dimmed());
        println!("  {}", "─".repeat(50).dimmed());
        let mut names: Vec<&String> = props.keys().collect();
        names.sort();
        for field_name in names.iter().take(20) {
            let ftype = props[*field_name]["type"].as_str().unwrap_or("object");
            println!("  {:<32} {}", field_name, ftype);
        }
        if names.len() > 20 {
            println!("  … and {} more fields", names.len() - 20);
        }
    }
    println!();
}

pub fn count_mapping_fields(mapping: &Value, name: &str) -> usize {
    mapping.pointer(&format!("/{}/mappings/properties", name))
        .and_then(Value::as_object).map(|m| m.len()).unwrap_or(0)
}

// ── forcemerge ────────────────────────────────────────────────────────────────

async fn handle_forcemerge(
    client: &EsClient, cfg: &AppConfig, name: &str, max_num_segments: u32, wait: bool,
) -> Result<()> {
    eprintln!("{} Force-merging '{}' to {} segment(s)…", "→".cyan(), name.bold(), max_num_segments);
    let base = format!("/{}/_forcemerge?max_num_segments={}", name, max_num_segments);
    let path = if wait { format!("{}&wait_for_completion=true", base) } else { base };
    let resp = client.post(&path, &Value::Null).await?;
    eprintln!("{} Force-merge dispatched.", "✓".green());
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── clear-cache ───────────────────────────────────────────────────────────────

async fn handle_clear_cache(
    client: &EsClient, cfg: &AppConfig, name: &str, cache: &str,
) -> Result<()> {
    let path = if cache == "all" {
        format!("/{}/_cache/clear", name)
    } else {
        let params: String = cache.split(',')
            .map(|c| format!("{}_cache=true", c.trim()))
            .collect::<Vec<_>>().join("&");
        format!("/{}/_cache/clear?{}", name, params)
    };
    let resp = client.post(&path, &Value::Null).await?;
    eprintln!("{} Cache cleared for '{}'.", "✓".green(), name.bold());
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── clone / shrink ────────────────────────────────────────────────────────────

async fn handle_clone(
    client: &EsClient, cfg: &AppConfig,
    source: &str, target: &str, shards: u32, replicas: u32,
) -> Result<()> {
    eprintln!("{} Setting '{}' read-only…", "→".cyan(), source.bold());
    client.put(&format!("/{}/_settings", source),
        &serde_json::json!({"settings":{"index.blocks.write":true}})).await?;

    eprintln!("{} Cloning '{}' → '{}'…", "→".cyan(), source.bold(), target.bold());
    let body = serde_json::json!({"settings":{"number_of_shards":shards,"number_of_replicas":replicas}});
    let resp = client.post(&format!("/{}/_clone/{}", source, target), &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Clone complete.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_shrink(
    client: &EsClient, cfg: &AppConfig,
    source: &str, target: &str, shards: u32, replicas: u32,
) -> Result<()> {
    eprintln!("{} Setting '{}' read-only…", "→".cyan(), source.bold());
    client.put(&format!("/{}/_settings", source),
        &serde_json::json!({"settings":{"index.blocks.write":true}})).await?;

    eprintln!("{} Shrinking '{}' → '{}' ({} shard(s))…", "→".cyan(), source.bold(), target.bold(), shards);
    let body = serde_json::json!({
        "settings": {
            "number_of_shards": shards,
            "number_of_replicas": replicas,
            "codec": "best_compression",
        }
    });
    let resp = client.post(&format!("/{}/_shrink/{}", source, target), &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Shrink complete.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

pub fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    match bytes {
        b if b >= TB => format!("{:.2} TB", b as f64 / TB as f64),
        b if b >= GB => format!("{:.2} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{:.2} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.2} KB", b as f64 / KB as f64),
        b            => format!("{} B", b),
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

pub fn truncate(s: &str, max: usize) -> String {
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
    fn test_human_bytes() {
        assert_eq!(human_bytes(0),               "0 B");
        assert_eq!(human_bytes(512),             "512 B");
        assert_eq!(human_bytes(1024),            "1.00 KB");
        assert_eq!(human_bytes(1_048_576),       "1.00 MB");
        assert_eq!(human_bytes(1_073_741_824),   "1.00 GB");
        assert_eq!(human_bytes(1_099_511_627_776), "1.00 TB");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0),         "0");
        assert_eq!(format_number(999),       "999");
        assert_eq!(format_number(1_000),     "1,000");
        assert_eq!(format_number(1_000_000), "1,000,000");
        assert_eq!(format_number(12_345),    "12,345");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10),      "hello");
        assert_eq!(truncate("hello world!", 8),"hello wo…");
        assert_eq!(truncate("", 5),            "");
        assert_eq!(truncate("exact", 5),       "exact");
    }

    #[test]
    fn test_filter_indices_all() {
        let data = serde_json::json!([
            {"index":"logs",    "status":"open"},
            {"index":".sys",    "status":"open"},
            {"index":"archive", "status":"close"},
        ]);
        assert_eq!(filter_indices(data, "all", true).as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_filter_indices_open_only() {
        let data = serde_json::json!([
            {"index":"logs",    "status":"open"},
            {"index":"archive", "status":"close"},
        ]);
        let result = filter_indices(data, "open", false);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["index"], "logs");
    }

    #[test]
    fn test_filter_indices_hides_system() {
        let data = serde_json::json!([
            {"index":"user-data", "status":"open"},
            {"index":".kibana",   "status":"open"},
        ]);
        let result = filter_indices(data, "all", false);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["index"], "user-data");
    }

    #[test]
    fn test_filter_indices_closed_only() {
        let data = serde_json::json!([
            {"index":"a","status":"open"},
            {"index":"b","status":"close"},
            {"index":"c","status":"close"},
        ]);
        assert_eq!(filter_indices(data, "closed", true).as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_count_mapping_fields() {
        let mapping = serde_json::json!({
            "my-index": {
                "mappings": {
                    "properties": {
                        "title":   {"type":"text"},
                        "date":    {"type":"date"},
                        "user_id": {"type":"keyword"},
                    }
                }
            }
        });
        assert_eq!(count_mapping_fields(&mapping, "my-index"), 3);
    }

    #[test]
    fn test_count_mapping_fields_empty() {
        let mapping = serde_json::json!({"other-index":{"mappings":{}}});
        assert_eq!(count_mapping_fields(&mapping, "my-index"), 0);
    }

    #[test]
    fn test_build_stats_summary_has_keys() {
        let resp = serde_json::json!({});
        let summary = build_stats_summary("my-index", &resp);
        assert_eq!(summary["index"], "my-index");
        assert!(summary.get("docs_count").is_some());
        assert!(summary.get("store_size").is_some());
        assert!(summary.get("search_total").is_some());
    }

    #[test]
    fn test_clear_cache_path_all() {
        let path = "/my-index/_cache/clear".to_string();
        assert!(path.ends_with("/_cache/clear"));
    }

    #[test]
    fn test_clear_cache_path_specific() {
        let cache = "field,query";
        let params: String = cache.split(',')
            .map(|c| format!("{}_cache=true", c.trim()))
            .collect::<Vec<_>>().join("&");
        assert_eq!(params, "field_cache=true&query_cache=true");
    }

    #[test]
    fn test_delete_multi_index_path() {
        let names = vec!["idx-a".to_string(), "idx-b".to_string()];
        assert_eq!(format!("/{}", names.join(",")), "/idx-a,idx-b");
    }

    #[test]
    fn test_filter_indices_non_array_passthrough() {
        let data = serde_json::json!({"error": "not an array"});
        let result = filter_indices(data.clone(), "all", true);
        assert_eq!(result, data);
    }
}
