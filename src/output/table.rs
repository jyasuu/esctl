use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table};
use serde_json::Value;

/// Render a JSON value as a table.
/// - Array of objects → rows with auto-detected column headers
/// - Single object    → two-column key/value table
/// - Other            → falls back to pretty JSON
pub fn print_table(value: &Value) {
    match value {
        Value::Array(arr) if !arr.is_empty() => {
            if let Some(Value::Object(_)) = arr.first() {
                print_array_of_objects(arr);
            } else {
                print_flat_array(arr);
            }
        }
        Value::Object(map) => {
            print_object_as_kv(map);
        }
        other => {
            // Fallback: pretty JSON
            println!("{}", serde_json::to_string_pretty(other).unwrap_or_default());
        }
    }
}

fn print_array_of_objects(arr: &[Value]) {
    // Collect all unique keys across all rows (preserve first-seen order)
    let mut keys: Vec<String> = Vec::new();
    for item in arr {
        if let Value::Object(map) = item {
            for k in map.keys() {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
        }
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(keys.iter().map(|k| k.as_str()).collect::<Vec<_>>());

    for item in arr {
        if let Value::Object(map) = item {
            let row: Vec<String> = keys
                .iter()
                .map(|k| format_cell(map.get(k).unwrap_or(&Value::Null)))
                .collect();
            table.add_row(row);
        }
    }
    println!("{table}");
}

fn print_flat_array(arr: &[Value]) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(["#", "value"]);

    for (i, item) in arr.iter().enumerate() {
        table.add_row([i.to_string(), format_cell(item)]);
    }
    println!("{table}");
}

fn print_object_as_kv(map: &serde_json::Map<String, Value>) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(["key", "value"]);

    for (k, v) in map {
        table.add_row([k.clone(), format_cell(v)]);
    }
    println!("{table}");
}

fn format_cell(v: &Value) -> String {
    match v {
        Value::Null   => String::from(""),
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}
