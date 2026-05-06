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
pub enum SnapshotCommands {
    /// Repository management
    #[command(subcommand)]
    Repo(RepoCommands),

    /// List snapshots in a repository
    List {
        repo: String,
        /// Filter by snapshot name pattern
        #[arg(default_value = "_all")]
        pattern: String,
        /// Show verbose snapshot details
        #[arg(long)]
        verbose: bool,
    },

    /// Create a snapshot
    Create(SnapshotCreateArgs),

    /// Restore a snapshot
    Restore(SnapshotRestoreArgs),

    /// Delete one or more snapshots
    Delete {
        repo: String,
        /// Snapshot name(s) — space-separated
        #[arg(required = true)]
        names: Vec<String>,
    },

    /// Show snapshot status (in-progress details)
    Status {
        repo: Option<String>,
        name: Option<String>,
    },

    /// Clone a snapshot to another repo
    Clone {
        repo: String,
        name: String,
        /// Target repository
        #[arg(long)]
        target_repo: String,
        /// Target snapshot name (defaults to same name)
        #[arg(long)]
        target_name: Option<String>,
        /// Indices to clone (default: all in snapshot)
        #[arg(long)]
        indices: Option<String>,
    },

    /// Verify a repository (checks all nodes can access it)
    Verify { repo: String },

    /// Clean up unused data in a repository
    Cleanup { repo: String },

    /// Get repository stats / integrity info
    RepoStats { repo: String },
}

#[derive(Subcommand, Debug)]
pub enum RepoCommands {
    /// List all snapshot repositories
    List,
    /// Create or update a snapshot repository
    Create(RepoCreateArgs),
    /// Delete a repository (does NOT delete snapshots)
    Delete {
        name: String,
        #[arg(long)]
        yes: bool,
    },
    /// Get repository details
    Get { name: String },
    /// Verify repository accessibility from all nodes
    Verify { name: String },
}

#[derive(Args, Debug)]
pub struct RepoCreateArgs {
    pub name: String,
    /// Repository type: fs | s3 | gcs | azure | hdfs
    #[arg(long, default_value = "fs")]
    pub r#type: String,
    /// Repository settings JSON (file or - for stdin)
    #[arg(long)]
    pub settings: String,
    /// Verify the repository after creation
    #[arg(long)]
    pub verify: bool,
}

#[derive(Args, Debug)]
pub struct SnapshotCreateArgs {
    pub repo: String,
    pub name: String,
    /// Comma-separated index pattern(s) to include
    #[arg(long)]
    pub indices: Option<String>,
    /// Include global cluster state
    #[arg(long)]
    pub include_global_state: bool,
    /// Ignore unavailable indices instead of failing
    #[arg(long)]
    pub ignore_unavailable: bool,
    /// Allow partial snapshots (some shards may be missing)
    #[arg(long)]
    pub partial: bool,
    /// Wait for snapshot to complete before returning
    #[arg(long)]
    pub wait: bool,
    /// Additional metadata (file or - for stdin)
    #[arg(long)]
    pub metadata: Option<String>,
}

#[derive(Args, Debug)]
pub struct SnapshotRestoreArgs {
    pub repo: String,
    pub name: String,
    /// Comma-separated index pattern(s) to restore
    #[arg(long)]
    pub indices: Option<String>,
    /// Rename pattern (regex) for restored index names
    #[arg(long)]
    pub rename_pattern: Option<String>,
    /// Rename replacement string
    #[arg(long)]
    pub rename_replacement: Option<String>,
    /// Restore cluster state
    #[arg(long)]
    pub include_global_state: bool,
    /// Ignore unavailable indices
    #[arg(long)]
    pub ignore_unavailable: bool,
    /// Allow partial restore
    #[arg(long)]
    pub partial: bool,
    /// Index settings overrides JSON (file or - for stdin)
    #[arg(long)]
    pub index_settings: Option<String>,
    /// Wait for restore to complete
    #[arg(long)]
    pub wait: bool,
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn handle(cmd: SnapshotCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        SnapshotCommands::Repo(r)      => handle_repo(r, client, cfg).await,
        SnapshotCommands::List { repo, pattern, verbose } =>
            handle_list(client, cfg, &repo, &pattern, verbose).await,
        SnapshotCommands::Create(a)    => handle_create(client, cfg, a).await,
        SnapshotCommands::Restore(a)   => handle_restore(client, cfg, a).await,
        SnapshotCommands::Delete { repo, names } =>
            handle_delete(client, cfg, &repo, &names).await,
        SnapshotCommands::Status { repo, name } =>
            handle_status(client, cfg, repo.as_deref(), name.as_deref()).await,
        SnapshotCommands::Clone { repo, name, target_repo, target_name, indices } =>
            handle_clone(client, cfg, &repo, &name, &target_repo, target_name.as_deref(), indices.as_deref()).await,
        SnapshotCommands::Verify { repo } => handle_verify_snapshot(client, cfg, &repo).await,
        SnapshotCommands::Cleanup { repo } => handle_cleanup(client, cfg, &repo).await,
        SnapshotCommands::RepoStats { repo } => handle_repo_stats(client, cfg, &repo).await,
    }
}

// ── repo ──────────────────────────────────────────────────────────────────────

async fn handle_repo(cmd: RepoCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        RepoCommands::List => {
            let resp = client.get("/_snapshot/_all").await?;
            match cfg.output {
                OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
                OutputFormat::Table => print_repo_table(&resp, cfg.no_color),
            }
        }
        RepoCommands::Create(args) => {
            let settings = stdin::read_json(&args.settings).await?;
            let body = serde_json::json!({ "type": args.r#type, "settings": settings });
            eprintln!("{} Creating repository '{}'…", "→".cyan(), args.name.bold());
            let resp = client.put(&format!("/_snapshot/{}", args.name), &body).await?;
            if resp["acknowledged"].as_bool().unwrap_or(false) {
                eprintln!("{} Repository '{}' created.", "✓".green(), args.name.bold());
            }
            if args.verify {
                eprintln!("{} Verifying repository…", "→".cyan());
                let vresp = client
                    .post(&format!("/_snapshot/{}/_verify", args.name), &Value::Null)
                    .await?;
                print_verify_result(&vresp, cfg.no_color);
            }
            cfg.output.print(&resp, cfg.no_color);
        }
        RepoCommands::Delete { name, yes } => {
            if !yes {
                eprint!(
                    "{} Delete repository '{}' (snapshots will remain)? [y/N] ",
                    "⚠".yellow(), name.bold()
                );
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(());
                }
            }
            let resp = client.delete(&format!("/_snapshot/{}", name)).await?;
            if resp["acknowledged"].as_bool().unwrap_or(false) {
                eprintln!("{} Repository '{}' deleted.", "✓".green(), name.bold());
            }
            cfg.output.print(&resp, cfg.no_color);
        }
        RepoCommands::Get { name } => {
            let resp = client.get(&format!("/_snapshot/{}", name)).await?;
            match cfg.output {
                OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
                OutputFormat::Table => print_repo_table(&resp, cfg.no_color),
            }
        }
        RepoCommands::Verify { name } => {
            eprintln!("{} Verifying repository '{}'…", "→".cyan(), name.bold());
            let resp = client
                .post(&format!("/_snapshot/{}/_verify", name), &Value::Null)
                .await?;
            print_verify_result(&resp, cfg.no_color);
            cfg.output.print(&resp, cfg.no_color);
        }
    }
    Ok(())
}

// ── list ──────────────────────────────────────────────────────────────────────

async fn handle_list(
    client: &EsClient, cfg: &AppConfig,
    repo: &str, pattern: &str, verbose: bool,
) -> Result<()> {
    let path = if verbose {
        format!("/_snapshot/{}/{}?verbose=true", repo, pattern)
    } else {
        format!("/_snapshot/{}/{}", repo, pattern)
    };
    let resp = client.get(&path).await?;

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_snapshot_list(&resp, cfg.no_color),
    }
    Ok(())
}

// ── create ────────────────────────────────────────────────────────────────────

async fn handle_create(client: &EsClient, cfg: &AppConfig, args: SnapshotCreateArgs) -> Result<()> {
    let mut body = serde_json::json!({
        "include_global_state": args.include_global_state,
        "ignore_unavailable":   args.ignore_unavailable,
        "partial":              args.partial,
    });
    if let Some(ref idx) = args.indices {
        body["indices"] = Value::String(idx.clone());
    }
    if let Some(ref meta_src) = args.metadata {
        body["metadata"] = stdin::read_json(meta_src).await?;
    }

    let wait_param = if args.wait { "?wait_for_completion=true" } else { "" };
    let path = format!("/_snapshot/{}/{}{}", args.repo, args.name, wait_param);

    eprintln!(
        "{} Creating snapshot '{}/{}'{} …",
        "→".cyan(),
        args.repo.bold(),
        args.name.bold(),
        if args.wait { " (waiting)" } else { " (async)" },
    );

    let resp = client.put(&path, &body).await?;

    if args.wait {
        print_snapshot_result(&resp, cfg.no_color);
    } else {
        eprintln!("{} Snapshot started (use `esctl snapshot status` to monitor).", "→".cyan());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── restore ───────────────────────────────────────────────────────────────────

async fn handle_restore(client: &EsClient, cfg: &AppConfig, args: SnapshotRestoreArgs) -> Result<()> {
    let mut body = serde_json::json!({
        "include_global_state": args.include_global_state,
        "ignore_unavailable":   args.ignore_unavailable,
        "partial":              args.partial,
    });
    if let Some(ref idx) = args.indices {
        body["indices"] = Value::String(idx.clone());
    }
    if let Some(ref rp) = args.rename_pattern {
        body["rename_pattern"] = Value::String(rp.clone());
    }
    if let Some(ref rr) = args.rename_replacement {
        body["rename_replacement"] = Value::String(rr.clone());
    }
    if let Some(ref is_src) = args.index_settings {
        body["index_settings"] = stdin::read_json(is_src).await?;
    }

    let wait_param = if args.wait { "?wait_for_completion=true" } else { "" };
    let path = format!("/_snapshot/{}/{}/_restore{}", args.repo, args.name, wait_param);

    eprintln!(
        "{} Restoring snapshot '{}/{}'{} …",
        "→".cyan(),
        args.repo.bold(),
        args.name.bold(),
        if args.wait { " (waiting)" } else { " (async)" },
    );

    let resp = client.post(&path, &body).await?;

    if args.wait {
        if let Some(snap) = resp.get("snapshot") {
            let state = snap["state"].as_str().unwrap_or("-");
            let shard_ok = snap.pointer("/shards/successful").and_then(Value::as_u64).unwrap_or(0);
            let shard_fail = snap.pointer("/shards/failed").and_then(Value::as_u64).unwrap_or(0);
            if shard_fail == 0 {
                eprintln!("{} Restore complete — state: {}, shards: {}", "✓".green(), state, shard_ok);
            } else {
                eprintln!("{} Restore done with failures — state: {}, ok: {}, failed: {}", "⚠".yellow(), state, shard_ok, shard_fail);
            }
        }
    } else {
        eprintln!("{} Restore started (use `esctl snapshot status` to monitor).", "→".cyan());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── delete ────────────────────────────────────────────────────────────────────

async fn handle_delete(
    client: &EsClient, cfg: &AppConfig,
    repo: &str, names: &[String],
) -> Result<()> {
    let names_str = names.join(",");
    eprintln!(
        "{} Deleting snapshot(s) '{}/{}' …",
        "→".cyan(), repo.bold(), names_str.bold()
    );
    let path = format!("/_snapshot/{}/{}", repo, names_str);
    let resp = client.delete(&path).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Deleted.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── status ────────────────────────────────────────────────────────────────────

async fn handle_status(
    client: &EsClient, cfg: &AppConfig,
    repo: Option<&str>, name: Option<&str>,
) -> Result<()> {
    let path = match (repo, name) {
        (Some(r), Some(n)) => format!("/_snapshot/{}/{}/_status", r, n),
        (Some(r), None)    => format!("/_snapshot/{}/_status", r),
        _                  => "/_snapshot/_status".to_string(),
    };
    let resp = client.get(&path).await?;
    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_status_table(&resp, cfg.no_color),
    }
    Ok(())
}

// ── clone ─────────────────────────────────────────────────────────────────────

async fn handle_clone(
    client: &EsClient, cfg: &AppConfig,
    repo: &str, name: &str,
    target_repo: &str, target_name: Option<&str>, indices: Option<&str>,
) -> Result<()> {
    let target = target_name.unwrap_or(name);
    let mut body = serde_json::json!({ "indices": indices.unwrap_or("*") });
    // Clone API: PUT /_snapshot/<repo>/<name>/_clone/<target>
    // with body pointing to target_repository
    body["target_repository"] = Value::String(target_repo.to_string());

    eprintln!(
        "{} Cloning '{}/{}' → '{}/{}'…",
        "→".cyan(), repo.bold(), name.bold(), target_repo.bold(), target.bold()
    );
    let path = format!("/_snapshot/{}/{}/_clone/{}", repo, name, target);
    let resp = client.put(&path, &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Clone complete.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── verify / cleanup / stats ──────────────────────────────────────────────────

async fn handle_verify_snapshot(client: &EsClient, cfg: &AppConfig, repo: &str) -> Result<()> {
    eprintln!("{} Verifying repository '{}'…", "→".cyan(), repo.bold());
    let resp = client
        .post(&format!("/_snapshot/{}/_verify", repo), &Value::Null)
        .await?;
    print_verify_result(&resp, cfg.no_color);
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_cleanup(client: &EsClient, cfg: &AppConfig, repo: &str) -> Result<()> {
    eprintln!("{} Cleaning up repository '{}'…", "→".cyan(), repo.bold());
    let resp = client
        .post(&format!("/_snapshot/{}/_cleanup", repo), &Value::Null)
        .await?;
    if let Some(results) = resp.get("results") {
        let freed = results.pointer("/freed_bytes").and_then(Value::as_u64).unwrap_or(0);
        let blobs = results.pointer("/deleted_blobs").and_then(Value::as_u64).unwrap_or(0);
        eprintln!(
            "{} Cleanup complete: {} blob(s) deleted, {} freed.",
            "✓".green(), blobs, human_bytes(freed)
        );
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_repo_stats(client: &EsClient, cfg: &AppConfig, repo: &str) -> Result<()> {
    let resp = client.get(&format!("/_snapshot/{}/_stats", repo)).await?;
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── table formatters ──────────────────────────────────────────────────────────

fn print_repo_table(resp: &Value, _no_color: bool) {
    let obj = match resp.as_object() {
        Some(o) if !o.is_empty() => o,
        _ => { println!("No repositories configured."); return; }
    };
    println!();
    println!("  {:<30} {:<10} {}", "name".dimmed(), "type".dimmed(), "location".dimmed());
    println!("  {}", "─".repeat(72).dimmed());
    let mut names: Vec<&String> = obj.keys().collect();
    names.sort();
    for name in names {
        let repo  = &obj[name];
        let rtype = repo["type"].as_str().unwrap_or("-");
        let loc   = repo.pointer("/settings/location")
            .or_else(|| repo.pointer("/settings/bucket"))
            .and_then(Value::as_str)
            .unwrap_or("-");
        println!("  {:<30} {:<10} {}", name, rtype, loc);
    }
    println!();
}

fn print_snapshot_list(resp: &Value, no_color: bool) {
    let snaps = match resp["snapshots"].as_array() {
        Some(a) if !a.is_empty() => a,
        _ => { println!("No snapshots found."); return; }
    };
    println!();
    println!(
        "  {:<32} {:<10} {:<26} {:>8} {:>8}",
        "snapshot".dimmed(), "state".dimmed(), "start_time".dimmed(),
        "shards".dimmed(), "indices".dimmed()
    );
    println!("  {}", "─".repeat(90).dimmed());
    for snap in snaps {
        let name  = snap["snapshot"].as_str().unwrap_or("-");
        let state = snap["state"].as_str().unwrap_or("-");
        let start = snap["start_time"].as_str().unwrap_or("-");
        let shards_ok  = snap.pointer("/shards/successful").and_then(Value::as_u64).unwrap_or(0);
        let shards_tot = snap.pointer("/shards/total").and_then(Value::as_u64).unwrap_or(0);
        let idx_count  = snap["indices"].as_array().map(|a| a.len()).unwrap_or(0);

        let state_str = if no_color { state.to_string() } else {
            match state {
                "SUCCESS" => state.green().to_string(),
                "FAILED"  => state.red().bold().to_string(),
                "PARTIAL" => state.yellow().to_string(),
                "IN_PROGRESS" => state.cyan().to_string(),
                other     => other.to_string(),
            }
        };
        println!(
            "  {:<32} {:<10} {:<26} {:>8} {:>8}",
            truncate(name, 32),
            state_str,
            truncate(start, 26),
            format!("{}/{}", shards_ok, shards_tot),
            idx_count,
        );
    }
    println!();
    println!("  {} snapshot(s)", snaps.len());
    println!();
}

fn print_snapshot_result(resp: &Value, no_color: bool) {
    if let Some(snap) = resp.get("snapshot") {
        let state  = snap["state"].as_str().unwrap_or("-");
        let name   = snap["snapshot"].as_str().unwrap_or("-");
        let ok     = snap.pointer("/shards/successful").and_then(Value::as_u64).unwrap_or(0);
        let failed = snap.pointer("/shards/failed").and_then(Value::as_u64).unwrap_or(0);
        let dur    = snap["duration_in_millis"].as_u64().unwrap_or(0);

        let state_str = if no_color { state.to_string() } else {
            match state {
                "SUCCESS" => state.green().bold().to_string(),
                "FAILED"  => state.red().bold().to_string(),
                other     => other.to_string(),
            }
        };
        eprintln!(
            "{} Snapshot '{}': {} — shards ok={}, failed={}, took {}ms",
            if failed == 0 { "✓".green() } else { "⚠".yellow() },
            name, state_str, ok, failed, dur
        );
    }
}

fn print_status_table(resp: &Value, no_color: bool) {
    let snaps = match resp["snapshots"].as_array() {
        Some(a) if !a.is_empty() => a,
        _ => { println!("No snapshots in progress."); return; }
    };
    println!();
    for snap in snaps {
        let repo  = snap["repository"].as_str().unwrap_or("-");
        let name  = snap["snapshot"].as_str().unwrap_or("-");
        let state = snap["state"].as_str().unwrap_or("-");

        let state_str = if no_color { state.to_string() } else {
            match state {
                "SUCCESS"     => state.green().to_string(),
                "FAILED"      => state.red().bold().to_string(),
                "IN_PROGRESS" => state.cyan().to_string(),
                other         => other.to_string(),
            }
        };
        println!("  {}/{} — {}", repo.bold(), name.bold(), state_str);

        // Shard stats
        if let Some(shards) = snap["shards_stats"].as_object() {
            let done  = shards.get("done").and_then(Value::as_u64).unwrap_or(0);
            let total = shards.get("total").and_then(Value::as_u64).unwrap_or(0);
            let fail  = shards.get("failed").and_then(Value::as_u64).unwrap_or(0);
            let pct   = if total > 0 { (done * 100) / total } else { 0 };
            println!("  {:<20} {}/{} shards ({}%)", "Progress:", done, total, pct);
            if fail > 0 {
                println!("  {:<20} {}", "Failed shards:", fail);
            }
        }

        // Stats
        if let Some(stats) = snap.get("stats") {
            let processed = stats.pointer("/incremental/size_in_bytes").and_then(Value::as_u64).unwrap_or(0);
            let total_bytes = stats.pointer("/total/size_in_bytes").and_then(Value::as_u64).unwrap_or(0);
            println!("  {:<20} {} / {}", "Data:", human_bytes(processed), human_bytes(total_bytes));
        }
        println!();
    }
}

fn print_verify_result(resp: &Value, _no_color: bool) {
    if let Some(nodes) = resp["nodes"].as_object() {
        let count = nodes.len();
        eprintln!("{} Repository accessible from {} node(s).", "✓".green(), count);
        for (node_id, node) in nodes {
            let name = node["name"].as_str().unwrap_or(node_id);
            eprintln!("  ✓ {}", name);
        }
    }
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
    fn test_human_bytes() {
        assert_eq!(human_bytes(0),                 "0 B");
        assert_eq!(human_bytes(1024),              "1.00 KB");
        assert_eq!(human_bytes(1_048_576),         "1.00 MB");
        assert_eq!(human_bytes(1_073_741_824),     "1.00 GB");
        assert_eq!(human_bytes(1_099_511_627_776), "1.00 TB");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10),       "hello");
        assert_eq!(truncate("hello world!", 8), "hello wo…");
        assert_eq!(truncate("", 5),             "");
    }

    #[test]
    fn test_snapshot_create_body_indices() {
        let mut body = serde_json::json!({
            "include_global_state": false,
            "ignore_unavailable":   true,
            "partial":              false,
        });
        let indices = "logs-*,events-*";
        body["indices"] = Value::String(indices.to_string());
        assert_eq!(body["indices"], indices);
        assert_eq!(body["ignore_unavailable"], true);
    }

    #[test]
    fn test_snapshot_create_no_indices() {
        let body = serde_json::json!({
            "include_global_state": false,
            "ignore_unavailable":   false,
            "partial":              false,
        });
        assert!(body.get("indices").is_none());
    }

    #[test]
    fn test_snapshot_restore_rename() {
        let mut body = serde_json::json!({});
        let rp = "(.+)";
        let rr = "restored_$1";
        body["rename_pattern"]     = Value::String(rp.into());
        body["rename_replacement"] = Value::String(rr.into());
        assert_eq!(body["rename_pattern"],     rp);
        assert_eq!(body["rename_replacement"], rr);
    }

    #[test]
    fn test_delete_multi_snapshot_path() {
        let repo  = "my-repo";
        let names = vec!["snap-1".to_string(), "snap-2".to_string()];
        let path  = format!("/_snapshot/{}/{}", repo, names.join(","));
        assert_eq!(path, "/_snapshot/my-repo/snap-1,snap-2");
    }

    #[test]
    fn test_status_path_all() {
        let path = match (None::<&str>, None::<&str>) {
            (Some(r), Some(n)) => format!("/_snapshot/{}/{}/_status", r, n),
            (Some(r), None)    => format!("/_snapshot/{}/_status", r),
            _                  => "/_snapshot/_status".to_string(),
        };
        assert_eq!(path, "/_snapshot/_status");
    }

    #[test]
    fn test_status_path_repo_only() {
        let path = match (Some("my-repo"), None::<&str>) {
            (Some(r), Some(n)) => format!("/_snapshot/{}/{}/_status", r, n),
            (Some(r), None)    => format!("/_snapshot/{}/_status", r),
            _                  => "/_snapshot/_status".to_string(),
        };
        assert_eq!(path, "/_snapshot/my-repo/_status");
    }

    #[test]
    fn test_status_path_repo_and_name() {
        let path = match (Some("my-repo"), Some("snap-1")) {
            (Some(r), Some(n)) => format!("/_snapshot/{}/{}/_status", r, n),
            (Some(r), None)    => format!("/_snapshot/{}/_status", r),
            _                  => "/_snapshot/_status".to_string(),
        };
        assert_eq!(path, "/_snapshot/my-repo/snap-1/_status");
    }

    #[test]
    fn test_wait_param() {
        let wait_true  = if true  { "?wait_for_completion=true" } else { "" };
        let wait_false = if false { "?wait_for_completion=true" } else { "" };
        assert_eq!(wait_true,  "?wait_for_completion=true");
        assert_eq!(wait_false, "");
    }

    #[test]
    fn test_list_path_pattern() {
        let repo    = "my-repo";
        let pattern = "snap-2024-*";
        let path    = format!("/_snapshot/{}/{}", repo, pattern);
        assert_eq!(path, "/_snapshot/my-repo/snap-2024-*");
    }

    #[test]
    fn test_print_repo_table_empty() {
        // Should not panic on empty object
        print_repo_table(&serde_json::json!({}), true);
    }

    #[test]
    fn test_print_snapshot_list_empty() {
        let resp = serde_json::json!({ "snapshots": [] });
        print_snapshot_list(&resp, true);
    }
}
