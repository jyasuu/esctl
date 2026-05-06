use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum AliasCommands {
    /// List aliases (optionally scoped to an index)
    List {
        #[arg(long)]
        index: Option<String>,
        /// Filter by alias name pattern
        #[arg(long)]
        name: Option<String>,
    },
    /// Add an alias to an index
    Add {
        alias: String,
        index: String,
        /// Optional filter query (file or - for stdin)
        #[arg(long)]
        filter: Option<String>,
        /// Routing value for search
        #[arg(long)]
        routing: Option<String>,
        /// Mark this as a write alias
        #[arg(long)]
        is_write_index: bool,
    },
    /// Remove an alias from an index
    Remove {
        alias: String,
        index: String,
    },
    /// Move an alias from one index to another atomically
    Move {
        alias: String,
        from: String,
        to: String,
        /// Mark the target as the write index
        #[arg(long)]
        is_write_index: bool,
    },
    /// Apply a set of alias actions from a JSON body (file or - for stdin)
    Actions {
        body: String,
    },
}

pub async fn handle(cmd: AliasCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        AliasCommands::List { index, name }           => handle_list(client, cfg, index, name).await,
        AliasCommands::Add { alias, index, filter, routing, is_write_index } =>
            handle_add(client, cfg, &alias, &index, filter.as_deref(), routing.as_deref(), is_write_index).await,
        AliasCommands::Remove { alias, index }        => handle_remove(client, cfg, &alias, &index).await,
        AliasCommands::Move { alias, from, to, is_write_index } =>
            handle_move(client, cfg, &alias, &from, &to, is_write_index).await,
        AliasCommands::Actions { body }               => handle_actions(client, cfg, &body).await,
    }
}

async fn handle_list(
    client: &EsClient, cfg: &AppConfig,
    index: Option<String>, name: Option<String>,
) -> Result<()> {
    let path = match (&index, &name) {
        (Some(i), Some(n)) => format!("/{}/_alias/{}", i, n),
        (Some(i), None)    => format!("/{}/_alias", i),
        (None,    Some(n)) => format!("/_alias/{}", n),
        (None,    None)    => "/_cat/aliases?format=json".to_string(),
    };

    let resp = client.get(&path).await?;

    // _cat/aliases returns an array; _alias returns an object — normalise
    let normalised = normalise_alias_response(resp, index.as_deref());

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&normalised, cfg.no_color),
        OutputFormat::Table => print_alias_table(&normalised, cfg.no_color),
    }
    Ok(())
}

async fn handle_add(
    client: &EsClient, cfg: &AppConfig,
    alias: &str, index: &str,
    filter: Option<&str>, routing: Option<&str>, is_write_index: bool,
) -> Result<()> {
    let mut action = serde_json::json!({
        "index": index,
        "alias": alias,
    });
    if let Some(f) = filter {
        action["filter"] = stdin::read_json(f).await?;
    }
    if let Some(r) = routing {
        action["routing"] = Value::String(r.into());
    }
    if is_write_index {
        action["is_write_index"] = Value::Bool(true);
    }

    let body = serde_json::json!({ "actions": [{ "add": action }] });
    eprintln!("{} Adding alias '{}' → '{}'…", "→".cyan(), alias.bold(), index.bold());
    let resp = client.post("/_aliases", &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Alias added.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_remove(
    client: &EsClient, cfg: &AppConfig, alias: &str, index: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "actions": [{ "remove": { "index": index, "alias": alias } }]
    });
    eprintln!("{} Removing alias '{}' from '{}'…", "→".cyan(), alias.bold(), index.bold());
    let resp = client.post("/_aliases", &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Alias removed.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_move(
    client: &EsClient, cfg: &AppConfig,
    alias: &str, from: &str, to: &str, is_write_index: bool,
) -> Result<()> {
    let mut add_action = serde_json::json!({ "index": to, "alias": alias });
    if is_write_index {
        add_action["is_write_index"] = Value::Bool(true);
    }
    let body = serde_json::json!({
        "actions": [
            { "remove": { "index": from, "alias": alias } },
            { "add":    add_action },
        ]
    });
    eprintln!(
        "{} Moving alias '{}': '{}' → '{}'…",
        "→".cyan(), alias.bold(), from.bold(), to.bold()
    );
    let resp = client.post("/_aliases", &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Alias moved.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_actions(client: &EsClient, cfg: &AppConfig, body: &str) -> Result<()> {
    let payload = stdin::read_json(body).await?;
    let resp = client.post("/_aliases", &payload).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Alias actions applied.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Convert both response shapes to a flat array of alias records.
pub fn normalise_alias_response(resp: Value, _index: Option<&str>) -> Value {
    match &resp {
        // Already an array (_cat/aliases)
        Value::Array(_) => resp,
        // Object shape from _alias API: {"index": {"aliases": {"alias": {...}}}}
        Value::Object(map) => {
            let mut rows = Vec::new();
            for (idx_name, idx_val) in map {
                if let Some(aliases) = idx_val["aliases"].as_object() {
                    for (alias_name, alias_cfg) in aliases {
                        rows.push(serde_json::json!({
                            "alias":          alias_name,
                            "index":          idx_name,
                            "filter":         alias_cfg.get("filter").map(|_| "yes").unwrap_or("-"),
                            "routing.index":  alias_cfg["index_routing"].as_str().unwrap_or("-"),
                            "routing.search": alias_cfg["search_routing"].as_str().unwrap_or("-"),
                            "is_write_index": alias_cfg["is_write_index"].as_bool()
                                                .map(|b| if b { "yes" } else { "no" })
                                                .unwrap_or("-"),
                        }));
                    }
                }
            }
            rows.sort_by(|a, b| {
                let ak = format!("{}{}", a["alias"].as_str().unwrap_or(""), a["index"].as_str().unwrap_or(""));
                let bk = format!("{}{}", b["alias"].as_str().unwrap_or(""), b["index"].as_str().unwrap_or(""));
                ak.cmp(&bk)
            });
            Value::Array(rows)
        }
        _ => resp,
    }
}

fn print_alias_table(v: &Value, _no_color: bool) {
    let arr = match v.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => { println!("No aliases found."); return; }
    };

    println!();
    println!(
        "  {:<30} {:<40} {:<8} {:<10}",
        "alias".dimmed(), "index".dimmed(), "filter".dimmed(), "write".dimmed()
    );
    println!("  {}", "─".repeat(92).dimmed());

    for row in arr {
        let alias  = row["alias"].as_str().unwrap_or(row["name"].as_str().unwrap_or("-"));
        let index  = row["index"].as_str().unwrap_or("-");
        let filter = row["filter"].as_str().unwrap_or("-");
        let write  = row["is_write_index"].as_str().unwrap_or("-");

        println!(
            "  {:<30} {:<40} {:<8} {:<10}",
            truncate(alias, 30),
            truncate(index, 40),
            filter,
            write,
        );
    }
    println!();
    println!("  {} alias(es)", arr.len());
    println!();
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() }
    else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalise_array_passthrough() {
        let data = serde_json::json!([
            {"alias":"my-alias","index":"my-index"}
        ]);
        let result = normalise_alias_response(data.clone(), None);
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_normalise_object_shape() {
        let data = serde_json::json!({
            "my-index": {
                "aliases": {
                    "my-alias": {},
                    "another-alias": { "is_write_index": true },
                }
            }
        });
        let result = normalise_alias_response(data, None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|r| r["alias"] == "my-alias"));
        assert!(arr.iter().any(|r| r["alias"] == "another-alias"));
    }

    #[test]
    fn test_normalise_object_write_index() {
        let data = serde_json::json!({
            "idx": {
                "aliases": {
                    "write-alias": { "is_write_index": true }
                }
            }
        });
        let result = normalise_alias_response(data, None);
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["is_write_index"], "yes");
    }

    #[test]
    fn test_normalise_multi_index() {
        let data = serde_json::json!({
            "idx-1": { "aliases": { "shared": {} } },
            "idx-2": { "aliases": { "shared": {} } },
        });
        let result = normalise_alias_response(data, None);
        assert_eq!(result.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_normalise_sorted() {
        let data = serde_json::json!({
            "z-index": { "aliases": { "z-alias": {} } },
            "a-index": { "aliases": { "a-alias": {} } },
        });
        let result = normalise_alias_response(data, None);
        let arr = result.as_array().unwrap();
        // sorted by alias+index key
        assert_eq!(arr[0]["alias"], "a-alias");
        assert_eq!(arr[1]["alias"], "z-alias");
    }

    #[test]
    fn test_normalise_empty_aliases() {
        let data = serde_json::json!({ "idx": { "aliases": {} } });
        let result = normalise_alias_response(data, None);
        assert_eq!(result.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10),      "hello");
        assert_eq!(truncate("hello world!", 8),"hello wo…");
        assert_eq!(truncate("", 5),            "");
    }

    #[test]
    fn test_move_alias_body_structure() {
        let from  = "old-index";
        let to    = "new-index";
        let alias = "my-alias";
        let body  = serde_json::json!({
            "actions": [
                { "remove": { "index": from, "alias": alias } },
                { "add":    { "index": to,   "alias": alias } },
            ]
        });
        let actions = body["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["remove"]["index"], from);
        assert_eq!(actions[1]["add"]["index"], to);
    }
}
