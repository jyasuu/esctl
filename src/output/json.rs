use colored::Colorize;
use serde_json::Value;

/// Recursively print JSON with ANSI colors.
pub fn print_colored(value: &Value) {
    print_value(value, 0);
    println!();
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn print_value(value: &Value, depth: usize) {
    match value {
        Value::Null => print!("{}", "null".cyan()),
        Value::Bool(b) => print!("{}", b.to_string().yellow()),
        Value::Number(n) => print!("{}", n.to_string().yellow()),
        Value::String(s) => print!("{}", format!("\"{}\"", s).green()),
        Value::Array(arr) => {
            if arr.is_empty() {
                print!("[]");
                return;
            }
            println!("[");
            for (i, item) in arr.iter().enumerate() {
                print!("{}", indent(depth + 1));
                print_value(item, depth + 1);
                if i < arr.len() - 1 {
                    print!(",");
                }
                println!();
            }
            print!("{}]", indent(depth));
        }
        Value::Object(map) => {
            if map.is_empty() {
                print!("{{}}");
                return;
            }
            println!("{{");
            let entries: Vec<_> = map.iter().collect();
            for (i, (k, v)) in entries.iter().enumerate() {
                print!(
                    "{}{}: ",
                    indent(depth + 1),
                    format!("\"{}\"", k).blue()
                );
                print_value(v, depth + 1);
                if i < entries.len() - 1 {
                    print!(",");
                }
                println!();
            }
            print!("{}}}", indent(depth));
        }
    }
}
