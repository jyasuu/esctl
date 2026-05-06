use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;

/// All cluster subcommands
#[derive(Subcommand, Debug)]
pub enum ClusterCommands {
    /// Cluster health (green / yellow / red)
    Health {
        /// Detail level: cluster | indices | shards
        #[arg(long, default_value = "cluster", value_parser = ["cluster","indices","shards"])]
        level: String,

        /// Wait until cluster reaches this status before returning
        #[arg(long, value_parser = ["green","yellow","red"])]
        wait_for_status: Option<String>,

        /// Timeout for --wait-for-status (e.g. 30s)
        #[arg(long, default_value = "30s")]
        timeout: String,
    },

    /// Basic cluster info (version, name, UUID)
    Info,

    /// Cluster-wide statistics (docs, store, JVM, OS…)
    Stats {
        /// Show only the top-level summary fields
        #[arg(long)]
        summary: bool,
    },

    /// List cluster nodes
    Nodes {
        /// Show all node attributes
        #[arg(long)]
        verbose: bool,

        /// Filter by node role: master | data | ingest | coordinating
        #[arg(long)]
        role: Option<String>,
    },

    /// Pending cluster tasks
    Pending,

    /// Cluster settings (persistent + transient)
    Settings {
        /// Include default settings
        #[arg(long)]
        include_defaults: bool,
    },

    /// Update a cluster setting  (key=value pairs)
    Set {
        /// e.g. cluster.routing.allocation.enable=all
        setting: String,

        /// "persistent" (survives restart) or "transient" (reset on restart)
        #[arg(long, default_value = "transient", value_parser = ["persistent","transient"])]
        r#type: String,
    },

    /// Allocation explain — why is a shard unassigned?
    AllocationExplain {
        /// Index name (optional; ES picks one if omitted)
        #[arg(long)]
        index: Option<String>,

        /// Shard number
        #[arg(long)]
        shard: Option<u32>,

        /// true = replica shard, false = primary
        #[arg(long)]
        primary: Option<bool>,
    },

    /// Reroute: retry failed shards
    Reroute {
        /// Only simulate — do not apply changes
        #[arg(long)]
        dry_run: bool,

        /// Retry all failed shard allocations
        #[arg(long)]
        retry_failed: bool,
    },
}

// ── Dispatch ─────────────────────────────────────────────────────────────────

pub async fn handle(cmd: ClusterCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        ClusterCommands::Health { level, wait_for_status, timeout } => {
            handle_health(client, cfg, &level, wait_for_status, &timeout).await
        }
        ClusterCommands::Info => {
            handle_info(client, cfg).await
        }
        ClusterCommands::Stats { summary } => {
            handle_stats(client, cfg, summary).await
        }
        ClusterCommands::Nodes { verbose, role } => {
            handle_nodes(client, cfg, verbose, role).await
        }
        ClusterCommands::Pending => {
            let resp = client.get("/_cluster/pending_tasks").await?;
            cfg.output.print(&resp, cfg.no_color);
            Ok(())
        }
        ClusterCommands::Settings { include_defaults } => {
            let path = if include_defaults {
                "/_cluster/settings?include_defaults=true"
            } else {
                "/_cluster/settings"
            };
            let resp = client.get(path).await?;
            cfg.output.print(&resp, cfg.no_color);
            Ok(())
        }
        ClusterCommands::Set { setting, r#type } => {
            handle_set(client, cfg, &setting, &r#type).await
        }
        ClusterCommands::AllocationExplain { index, shard, primary } => {
            handle_allocation_explain(client, cfg, index, shard, primary).await
        }
        ClusterCommands::Reroute { dry_run, retry_failed } => {
            handle_reroute(client, cfg, dry_run, retry_failed).await
        }
    }
}

// ── health ────────────────────────────────────────────────────────────────────

async fn handle_health(
    client: &EsClient,
    cfg: &AppConfig,
    level: &str,
    wait_for_status: Option<String>,
    timeout: &str,
) -> Result<()> {
    let mut path = format!("/_cluster/health?level={}", level);
    if let Some(ref status) = wait_for_status {
        path.push_str(&format!(
            "&wait_for_status={}&timeout={}",
            status, timeout
        ));
        eprintln!(
            "{} Waiting for cluster status {} (timeout: {})…",
            "→".cyan(),
            status.to_uppercase().bold(),
            timeout
        );
    }

    let resp = client.get(&path).await?;

    match cfg.output {
        OutputFormat::Json => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_health_table(&resp, cfg.no_color),
    }
    Ok(())
}

/// Human-friendly health summary printed as a status board
fn print_health_table(v: &Value, no_color: bool) {
    let status = v["status"].as_str().unwrap_or("unknown");
    let status_colored = if no_color {
        status.to_string()
    } else {
        match status {
            "green"  => status.green().bold().to_string(),
            "yellow" => status.yellow().bold().to_string(),
            "red"    => status.red().bold().to_string(),
            other    => other.to_string(),
        }
    };

    println!();
    println!(
        "  Cluster : {}",
        v["cluster_name"].as_str().unwrap_or("-").bold()
    );
    println!("  Status  : {}", status_colored);
    println!();

    let rows = [
        ("Nodes",           v["number_of_nodes"].to_string()),
        ("Data nodes",      v["number_of_data_nodes"].to_string()),
        ("Shards",          v["active_shards"].to_string()),
        ("Primary shards",  v["active_primary_shards"].to_string()),
        ("Relocating",      v["relocating_shards"].to_string()),
        ("Initializing",    v["initializing_shards"].to_string()),
        ("Unassigned",      colorize_unassigned(
                                v["unassigned_shards"].as_u64().unwrap_or(0),
                                no_color)),
        ("Pending tasks",   v["number_of_pending_tasks"].to_string()),
        ("In-flight fetch", v["number_of_in_flight_fetch"].to_string()),
        ("Active %",        format!("{:.1}%",
                                v["active_shards_percent_as_number"]
                                    .as_f64().unwrap_or(0.0))),
    ];

    for (label, value) in &rows {
        println!("  {:20} {}", format!("{}:", label), value);
    }

    // Per-index table if present
    if let Some(indices) = v["indices"].as_object() {
        println!();
        println!("  {}", "── Per-index health ──".dimmed());
        println!(
            "  {:<40} {:<8} {:>8} {:>8} {:>8}",
            "index", "status", "shards", "primary", "unassig"
        );
        println!("  {}", "─".repeat(74).dimmed());

        let mut names: Vec<&String> = indices.keys().collect();
        names.sort();
        for name in names {
            let idx = &indices[name];
            let s = idx["status"].as_str().unwrap_or("-");
            let colored_s = if no_color {
                s.to_string()
            } else {
                match s {
                    "green"  => s.green().to_string(),
                    "yellow" => s.yellow().to_string(),
                    "red"    => s.red().to_string(),
                    other    => other.to_string(),
                }
            };
            println!(
                "  {:<40} {:<8} {:>8} {:>8} {:>8}",
                truncate(name, 40),
                colored_s,
                idx["active_shards"].as_u64().unwrap_or(0),
                idx["active_primary_shards"].as_u64().unwrap_or(0),
                idx["unassigned_shards"].as_u64().unwrap_or(0),
            );
        }
    }
    println!();
}

fn colorize_unassigned(n: u64, no_color: bool) -> String {
    if no_color || n == 0 {
        return n.to_string();
    }
    n.to_string().red().bold().to_string()
}

// ── info ──────────────────────────────────────────────────────────────────────

async fn handle_info(client: &EsClient, cfg: &AppConfig) -> Result<()> {
    let resp = client.get("/").await?;

    match cfg.output {
        OutputFormat::Json => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => {
            println!();
            let name    = resp["name"].as_str().unwrap_or("-");
            let cluster = resp["cluster_name"].as_str().unwrap_or("-");
            let uuid    = resp["cluster_uuid"].as_str().unwrap_or("-");
            let version = resp.pointer("/version/number").and_then(Value::as_str).unwrap_or("-");
            let lucene  = resp.pointer("/version/lucene_version").and_then(Value::as_str).unwrap_or("-");
            let tagline = resp["tagline"].as_str().unwrap_or("");

            println!("  Node     : {}", name.bold());
            println!("  Cluster  : {}", cluster.bold());
            println!("  UUID     : {}", uuid.dimmed());
            println!("  ES ver.  : {}", version.green().bold());
            println!("  Lucene   : {}", lucene);
            println!("  Tagline  : {}", tagline.italic());
            println!();
        }
    }
    Ok(())
}

// ── stats ─────────────────────────────────────────────────────────────────────

async fn handle_stats(client: &EsClient, cfg: &AppConfig, summary: bool) -> Result<()> {
    let resp = client.get("/_cluster/stats").await?;

    if !summary {
        cfg.output.print(&resp, cfg.no_color);
        return Ok(());
    }

    // Summary view — pull out the most useful numbers
    match cfg.output {
        OutputFormat::Json => {
            let summary_val = build_stats_summary(&resp);
            cfg.output.print(&summary_val, cfg.no_color);
        }
        OutputFormat::Table => print_stats_table(&resp, cfg.no_color),
    }
    Ok(())
}

fn build_stats_summary(v: &Value) -> Value {
    serde_json::json!({
        "cluster_name":     v["cluster_name"],
        "status":           v["status"],
        "nodes":            v.pointer("/nodes/count/total"),
        "indices":          v.pointer("/indices/count"),
        "shards":           v.pointer("/indices/shards/total"),
        "docs":             v.pointer("/indices/docs/count"),
        "store_size":       v.pointer("/indices/store/size_in_bytes"),
        "jvm_heap_used":    v.pointer("/nodes/jvm/mem/heap_used_in_bytes"),
        "jvm_heap_max":     v.pointer("/nodes/jvm/mem/heap_max_in_bytes"),
        "os_mem_used":      v.pointer("/nodes/os/mem/used_in_bytes"),
        "os_mem_free":      v.pointer("/nodes/os/mem/free_in_bytes"),
    })
}

fn print_stats_table(v: &Value, _no_color: bool) {
    let fmt_bytes = |ptr: &str| -> String {
        v.pointer(ptr)
            .and_then(Value::as_u64)
            .map(human_bytes)
            .unwrap_or_else(|| "-".into())
    };

    println!();
    println!("  {} {}", "Cluster:".bold(), v["cluster_name"].as_str().unwrap_or("-"));
    println!();
    println!("  {:<28} {}", "Nodes:",         v.pointer("/nodes/count/total").and_then(Value::as_u64).unwrap_or(0));
    println!("  {:<28} {}", "Indices:",        v.pointer("/indices/count").and_then(Value::as_u64).unwrap_or(0));
    println!("  {:<28} {}", "Total shards:",   v.pointer("/indices/shards/total").and_then(Value::as_u64).unwrap_or(0));
    println!("  {:<28} {}", "Docs:",           v.pointer("/indices/docs/count").and_then(Value::as_u64).unwrap_or(0));
    println!("  {:<28} {}", "Store size:",     fmt_bytes("/indices/store/size_in_bytes"));
    println!("  {:<28} {}", "JVM heap used:",  fmt_bytes("/nodes/jvm/mem/heap_used_in_bytes"));
    println!("  {:<28} {}", "JVM heap max:",   fmt_bytes("/nodes/jvm/mem/heap_max_in_bytes"));
    println!("  {:<28} {}", "OS mem used:",    fmt_bytes("/nodes/os/mem/used_in_bytes"));
    println!("  {:<28} {}", "OS mem free:",    fmt_bytes("/nodes/os/mem/free_in_bytes"));
    println!();
}

// ── nodes ─────────────────────────────────────────────────────────────────────

async fn handle_nodes(
    client: &EsClient,
    cfg: &AppConfig,
    verbose: bool,
    role: Option<String>,
) -> Result<()> {
    // Use _nodes API for full detail, _cat/nodes for compact
    let path = if verbose {
        "/_nodes?filter_path=nodes.*.name,nodes.*.roles,nodes.*.ip,nodes.*.version,nodes.*.os,nodes.*.jvm,nodes.*.process".to_string()
    } else {
        "/_cat/nodes?format=json&h=name,ip,heap.percent,ram.percent,cpu,load_1m,load_5m,node.role,master,version".to_string()
    };

    let mut resp = client.get(&path).await?;

    // Filter by role if requested
    if let Some(ref filter_role) = role {
        resp = filter_nodes_by_role(resp, filter_role);
    }

    match cfg.output {
        OutputFormat::Json => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => {
            if verbose {
                print_nodes_verbose(&resp, cfg.no_color);
            } else {
                print_nodes_compact(&resp, cfg.no_color);
            }
        }
    }
    Ok(())
}

fn filter_nodes_by_role(resp: Value, role: &str) -> Value {
    match resp {
        Value::Array(arr) => {
            let filtered: Vec<Value> = arr
                .into_iter()
                .filter(|n| {
                    n["node.role"]
                        .as_str()
                        .map(|r| r.contains(&role[..1]))
                        .unwrap_or(false)
                })
                .collect();
            Value::Array(filtered)
        }
        // verbose _nodes response — filter object keys
        Value::Object(mut map) => {
            if let Some(Value::Object(nodes)) = map.get_mut("nodes") {
                nodes.retain(|_, v| {
                    v["roles"]
                        .as_array()
                        .map(|roles| roles.iter().any(|r| r.as_str() == Some(role)))
                        .unwrap_or(false)
                });
            }
            Value::Object(map)
        }
        other => other,
    }
}

fn print_nodes_compact(v: &Value, no_color: bool) {
    let arr = match v.as_array() {
        Some(a) => a,
        None => { println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()); return; }
    };

    println!();
    println!(
        "  {:<28} {:<16} {:>5} {:>5} {:>4} {:<6} {:<8} {}",
        "name", "ip", "heap%", "ram%", "cpu", "role", "master", "version"
    );
    println!("  {}", "─".repeat(90).dimmed());

    for node in arr {
        let heap  = node["heap.percent"].as_str().unwrap_or("-");
        let ram   = node["ram.percent"].as_str().unwrap_or("-");
        let cpu   = node["cpu"].as_str().unwrap_or("-");
        let role  = node["node.role"].as_str().unwrap_or("-");
        let master = node["master"].as_str().unwrap_or("-");
        let is_master = master == "*";

        let name_str = node["name"].as_str().unwrap_or("-");
        let name_fmt = if !no_color && is_master {
            format!("{} ★", name_str).green().bold().to_string()
        } else {
            name_str.to_string()
        };

        println!(
            "  {:<28} {:<16} {:>5} {:>5} {:>4} {:<6} {:<8} {}",
            truncate(&name_fmt, 28),
            node["ip"].as_str().unwrap_or("-"),
            colorize_percent(heap, no_color),
            colorize_percent(ram, no_color),
            cpu,
            role,
            if is_master { if no_color { "*".to_string() } else { "*".green().to_string() } } else { " ".to_string() },
            node["version"].as_str().unwrap_or("-"),
        );
    }
    println!();
}

fn print_nodes_verbose(v: &Value, _no_color: bool) {
    let nodes = match v.pointer("/nodes").and_then(Value::as_object) {
        Some(n) => n,
        None => { println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()); return; }
    };

    println!();
    for (_id, node) in nodes {
        let name    = node["name"].as_str().unwrap_or("-");
        let ip      = node["ip"].as_str().unwrap_or("-");
        let ver     = node["version"].as_str().unwrap_or("-");
        let roles: Vec<&str> = node["roles"]
            .as_array()
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        println!("  {} ({})", name.bold(), ip);
        println!("  {:<20} {}", "Version:", ver);
        println!("  {:<20} {}", "Roles:", roles.join(", "));

        if let Some(jvm) = node["jvm"].as_object() {
            let used = jvm.get("mem").and_then(|m| m.pointer("/heap_used_in_bytes")).and_then(Value::as_u64);
            let max  = jvm.get("mem").and_then(|m| m.pointer("/heap_max_in_bytes")).and_then(Value::as_u64);
            if let (Some(u), Some(m)) = (used, max) {
                let pct = (u as f64 / m as f64) * 100.0;
                println!("  {:<20} {} / {} ({:.1}%)", "JVM heap:", human_bytes(u), human_bytes(m), pct);
            }
        }
        if let Some(os) = node["os"].as_object() {
            let used = os.get("mem").and_then(|m| m["used_in_bytes"].as_u64());
            let free = os.get("mem").and_then(|m| m["free_in_bytes"].as_u64());
            if let (Some(u), Some(f)) = (used, free) {
                println!("  {:<20} used {} / free {}", "OS mem:", human_bytes(u), human_bytes(f));
            }
            if let Some(cpu) = os.get("cpu").and_then(|c| c["percent"].as_u64()) {
                println!("  {:<20} {}%", "CPU:", cpu);
            }
        }
        println!();
    }
}

// ── set ───────────────────────────────────────────────────────────────────────

async fn handle_set(
    client: &EsClient,
    cfg: &AppConfig,
    setting: &str,
    setting_type: &str,
) -> Result<()> {
    // Parse "key=value"
    let (key, value) = setting
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("setting must be in key=value format, got: {}", setting))?;

    // Try to parse value as JSON; fall back to string
    let val: Value = serde_json::from_str(value)
        .unwrap_or_else(|_| Value::String(value.to_string()));

    let body = serde_json::json!({ setting_type: { key: val } });
    let resp = client.put("/_cluster/settings", &body).await?;
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── allocation explain ────────────────────────────────────────────────────────

async fn handle_allocation_explain(
    client: &EsClient,
    cfg: &AppConfig,
    index: Option<String>,
    shard: Option<u32>,
    primary: Option<bool>,
) -> Result<()> {
    let body = if index.is_some() || shard.is_some() {
        let mut b = serde_json::json!({});
        if let Some(i) = index   { b["index"]   = Value::String(i); }
        if let Some(s) = shard   { b["shard"]   = Value::Number(s.into()); }
        if let Some(p) = primary { b["primary"] = Value::Bool(p); }
        b
    } else {
        Value::Null
    };

    let resp = if body.is_null() {
        client.get("/_cluster/allocation/explain").await?
    } else {
        client.post("/_cluster/allocation/explain", &body).await?
    };

    match cfg.output {
        OutputFormat::Json => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_allocation_explain(&resp),
    }
    Ok(())
}

fn print_allocation_explain(v: &Value) {
    println!();
    println!("  {} {}", "Index:".bold(), v["index"].as_str().unwrap_or("-"));
    println!("  {} {}", "Shard:".bold(), v["shard"].as_u64().unwrap_or(0));
    println!("  {} {}", "Primary:".bold(), v["primary"].as_bool().unwrap_or(false));
    println!("  {} {}", "State:".bold(),   v["unassigned_info"]["reason"].as_str().unwrap_or("-"));
    println!();

    if let Some(decisions) = v["node_allocation_decisions"].as_array() {
        println!("  {}", "── Node decisions ──".dimmed());
        println!("  {:<30} {:<12} {}", "node", "decision", "reason");
        println!("  {}", "─".repeat(70).dimmed());
        for d in decisions {
            let node     = d["node_name"].as_str().unwrap_or("-");
            let decision = d["node_decision"].as_str().unwrap_or("-");
            let reason   = d["deciders"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|x| x["explanation"].as_str())
                .unwrap_or("-");
            println!("  {:<30} {:<12} {}", truncate(node, 30), decision, reason);
        }
        println!();
    }
}

// ── reroute ───────────────────────────────────────────────────────────────────

async fn handle_reroute(
    client: &EsClient,
    cfg: &AppConfig,
    dry_run: bool,
    retry_failed: bool,
) -> Result<()> {
    let mut body = serde_json::json!({});
    if retry_failed {
        body["retry_failed"] = Value::Bool(true);
    }

    let path = if dry_run {
        "/_cluster/reroute?dry_run=true&explain=true"
    } else {
        "/_cluster/reroute?explain=true"
    };

    if dry_run {
        eprintln!("{} Dry-run mode — no changes will be applied.", "→".cyan());
    }

    let resp = client.post(path, &body).await?;
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Convert bytes to human-readable string (KB / MB / GB / TB)
fn human_bytes(bytes: u64) -> String {
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

/// Colorize a percent string: ≥90 = red, ≥75 = yellow, else default
fn colorize_percent(s: &str, no_color: bool) -> String {
    if no_color {
        return s.to_string();
    }
    let pct: u64 = s.parse().unwrap_or(0);
    if pct >= 90 {
        s.red().bold().to_string()
    } else if pct >= 75 {
        s.yellow().to_string()
    } else {
        s.to_string()
    }
}

/// Truncate a string to `max` chars, appending '…' if needed
fn truncate(s: &str, max: usize) -> String {
    // Strip ANSI codes for length check — use char count, not byte count
    let visible_len = strip_ansi(s).chars().count();
    if visible_len <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn strip_ansi(s: &str) -> String {
    // Simple ANSI escape stripper: remove ESC [ ... m sequences
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_alphabetic() { break; }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_bytes() {
        assert_eq!(human_bytes(0),                "0 B");
        assert_eq!(human_bytes(1023),             "1023 B");
        assert_eq!(human_bytes(1024),             "1.00 KB");
        assert_eq!(human_bytes(1024 * 1024),      "1.00 MB");
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world!", 8), "hello wo…");
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_strip_ansi() {
        let colored = "\x1b[32mgreen\x1b[0m";
        assert_eq!(strip_ansi(colored), "green");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn test_build_stats_summary() {
        let raw = serde_json::json!({
            "cluster_name": "test",
            "status": "green",
            "nodes": { "count": { "total": 3 } },
            "indices": {
                "count": 10,
                "shards": { "total": 20 },
                "docs":   { "count": 1000 },
                "store":  { "size_in_bytes": 1048576 }
            }
        });
        let summary = build_stats_summary(&raw);
        assert_eq!(summary["cluster_name"], "test");
        assert_eq!(summary["nodes"], 3);
        assert_eq!(summary["indices"], 10);
        assert_eq!(summary["docs"], 1000);
        assert_eq!(summary["store_size"], 1048576);
    }

    #[test]
    fn test_colorize_unassigned_zero() {
        // With no_color=true, should just be the number as string
        assert_eq!(colorize_unassigned(0, true), "0");
        assert_eq!(colorize_unassigned(5, true), "5");
    }

    #[test]
    fn test_filter_nodes_by_role_array() {
        let nodes = serde_json::json!([
            { "name": "node1", "node.role": "dim" },
            { "name": "node2", "node.role": "mr"  },
        ]);
        let filtered = filter_nodes_by_role(nodes, "master");
        let arr = filtered.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "node2");
    }

    #[test]
    fn test_set_parses_kv() {
        // Just verify the key=value split logic
        let setting = "cluster.routing.allocation.enable=all";
        let (key, value) = setting.split_once('=').unwrap();
        assert_eq!(key,   "cluster.routing.allocation.enable");
        assert_eq!(value, "all");
    }

    #[test]
    fn test_set_parses_numeric_value() {
        let value = "10";
        let val: serde_json::Value = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
        assert!(val.is_number());
    }

    #[test]
    fn test_set_parses_string_value() {
        let value = "all";
        let val: serde_json::Value = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
        assert_eq!(val, serde_json::Value::String("all".to_string()));
    }
}
