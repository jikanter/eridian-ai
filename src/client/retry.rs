//! Phase 10A: HTTP retry with exponential backoff.
//!
//! Transient provider failures (HTTP 429, 500, 502, 503) are automatically
//! retried with exponential backoff. Non-transient errors (401, 403, 404) and
//! any other non-retryable statuses surface immediately. For 429 responses
//! that carry a `Retry-After` header, we honor the server's delay hint over
//! our computed backoff.
//!
//! The retry is HTTP-level only; it does not wrap streaming SSE (where
//! partial output has already reached the terminal) or the pipeline stage
//! level (that's Phase 10C).

use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use reqwest::RequestBuilder;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 1000,
            max_backoff_ms: 30_000,
            backoff_multiplier: 2.0,
        }
    }
}

/// Process-wide default retry config. Set once at startup by `Config::load`;
/// read by every HTTP call site. Tests can override via `set_global()`.
static GLOBAL: LazyLock<RwLock<RetryConfig>> =
    LazyLock::new(|| RwLock::new(RetryConfig::default()));

pub fn set_global(cfg: RetryConfig) {
    *GLOBAL.write() = cfg;
}

pub fn global() -> RetryConfig {
    GLOBAL.read().clone()
}

pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503)
}

/// Parse the `Retry-After` header's delta-seconds form. We deliberately skip
/// HTTP-date parsing — every production LLM provider (OpenAI, Anthropic,
/// Gemini) returns an integer number of seconds.
pub fn parse_retry_after_seconds(header: &str) -> Option<Duration> {
    header.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Exponential backoff with a hard cap at `max_backoff_ms`.
pub fn backoff_delay(cfg: &RetryConfig, attempt: usize) -> Duration {
    let base_ms =
        (cfg.initial_backoff_ms as f64) * cfg.backoff_multiplier.powi(attempt as i32);
    let delay_ms = (base_ms as u64).min(cfg.max_backoff_ms);
    Duration::from_millis(delay_ms)
}

fn is_retryable_reqwest_error(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect()
}

/// Phase 42E-2b: on a capture turn, buffer the raw response body into the wire
/// slot (so `provider.response` carries the real wire bytes + status) and hand
/// the caller a faithful rebuilt `Response` it can still consume. reqwest does
/// not allow re-reading a streamed body, so we read it once and reconstruct.
/// Status and headers are preserved.
async fn capture_and_rebuild(res: reqwest::Response) -> Result<reqwest::Response> {
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.bytes().await?;
    crate::utils::trace_spec::wiring::capture_wire_response(status.as_u16(), body.to_vec());
    let mut builder = http::Response::builder().status(status);
    if let Some(dst) = builder.headers_mut() {
        *dst = headers;
    }
    let rebuilt = builder.body(body).map_err(anyhow::Error::new)?;
    Ok(reqwest::Response::from(rebuilt))
}

/// Return the response to the caller, buffering+rebuilding it first when a
/// trace turn is active. Off the trace path (the default) this is a pass-through
/// that never touches the body — zero cost on the request hot path.
async fn finalize_response(res: reqwest::Response, capture: bool) -> Result<reqwest::Response> {
    if capture {
        capture_and_rebuild(res).await
    } else {
        Ok(res)
    }
}

/// Send the request with retry. Consumes the builder; clones internally per
/// attempt via `try_clone`. All our request bodies are fully buffered JSON so
/// cloning always succeeds — the Err branch is a guard for the impossible.
pub async fn send_with_retry(
    builder: RequestBuilder,
    cfg: &RetryConfig,
) -> Result<reqwest::Response> {
    // Phase 42E-2a: when a trace turn is active, record the final status and one
    // entry per retry attempt so the trace makes the retry layer observable
    // (EVAL-001 §2). Guarded so tracing-off pays nothing.
    let capture = crate::utils::trace_spec::wiring::current_session().is_some();
    let mut attempt: usize = 0;
    loop {
        let this = builder
            .try_clone()
            .ok_or_else(|| anyhow!("Cannot clone RequestBuilder for retry"))?;
        match this.send().await {
            Ok(res) => {
                let status = res.status().as_u16();
                if res.status().is_success() || !is_retryable_status(status) {
                    return finalize_response(res, capture).await;
                }
                if attempt >= cfg.max_retries {
                    return finalize_response(res, capture).await;
                }
                let retry_after = res
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_retry_after_seconds);
                let delay = retry_after
                    .unwrap_or_else(|| backoff_delay(cfg, attempt))
                    .min(Duration::from_millis(cfg.max_backoff_ms));
                debug!(
                    "HTTP retry {}/{} after {:?} (status {})",
                    attempt + 1,
                    cfg.max_retries,
                    delay,
                    status
                );
                if capture {
                    crate::utils::trace_spec::wiring::capture_wire_retry(
                        crate::utils::trace_spec::wiring::WireRetry {
                            attempt: attempt as u32,
                            status: Some(status),
                            error: None,
                            backoff_ms: delay.as_millis() as u64,
                        },
                    );
                }
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(e) => {
                if attempt >= cfg.max_retries || !is_retryable_reqwest_error(&e) {
                    return Err(anyhow::Error::new(e));
                }
                let delay = backoff_delay(cfg, attempt);
                debug!(
                    "HTTP retry {}/{} after {:?} (error: {e})",
                    attempt + 1,
                    cfg.max_retries,
                    delay
                );
                if capture {
                    crate::utils::trace_spec::wiring::capture_wire_retry(
                        crate::utils::trace_spec::wiring::WireRetry {
                            attempt: attempt as u32,
                            status: None,
                            error: Some(e.to_string()),
                            backoff_ms: delay.as_millis() as u64,
                        },
                    );
                }
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// Stamp the `X-Eridian-Session-Id` correlation header (Phase 42D) when a value
/// is supplied. Pure so it can be tested without the process-global; `send`
/// feeds it the active trace turn's id.
pub fn apply_session_header(builder: RequestBuilder, session_id: Option<String>) -> RequestBuilder {
    match session_id {
        Some(id) => builder.header(crate::utils::trace_spec::wiring::SESSION_HEADER, id),
        None => builder,
    }
}

/// Phase 42E-1: recover the wire-true endpoint + serialized body from a builder
/// before it is sent. Our request bodies are fully buffered JSON, so the clone
/// and `as_bytes` always succeed; returns `None` only for the impossible
/// non-buffered/un-cloneable case. Pure, so it is unit-testable without a server.
pub fn wire_from_builder(builder: &RequestBuilder) -> Option<(String, Vec<u8>)> {
    let req = builder.try_clone()?.build().ok()?;
    let endpoint = req.url().to_string();
    let body = req
        .body()
        .and_then(|b| b.as_bytes())
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    Some((endpoint, body))
}

/// Convenience: send using the process-wide retry config, stamping the trace
/// correlation header for the active turn (if any).
pub async fn send(builder: RequestBuilder) -> Result<reqwest::Response> {
    let session = crate::utils::trace_spec::wiring::current_session();
    let builder = apply_session_header(builder, session.clone());
    // Phase 42E-1: capture the real wire request for the active turn to emit as
    // a wire-true `provider.request`. Guarded on an active turn so tracing-off
    // (the default) pays nothing on the request hot path.
    if session.is_some() {
        if let Some((endpoint, body)) = wire_from_builder(&builder) {
            crate::utils::trace_spec::wiring::capture_wire_request(endpoint, body);
        }
    }
    send_with_retry(builder, &global()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = RetryConfig::default();
        assert_eq!(c.max_retries, 3);
        assert_eq!(c.initial_backoff_ms, 1000);
        assert_eq!(c.max_backoff_ms, 30_000);
        assert!((c.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn capture_and_rebuild_buffers_body_and_preserves_response() {
        // Phase 42E-2b: on a capture turn, `send` buffers the raw response body
        // into the wire slot, then hands the client a faithful rebuilt
        // `Response` it can still `.json()`. Status + bytes survive the rebuild.
        let raw = br#"{"stop_reason":"end_turn","content":[]}"#;
        let http_res = http::Response::builder()
            .status(503)
            .header("content-type", "application/json")
            .body(raw.to_vec())
            .unwrap();
        let res = reqwest::Response::from(http_res);

        let rebuilt = capture_and_rebuild(res).await.expect("rebuild succeeds");

        // The raw wire bytes + real status landed in the capture slot.
        let captured =
            crate::utils::trace_spec::wiring::take_wire_response().expect("a captured response");
        assert_eq!(captured.status, 503);
        assert_eq!(captured.body, raw);

        // The client still sees the same status and body on the rebuilt response.
        assert_eq!(rebuilt.status().as_u16(), 503);
        let body = rebuilt.bytes().await.unwrap();
        assert_eq!(&body[..], raw);
    }

    #[tokio::test]
    async fn capture_and_rebuild_round_trips_a_real_streamed_response() {
        // Phase 42E-2b: the unit test above builds a Response by hand; this one
        // proves the buffer+rebuild also survives a genuine streamed body off a
        // real socket — the path `retry::send` actually drives — and that the
        // wire `finish_reason` is recoverable from the captured bytes.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"hi"}}]}"#;
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await; // drain the request line/headers
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.flush().await.unwrap();
        });

        let res = reqwest::Client::new()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap();
        let rebuilt = capture_and_rebuild(res).await.expect("rebuild succeeds");

        let captured =
            crate::utils::trace_spec::wiring::take_wire_response().expect("a captured response");
        assert_eq!(captured.status, 200);
        assert_eq!(captured.body, body.as_bytes());
        assert_eq!(
            super::super::common::finish_reason_from_body(&captured.body).as_deref(),
            Some("stop"),
        );

        // The rebuilt response is still fully parseable by the provider client.
        let v: serde_json::Value = rebuilt.json().await.unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        server.await.unwrap();
    }

    #[test]
    fn wire_from_builder_extracts_endpoint_and_body() {
        // Phase 42E-1: the wire-true request body + endpoint are recoverable
        // from the builder *before* send, so `provider.request` can carry the
        // real serialized payload instead of a pre-send stub.
        let client = reqwest::Client::new();
        let builder = client
            .post("https://api.example.com/v1/messages")
            .body(r#"{"model":"m","messages":[]}"#);
        let (endpoint, body) = wire_from_builder(&builder).expect("buffered JSON body is extractable");
        assert_eq!(endpoint, "https://api.example.com/v1/messages");
        assert_eq!(body, br#"{"model":"m","messages":[]}"#);
    }

    #[test]
    fn session_header_applied_when_present() {
        let client = reqwest::Client::new();
        let req = apply_session_header(client.get("http://x"), Some("01HSESSION".into()))
            .build()
            .unwrap();
        assert_eq!(
            req.headers()
                .get("X-Eridian-Session-Id")
                .and_then(|v| v.to_str().ok()),
            Some("01HSESSION")
        );
    }

    #[test]
    fn session_header_absent_when_none() {
        let client = reqwest::Client::new();
        let req = apply_session_header(client.get("http://x"), None)
            .build()
            .unwrap();
        assert!(req.headers().get("X-Eridian-Session-Id").is_none());
    }

    #[test]
    fn retryable_statuses_are_transient_5xx_and_429() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
    }

    #[test]
    fn non_retryable_statuses_fail_fast() {
        for s in [200, 201, 204, 301, 400, 401, 403, 404, 422, 501, 504] {
            assert!(!is_retryable_status(s), "status {s} should not retry");
        }
    }

    #[test]
    fn retry_after_parses_delta_seconds() {
        assert_eq!(parse_retry_after_seconds("30"), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after_seconds("  5 "), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after_seconds("0"), Some(Duration::from_secs(0)));
    }

    #[test]
    fn retry_after_rejects_http_date_and_junk() {
        assert_eq!(parse_retry_after_seconds("Wed, 21 Oct 2026 07:28:00 GMT"), None);
        assert_eq!(parse_retry_after_seconds("soon"), None);
        assert_eq!(parse_retry_after_seconds(""), None);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let cfg = RetryConfig::default();
        assert_eq!(backoff_delay(&cfg, 0), Duration::from_millis(1000));
        assert_eq!(backoff_delay(&cfg, 1), Duration::from_millis(2000));
        assert_eq!(backoff_delay(&cfg, 2), Duration::from_millis(4000));
        assert_eq!(backoff_delay(&cfg, 3), Duration::from_millis(8000));
        assert_eq!(backoff_delay(&cfg, 4), Duration::from_millis(16_000));
        // cap kicks in
        assert_eq!(backoff_delay(&cfg, 5), Duration::from_millis(30_000));
        assert_eq!(backoff_delay(&cfg, 20), Duration::from_millis(30_000));
    }

    #[test]
    fn backoff_respects_custom_multiplier() {
        let cfg = RetryConfig {
            initial_backoff_ms: 500,
            max_backoff_ms: 10_000,
            backoff_multiplier: 3.0,
            max_retries: 3,
        };
        assert_eq!(backoff_delay(&cfg, 0), Duration::from_millis(500));
        assert_eq!(backoff_delay(&cfg, 1), Duration::from_millis(1500));
        assert_eq!(backoff_delay(&cfg, 2), Duration::from_millis(4500));
        // cap
        assert_eq!(backoff_delay(&cfg, 3), Duration::from_millis(10_000));
    }

    #[test]
    fn global_roundtrip() {
        let custom = RetryConfig {
            max_retries: 7,
            initial_backoff_ms: 42,
            max_backoff_ms: 99,
            backoff_multiplier: 1.5,
        };
        set_global(custom.clone());
        let got = global();
        assert_eq!(got.max_retries, 7);
        assert_eq!(got.initial_backoff_ms, 42);
        // restore default so other tests are unaffected
        set_global(RetryConfig::default());
    }

    #[test]
    fn retry_config_deserializes_partial() {
        // serde(default) should let callers override just one field
        let yaml = "max_retries: 5";
        let cfg: RetryConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(cfg.max_retries, 5);
        assert_eq!(cfg.initial_backoff_ms, 1000); // default preserved
        assert!((cfg.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }
}
