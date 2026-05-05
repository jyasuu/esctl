use anyhow::{Context, Result};
use is_terminal::IsTerminal;
use std::io::{self, Read};

/// Read a JSON value from:
///   - `"-"` → stdin
///   - a file path → read file
///   - piped stdin (auto-detected if no explicit source given)
pub async fn read_json(source: &str) -> Result<serde_json::Value> {
    let raw = read_string(source).await?;
    serde_json::from_str(&raw).context("invalid JSON")
}

/// Read raw NDJSON (newline-delimited JSON) from file or stdin.
/// Returns the raw string for bulk API calls.
pub async fn read_ndjson(source: &str) -> Result<String> {
    read_string(source).await
}

/// Read the full text from a source: "-" | file path | auto-detect stdin pipe
async fn read_string(source: &str) -> Result<String> {
    if source == "-" || is_piped_stdin() && source.is_empty() {
        read_stdin()
    } else {
        tokio::fs::read_to_string(source)
            .await
            .with_context(|| format!("reading file '{}'", source))
    }
}

/// Returns true when stdin is being piped (i.e. not a TTY).
pub fn is_piped_stdin() -> bool {
    !io::stdin().is_terminal()
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}
