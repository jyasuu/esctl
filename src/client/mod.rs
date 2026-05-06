pub mod auth;

use anyhow::{anyhow, Context, Result};
use reqwest::{header, Client};
use serde_json::Value;
use std::time::Duration;

use crate::config::AppConfig;
use auth::Auth;

/// Default request timeout (seconds)
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Thin wrapper around `reqwest::Client` scoped to an Elasticsearch host.
pub struct EsClient {
    http: Client,
    base_url: String,
    auth: Auth,
}

impl EsClient {
    pub fn new(cfg: &AppConfig) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        // Elasticsearch expects JSON — tell it explicitly
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );

        let timeout_secs = cfg.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);

        let http = Client::builder()
            .default_headers(headers)
            .danger_accept_invalid_certs(cfg.tls_skip_verify)
            .timeout(Duration::from_secs(timeout_secs))
            .connection_verbose(false)
            .build()
            .context("building HTTP client")?;

        let auth = Auth::from_config(cfg)?;
        let base_url = cfg.active_host().trim_end_matches('/').to_string();

        Ok(Self { http, base_url, auth })
    }

    // ── Public HTTP helpers ───────────────────────────────────────────────────

    pub async fn get(&self, path: &str) -> Result<Value> {
        let req = self.auth.apply(self.http.get(self.url(path)));
        self.send(req, "GET", path).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let req = self.auth.apply(self.http.post(self.url(path)).json(body));
        self.send(req, "POST", path).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        let req = self.auth.apply(self.http.put(self.url(path)).json(body));
        self.send(req, "PUT", path).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        let req = self.auth.apply(self.http.delete(self.url(path)));
        self.send(req, "DELETE", path).await
    }

    pub async fn delete_with_body(&self, path: &str, body: &Value) -> Result<Value> {
        let req = self.auth.apply(self.http.delete(self.url(path)).json(body));
        self.send(req, "DELETE", path).await
    }

    /// POST with a custom content-type (e.g. NDJSON for `_bulk`)
    pub async fn post_raw(
        &self,
        path: &str,
        body: String,
        content_type: &str,
    ) -> Result<Value> {
        let req = self
            .auth
            .apply(
                self.http
                    .post(self.url(path))
                    .header(header::CONTENT_TYPE, content_type)
                    .body(body),
            );
        self.send(req, "POST", path).await
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn send(
        &self,
        req: reqwest::RequestBuilder,
        method: &str,
        path: &str,
    ) -> Result<Value> {
        let response = req.send().await.map_err(|e| {
            if e.is_connect() {
                anyhow!(
                    "Cannot connect to Elasticsearch at {}.\n  \
                     Is it running? Check your --host / profile settings.\n  \
                     Details: {}",
                    self.base_url, e
                )
            } else if e.is_timeout() {
                anyhow!(
                    "{} {} timed out ({}s). Use --timeout to increase.",
                    method, path, DEFAULT_TIMEOUT_SECS
                )
            } else {
                anyhow!("{} {}: {}", method, path, e)
            }
        })?;

        let status = response.status();
        let body: Value = response
            .json()
            .await
            .unwrap_or(Value::Null);

        if status.is_success() {
            Ok(body)
        } else {
            // Build a rich error message from the ES error object
            let reason = extract_error_reason(&body);
            Err(anyhow!(
                "Elasticsearch {} {} — {} {}: {}",
                method,
                path,
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                reason,
            ))
        }
    }
}

/// Extract the most useful error text from an ES error response body.
fn extract_error_reason(body: &Value) -> String {
    // Try: error.root_cause[0].reason
    if let Some(rc) = body
        .pointer("/error/root_cause/0/reason")
        .and_then(Value::as_str)
    {
        return rc.to_string();
    }
    // Try: error.reason
    if let Some(r) = body.pointer("/error/reason").and_then(Value::as_str) {
        return r.to_string();
    }
    // Try: error (string form)
    if let Some(s) = body["error"].as_str() {
        return s.to_string();
    }
    // Fallback: dump the body
    serde_json::to_string(body).unwrap_or_else(|_| "unknown error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_error_reason_root_cause() {
        let body = serde_json::json!({
            "error": {
                "root_cause": [{ "reason": "index not found" }],
                "reason": "top level"
            }
        });
        assert_eq!(extract_error_reason(&body), "index not found");
    }

    #[test]
    fn test_extract_error_reason_fallback() {
        let body = serde_json::json!({
            "error": { "reason": "shard failed" }
        });
        assert_eq!(extract_error_reason(&body), "shard failed");
    }

    #[test]
    fn test_extract_error_reason_string() {
        let body = serde_json::json!({ "error": "Not found" });
        assert_eq!(extract_error_reason(&body), "Not found");
    }

    #[test]
    fn test_url_construction() {
        // Verify trailing-slash trimming works correctly
        let host = "http://localhost:9200/";
        let trimmed = host.trim_end_matches('/');
        assert_eq!(format!("{}{}", trimmed, "/_cluster/health"), "http://localhost:9200/_cluster/health");
    }
}
