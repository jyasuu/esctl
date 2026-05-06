use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum MappingCommands {
    /// Get the mapping for an index
    Get {
        index: String,
        /// Show only field types in a flat list
        #[arg(long)]
        flat: bool,
    },
    /// Add or update fields in the mapping (file or - for stdin)
    Put {
        index: String,
        #[arg(long)]
        body: String,
    },
    /// Show all field names and their types (flat view)
    Fields { index: String },
}

pub async fn handle(cmd: MappingCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        MappingCommands::Get { index, flat } => handle_get(client, cfg, &index, flat).await,
        MappingCommands::Put { index, body } => handle_put(client, cfg, &index, &body).await,
        MappingCommands::Fields { index }    => handle_fields(client, cfg, &index).await,
    }
}

async fn handle_get(client: &EsClient, cfg: &AppConfig, index: &str, flat: bool) -> Result<()> {
    let resp = client.get(&format!("/{}/_mapping", index)).await?;
    if flat {
        let fields = flatten_mapping(&resp, index);
        match cfg.output {
            OutputFormat::Json  => cfg.output.print(&Value::Array(fields.iter().map(|(f,t)| {
                serde_json::json!({"field": f, "type": t})
            }).collect()), cfg.no_color),
            OutputFormat::Table => print_fields_table(&fields, cfg.no_color),
        }
    } else {
        cfg.output.print(&resp, cfg.no_color);
    }
    Ok(())
}

async fn handle_put(client: &EsClient, cfg: &AppConfig, index: &str, body: &str) -> Result<()> {
    let payload = stdin::read_json(body).await?;
    eprintln!("{} Updating mapping for '{}'…", "→".cyan(), index.bold());
    let resp = client.put(&format!("/{}/_mapping", index), &payload).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Mapping updated.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_fields(client: &EsClient, cfg: &AppConfig, index: &str) -> Result<()> {
    let resp = client.get(&format!("/{}/_mapping", index)).await?;
    let fields = flatten_mapping(&resp, index);
    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&Value::Array(fields.iter().map(|(f,t)| {
            serde_json::json!({"field": f, "type": t})
        }).collect()), cfg.no_color),
        OutputFormat::Table => print_fields_table(&fields, cfg.no_color),
    }
    Ok(())
}

/// Flatten nested mapping properties into (dotted.path, type) pairs.
pub fn flatten_mapping(resp: &Value, index: &str) -> Vec<(String, String)> {
    let props_path = format!("/{}/mappings/properties", index);
    let props = match resp.pointer(&props_path).and_then(Value::as_object) {
        Some(p) => p,
        None    => return vec![],
    };
    let mut out = Vec::new();
    flatten_props(props, "", &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn flatten_props(
    props: &serde_json::Map<String, Value>,
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    for (name, def) in props {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let ftype = def["type"].as_str().unwrap_or("object").to_string();
        out.push((full.clone(), ftype));

        // Recurse into nested properties
        if let Some(nested) = def["properties"].as_object() {
            flatten_props(nested, &full, out);
        }
        // Also recurse into fields (multi-fields)
        if let Some(fields) = def["fields"].as_object() {
            flatten_props(fields, &full, out);
        }
    }
}

fn print_fields_table(fields: &[(String, String)], _no_color: bool) {
    if fields.is_empty() {
        println!("No fields in mapping.");
        return;
    }
    println!();
    println!("  {:<48} {}", "field".dimmed(), "type".dimmed());
    println!("  {}", "─".repeat(64).dimmed());
    for (field, ftype) in fields {
        println!("  {:<48} {}", field, ftype);
    }
    println!();
    println!("  {} fields", fields.len());
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mapping(index: &str, props: serde_json::Value) -> Value {
        serde_json::json!({ index: { "mappings": { "properties": props } } })
    }

    #[test]
    fn test_flatten_mapping_simple() {
        let m = make_mapping("idx", serde_json::json!({
            "title":   { "type": "text" },
            "user_id": { "type": "keyword" },
            "count":   { "type": "integer" },
        }));
        let fields = flatten_mapping(&m, "idx");
        assert_eq!(fields.len(), 3);
        assert!(fields.iter().any(|(f, t)| f == "title"   && t == "text"));
        assert!(fields.iter().any(|(f, t)| f == "user_id" && t == "keyword"));
        assert!(fields.iter().any(|(f, t)| f == "count"   && t == "integer"));
    }

    #[test]
    fn test_flatten_mapping_nested() {
        let m = make_mapping("idx", serde_json::json!({
            "address": {
                "properties": {
                    "city":    { "type": "keyword" },
                    "country": { "type": "keyword" },
                }
            }
        }));
        let fields = flatten_mapping(&m, "idx");
        // "address" itself (type=object) + city + country
        assert!(fields.iter().any(|(f, _)| f == "address.city"));
        assert!(fields.iter().any(|(f, _)| f == "address.country"));
    }

    #[test]
    fn test_flatten_mapping_multi_fields() {
        let m = make_mapping("idx", serde_json::json!({
            "name": {
                "type": "text",
                "fields": {
                    "keyword": { "type": "keyword" }
                }
            }
        }));
        let fields = flatten_mapping(&m, "idx");
        assert!(fields.iter().any(|(f, t)| f == "name" && t == "text"));
        assert!(fields.iter().any(|(f, t)| f == "name.keyword" && t == "keyword"));
    }

    #[test]
    fn test_flatten_mapping_empty() {
        let m = serde_json::json!({ "idx": { "mappings": {} } });
        assert!(flatten_mapping(&m, "idx").is_empty());
    }

    #[test]
    fn test_flatten_mapping_sorted() {
        let m = make_mapping("idx", serde_json::json!({
            "zzz": { "type": "text" },
            "aaa": { "type": "keyword" },
            "mmm": { "type": "date" },
        }));
        let fields = flatten_mapping(&m, "idx");
        let names: Vec<&str> = fields.iter().map(|(f, _)| f.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
