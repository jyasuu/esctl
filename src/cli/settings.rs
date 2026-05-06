use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;
use crate::stdin;

#[derive(Subcommand, Debug)]
pub enum SettingsCommands {
    /// Get settings for an index
    Get {
        index: String,
        #[arg(long)]
        include_defaults: bool,
        /// Filter to a specific setting key (e.g. "number_of_replicas")
        #[arg(long)]
        key: Option<String>,
    },
    /// Update settings for an index (file or - for stdin)
    Put {
        index: String,
        #[arg(long)]
        body: String,
    },
    /// Update a single setting inline: key=value
    Set {
        index: String,
        /// e.g. number_of_replicas=2
        setting: String,
    },
}

pub async fn handle(cmd: SettingsCommands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        SettingsCommands::Get { index, include_defaults, key } =>
            handle_get(client, cfg, &index, include_defaults, key.as_deref()).await,
        SettingsCommands::Put { index, body } =>
            handle_put(client, cfg, &index, &body).await,
        SettingsCommands::Set { index, setting } =>
            handle_set(client, cfg, &index, &setting).await,
    }
}

async fn handle_get(
    client: &EsClient, cfg: &AppConfig,
    index: &str, include_defaults: bool, key: Option<&str>,
) -> Result<()> {
    let mut path = format!("/{}/_settings", index);
    if include_defaults { path.push_str("?include_defaults=true"); }
    if let Some(k) = key { path = format!("/{}/_settings/index.{}", index, k); }

    let resp = client.get(&path).await?;

    match cfg.output {
        OutputFormat::Json  => cfg.output.print(&resp, cfg.no_color),
        OutputFormat::Table => print_settings_table(&resp, index, cfg.no_color),
    }
    Ok(())
}

async fn handle_put(client: &EsClient, cfg: &AppConfig, index: &str, body: &str) -> Result<()> {
    let payload = stdin::read_json(body).await?;
    eprintln!("{} Updating settings for '{}'…", "→".cyan(), index.bold());
    let resp = client.put(&format!("/{}/_settings", index), &payload).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Settings updated.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

async fn handle_set(client: &EsClient, cfg: &AppConfig, index: &str, setting: &str) -> Result<()> {
    let (key, value) = setting.split_once('=')
        .ok_or_else(|| anyhow::anyhow!("setting must be key=value, got: {}", setting))?;

    let val: Value = serde_json::from_str(value)
        .unwrap_or_else(|_| Value::String(value.to_string()));

    let body = serde_json::json!({ key: val });
    eprintln!("{} Setting {}.{} = {}…", "→".cyan(), index.bold(), key, value);
    let resp = client.put(&format!("/{}/_settings", index), &body).await?;
    if resp["acknowledged"].as_bool().unwrap_or(false) {
        eprintln!("{} Done.", "✓".green());
    }
    cfg.output.print(&resp, cfg.no_color);
    Ok(())
}

fn print_settings_table(resp: &Value, index: &str, _no_color: bool) {
    // ES returns { "index-name": { "settings": { "index": { ... } } } }
    let base = format!("/{}/settings/index", index);
    let settings = match resp.pointer(&base).and_then(Value::as_object) {
        Some(s) => s,
        None    => {
            println!("No settings found for '{}'.", index);
            return;
        }
    };

    println!();
    println!("  {} {}", "Index settings:".bold(), index);
    println!();
    println!("  {:<44} {}", "key".dimmed(), "value".dimmed());
    println!("  {}", "─".repeat(70).dimmed());

    let mut keys: Vec<&String> = settings.keys().collect();
    keys.sort();
    for k in keys {
        let v = &settings[k];
        println!("  {:<44} {}", format!("index.{}", k), format_setting_value(v));
    }
    println!();
}

fn format_setting_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b)   => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null      => "-".into(),
        other            => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_setting_value_string() {
        assert_eq!(format_setting_value(&Value::String("green".into())), "green");
    }

    #[test]
    fn test_format_setting_value_bool() {
        assert_eq!(format_setting_value(&Value::Bool(true)), "true");
        assert_eq!(format_setting_value(&Value::Bool(false)), "false");
    }

    #[test]
    fn test_format_setting_value_number() {
        assert_eq!(
            format_setting_value(&serde_json::json!(42)),
            "42"
        );
    }

    #[test]
    fn test_format_setting_value_null() {
        assert_eq!(format_setting_value(&Value::Null), "-");
    }

    #[test]
    fn test_set_parses_kv_string() {
        let setting = "number_of_replicas=2";
        let (key, value) = setting.split_once('=').unwrap();
        assert_eq!(key, "number_of_replicas");
        let val: Value = serde_json::from_str(value)
            .unwrap_or_else(|_| Value::String(value.into()));
        assert_eq!(val, serde_json::json!(2));
    }

    #[test]
    fn test_set_parses_kv_string_value() {
        let setting = "routing.allocation.enable=none";
        let (key, value) = setting.split_once('=').unwrap();
        assert_eq!(key, "routing.allocation.enable");
        let val: Value = serde_json::from_str(value)
            .unwrap_or_else(|_| Value::String(value.into()));
        assert_eq!(val, Value::String("none".into()));
    }

    #[test]
    fn test_set_missing_equals_errors() {
        let setting = "no-equals-here";
        assert!(setting.split_once('=').is_none());
    }
}
