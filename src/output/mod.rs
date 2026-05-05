pub mod json;
pub mod table;

use clap::ValueEnum;
use serde_json::Value;

/// Selectable output format (maps to --output flag)
#[derive(Debug, Clone, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Json,
    Table,
}

impl OutputFormat {
    pub fn print(&self, value: &Value, no_color: bool) {
        match self {
            OutputFormat::Json  => print_json(value, no_color),
            OutputFormat::Table => table::print_table(value),
        }
    }
}

/// Pretty-print JSON with optional coloring.
pub fn print_json(value: &Value, no_color: bool) {
    if no_color {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    } else {
        json::print_colored(value);
    }
}
