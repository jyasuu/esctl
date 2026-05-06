use anyhow::{anyhow, Context, Result};
use is_terminal::IsTerminal;
use std::io::{self, BufRead, Read};

// ── Public API ────────────────────────────────────────────────────────────────

/// Read a single JSON value from: file path | "-" | piped stdin
pub async fn read_json(source: &str) -> Result<serde_json::Value> {
    let raw = read_string(source).await?.trim().to_string();
    serde_json::from_str(&raw).with_context(|| {
        format!(
            "invalid JSON from '{}' (first 120 chars): {}",
            source,
            &raw[..raw.len().min(120)]
        )
    })
}

/// Read raw NDJSON string from file or stdin (for `_bulk` body).
pub async fn read_ndjson(source: &str) -> Result<String> {
    read_string(source).await
}

/// Read NDJSON and parse each line into a serde_json::Value.
/// Skips blank lines. Returns (line_number, value) pairs.
pub async fn read_ndjson_lines(source: &str) -> Result<Vec<(usize, serde_json::Value)>> {
    let raw = read_string(source).await?;
    let mut out = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let val = serde_json::from_str(trimmed)
            .with_context(|| format!("invalid JSON on line {}: {}", i + 1, trimmed))?;
        out.push((i + 1, val));
    }
    Ok(out)
}

/// Read multiple JSON objects from stdin — either:
///   - A single JSON array  → iterate elements
///   - Multiple JSON lines  → iterate each line
pub async fn read_json_stream(source: &str) -> Result<Vec<serde_json::Value>> {
    let raw = read_string(source).await?;
    let trimmed = raw.trim();

    // Try as a single JSON value first
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return match val {
            serde_json::Value::Array(arr) => Ok(arr),
            other => Ok(vec![other]),
        };
    }

    // Fall back to NDJSON
    let mut out = Vec::new();
    for (i, line) in trimmed.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() { continue; }
        let v = serde_json::from_str(l)
            .with_context(|| format!("invalid JSON on line {}: {}", i + 1, l))?;
        out.push(v);
    }
    Ok(out)
}

/// Build an NDJSON bulk body from a vec of documents.
/// Each doc becomes: {"index":{}} \n <doc> \n
pub fn build_bulk_ndjson(index: &str, docs: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for doc in docs {
        body.push_str(&format!(
            "{{\"index\":{{\"_index\":\"{}\"}}}}\n",
            index
        ));
        body.push_str(&serde_json::to_string(doc).unwrap_or_default());
        body.push('\n');
    }
    body
}

/// Build an NDJSON bulk body with explicit IDs.
pub fn build_bulk_ndjson_with_ids(
    index: &str,
    docs: &[(Option<String>, serde_json::Value)],
) -> String {
    let mut body = String::new();
    for (id, doc) in docs {
        let meta = match id {
            Some(id_val) => format!(
                "{{\"index\":{{\"_index\":\"{}\",\"_id\":\"{}\"}}}}",
                index, id_val
            ),
            None => format!("{{\"index\":{{\"_index\":\"{}\"}}}}", index),
        };
        body.push_str(&meta);
        body.push('\n');
        body.push_str(&serde_json::to_string(doc).unwrap_or_default());
        body.push('\n');
    }
    body
}

/// Count non-empty lines in an NDJSON string (each pair = 1 op).
pub fn count_bulk_ops(ndjson: &str) -> usize {
    ndjson.lines().filter(|l| !l.trim().is_empty()).count() / 2
}

/// Parse the bulk API response and return (total, errors, failed_items).
pub fn parse_bulk_response(
    resp: &serde_json::Value,
) -> (usize, usize, Vec<serde_json::Value>) {
    let items = resp["items"].as_array().cloned().unwrap_or_default();
    let total = items.len();
    let mut errors = 0;
    let mut failed = Vec::new();

    for item in &items {
        // Each item is {"index": {...}} or {"create": {...}} etc.
        for action in &["index", "create", "update", "delete"] {
            if let Some(op) = item[action].as_object() {
                if op.get("error").is_some() {
                    errors += 1;
                    failed.push(item.clone());
                }
            }
        }
    }
    (total, errors, failed)
}

/// Returns true when stdin is a pipe (not a TTY).
pub fn is_piped_stdin() -> bool {
    !io::stdin().is_terminal()
}

// ── Internal ──────────────────────────────────────────────────────────────────

async fn read_string(source: &str) -> Result<String> {
    if source == "-" || (source.is_empty() && is_piped_stdin()) {
        read_stdin()
    } else if source.is_empty() {
        Err(anyhow!("no input source — provide a file path or pipe data to stdin"))
    } else {
        tokio::fs::read_to_string(source)
            .await
            .with_context(|| format!("reading file '{}'", source))
    }
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_bulk_ndjson_structure() {
        let docs = vec![
            serde_json::json!({"name": "alice"}),
            serde_json::json!({"name": "bob"}),
        ];
        let ndjson = build_bulk_ndjson("my-index", &docs);
        let lines: Vec<&str> = ndjson.lines().collect();
        // 2 docs × 2 lines each = 4 lines
        assert_eq!(lines.len(), 4);
        // Meta lines contain the index name
        assert!(lines[0].contains("\"_index\":\"my-index\""));
        assert!(lines[2].contains("\"_index\":\"my-index\""));
        // Doc lines contain the source
        assert!(lines[1].contains("alice"));
        assert!(lines[3].contains("bob"));
    }

    #[test]
    fn test_build_bulk_ndjson_with_ids() {
        let docs = vec![
            (Some("id-1".to_string()), serde_json::json!({"x": 1})),
            (None,                     serde_json::json!({"x": 2})),
        ];
        let ndjson = build_bulk_ndjson_with_ids("idx", &docs);
        let lines: Vec<&str> = ndjson.lines().collect();
        assert!(lines[0].contains("\"_id\":\"id-1\""));
        assert!(!lines[2].contains("\"_id\""));
    }

    #[test]
    fn test_count_bulk_ops() {
        let ndjson = "{\"index\":{}}\n{\"a\":1}\n{\"index\":{}}\n{\"b\":2}\n";
        assert_eq!(count_bulk_ops(ndjson), 2);
    }

    #[test]
    fn test_count_bulk_ops_with_blank_lines() {
        let ndjson = "{\"index\":{}}\n{\"a\":1}\n\n{\"index\":{}}\n{\"b\":2}\n";
        // blank line doesn't count toward ops
        assert_eq!(count_bulk_ops(ndjson), 2);
    }

    #[test]
    fn test_parse_bulk_response_no_errors() {
        let resp = serde_json::json!({
            "took": 5,
            "errors": false,
            "items": [
                { "index": { "_id": "1", "_index": "idx", "result": "created" } },
                { "index": { "_id": "2", "_index": "idx", "result": "created" } },
            ]
        });
        let (total, errors, failed) = parse_bulk_response(&resp);
        assert_eq!(total, 2);
        assert_eq!(errors, 0);
        assert!(failed.is_empty());
    }

    #[test]
    fn test_parse_bulk_response_with_errors() {
        let resp = serde_json::json!({
            "took": 3,
            "errors": true,
            "items": [
                { "index": { "_id": "1", "result": "created" } },
                { "index": { "_id": "2", "error": { "reason": "mapper error" } } },
            ]
        });
        let (total, errors, failed) = parse_bulk_response(&resp);
        assert_eq!(total, 2);
        assert_eq!(errors, 1);
        assert_eq!(failed.len(), 1);
    }

    #[test]
    fn test_parse_bulk_response_empty() {
        let resp = serde_json::json!({ "took": 0, "errors": false, "items": [] });
        let (total, errors, _) = parse_bulk_response(&resp);
        assert_eq!(total, 0);
        assert_eq!(errors, 0);
    }

    #[tokio::test]
    async fn test_read_json_stream_array() {
        // Write temp file
        let path = "/tmp/esctl_test_array.json";
        tokio::fs::write(path, b"[{\"a\":1},{\"b\":2}]").await.unwrap();
        let result = read_json_stream(path).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["a"], 1);
    }

    #[tokio::test]
    async fn test_read_json_stream_ndjson() {
        let path = "/tmp/esctl_test_ndjson.json";
        tokio::fs::write(path, b"{\"a\":1}\n{\"b\":2}\n").await.unwrap();
        let result = read_json_stream(path).await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_read_ndjson_lines_skips_blanks() {
        let path = "/tmp/esctl_test_ndjson_lines.json";
        tokio::fs::write(path, b"{\"a\":1}\n\n{\"b\":2}\n").await.unwrap();
        let result = read_ndjson_lines(path).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 1); // line 1
        assert_eq!(result[1].0, 3); // line 3 (blank was 2)
    }

    #[tokio::test]
    async fn test_read_json_from_file() {
        let path = "/tmp/esctl_test_read_json.json";
        tokio::fs::write(path, b"{\"hello\":\"world\"}").await.unwrap();
        let val = read_json(path).await.unwrap();
        assert_eq!(val["hello"], "world");
    }
}
