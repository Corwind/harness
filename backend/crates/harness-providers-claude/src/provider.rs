//! `ClaudeProvider` — Anthropic Messages API adapter.
//!
//! Implements `harness_core::LlmProvider` against the public Messages
//! API. Talks raw HTTPS via `reqwest` (rustls); SSE is decoded with
//! `eventsource-stream`. There is no third-party Anthropic SDK in this
//! crate by design (PLAN R2): the wire format is small, stable, and
//! avoiding the SDK keeps the adapter lean and dependency-light.

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use futures::{StreamExt, TryStreamExt};
use harness_core::chat::{ChatEvent, ChatRequest};
use harness_core::error::ProviderError;
use harness_core::provider::{LlmProvider, ModelInfo, ProviderCapabilities, ProviderConfig};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, StatusCode};

use crate::config::{self, ClaudeConfig};
use crate::translate::Translator;
use crate::wire::{self, ErrorEnvelope, ListModelsResponse};

/// Default `max_tokens` for chat requests when the caller did not
/// specify one. Anthropic requires the field; we pick a generous cap so
/// production users only set it explicitly when they want to cap cost.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic provider adapter.
pub struct ClaudeProvider {
    http: Client,
}

impl std::fmt::Debug for ClaudeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeProvider").finish_non_exhaustive()
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeProvider {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(15))
            .https_only(false) // base_url override may target http://127.0.0.1 in tests
            .build()
            .expect("reqwest client builds with default rustls config");
        Self { http }
    }

    /// Construct with a pre-built reqwest client (test injection point).
    #[doc(hidden)]
    pub fn with_client(http: Client) -> Self {
        Self { http }
    }

    fn build_headers(cfg: &ClaudeConfig, accept: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-api-key", header(&cfg.api_key));
        h.insert("anthropic-version", header(&cfg.api_version));
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(reqwest::header::ACCEPT, HeaderValue::from_static(accept));
        // We never send AUTHORIZATION but reserve the header so a
        // future bearer-auth variant can override cleanly.
        let _ = AUTHORIZATION;
        h
    }
}

fn header(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("invalid"))
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: true,
            system_prompt: true,
            max_context_tokens: Some(200_000),
        }
    }

    async fn list_models(&self, cfg: &ProviderConfig) -> Result<Vec<ModelInfo>, ProviderError> {
        let parsed = config::parse(cfg)?;
        let url = format!("{}/v1/models", parsed.base_url.trim_end_matches('/'));

        let resp = self
            .http
            .get(&url)
            .headers(Self::build_headers(&parsed, "application/json"))
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_http_error(status, resp).await);
        }

        let body: ListModelsResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(format!("list_models body: {e}")))?;

        Ok(body
            .data
            .into_iter()
            .map(|m| ModelInfo {
                display_name: m.display_name.clone().unwrap_or_else(|| m.id.clone()),
                id: m.id,
                context_window: Some(200_000),
                max_output_tokens: Some(8192),
            })
            .collect())
    }

    async fn chat(
        &self,
        cfg: &ProviderConfig,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, ChatEvent>, ProviderError> {
        let parsed = config::parse(cfg)?;
        let url = format!("{}/v1/messages", parsed.base_url.trim_end_matches('/'));
        let body = wire::build_request(&request, DEFAULT_MAX_TOKENS);
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| ProviderError::Other(format!("serialise request: {e}")))?;

        let resp = self
            .http
            .post(&url)
            .headers(Self::build_headers(&parsed, "text/event-stream"))
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(map_http_error(status, resp).await);
        }

        // SSE decoder over the chunked byte stream.
        use eventsource_stream::Eventsource;
        let byte_stream = resp.bytes_stream().map_err(std::io::Error::other);
        let sse = byte_stream.eventsource();

        // Translate event-by-event, flattening into the canonical
        // ChatEvent stream. We keep the translator's terminated flag
        // outside the closure via `take_while`.
        let mut translator = Translator::new();
        let mapped = sse.flat_map(move |item| {
            let batch = match item {
                Ok(ev) => translator.handle(&ev.event, &ev.data),
                Err(e) => {
                    vec![translator.fatal(format!("sse error: {e}"))]
                }
            };
            stream::iter(batch)
        });

        // Take events until (and including) the first terminal one,
        // then close. `MessageStop` and `Error` are the only terminals
        // the translator emits.
        let stream = mapped
            .scan(false, |done, ev| {
                let was_done = *done;
                let terminal =
                    matches!(ev, ChatEvent::MessageStop { .. } | ChatEvent::Error { .. });
                *done = was_done || terminal;
                async move {
                    if was_done {
                        None
                    } else {
                        Some(ev)
                    }
                }
            })
            .boxed();

        Ok(stream)
    }
}

/// Map a non-2xx response to a typed `ProviderError`. Consumes the
/// response body so the caller never sees it.
async fn map_http_error(status: StatusCode, resp: reqwest::Response) -> ProviderError {
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let body = resp.text().await.unwrap_or_default();
    let message = match serde_json::from_str::<ErrorEnvelope>(&body) {
        Ok(env) => env.error.message.unwrap_or(body.clone()),
        Err(_) => body.clone(),
    };

    match status.as_u16() {
        401 | 403 => ProviderError::Unauthorized(message),
        429 => ProviderError::RateLimited {
            retry_after_secs: retry_after,
            message,
        },
        code => ProviderError::Request {
            status: Some(code),
            message,
        },
    }
}
