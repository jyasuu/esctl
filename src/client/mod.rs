pub mod auth;

use anyhow::{anyhow, Result};
use reqwest::{header, Client, StatusCode};
use serde_json::Value;

use crate::config::AppConfig;
use auth::Auth;

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

        let http = Client::builder()
            .default_headers(headers)
            .danger_accept_invalid_certs(cfg.tls_skip_verify)
            .build()?;

        let auth = Auth::from_config(cfg)?;
        let base_url = cfg.active_host().trim_end_matches('/').to_string();

        Ok(Self { http, base_url, auth })
    }

    // ── HTTP helpers ──────────────────────────────────────────────────────────

    pub async fn get(&self, path: &str) -> Result<Value> {
        let req = self.http.get(self.url(path));
        let req = self.auth.apply(req);
        self.send(req).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let req = self
            .http
            .post(self.url(path))
            .json(body);
        let req = self.auth.apply(req);
        self.send(req).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        let req = self
            .http
            .put(self.url(path))
            .json(body);
        let req = self.auth.apply(req);
        self.send(req).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        let req = self.http.delete(self.url(path));
        let req = self.auth.apply(req);
        self.send(req).await
    }

    pub async fn delete_with_body(&self, path: &str, body: &Value) -> Result<Value> {
        let req = self
            .http
            .delete(self.url(path))
            .json(body);
        let req = self.auth.apply(req);
        self.send(req).await
    }

    /// POST with a custom content-type (e.g. NDJSON for bulk)
    pub async fn post_raw(&self, path: &str, body: String, content_type: &str) -> Result<Value> {
        let req = self
            .http
            .post(self.url(path))
            .header(header::CONTENT_TYPE, content_type)
            .body(body);
        let req = self.auth.apply(req);
        self.send(req).await
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req.send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        if status.is_success() {
            Ok(body)
        } else {
            let reason = body
                .pointer("/error/reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            Err(anyhow!(
                "Elasticsearch {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                reason,
            ))
        }
    }
}
