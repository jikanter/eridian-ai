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

/// Send the request with retry. Consumes the builder; clones internally per
/// attempt via `try_clone`. All our request bodies are fully buffered JSON so
/// cloning always succeeds — the Err branch is a guard for the impossible.
pub async fn send_with_retry(
    builder: RequestBuilder,
    cfg: &RetryConfig,
) -> Result<reqwest::Response> {
    let mut attempt: usize = 0;
    loop {
        let this = builder
            .try_clone()
            .ok_or_else(|| anyhow!("Cannot clone RequestBuilder for retry"))?;
        match this.send().await {
            Ok(res) => {
                let status = res.status().as_u16();
                if res.status().is_success() || !is_retryable_status(status) {
                    return Ok(res);
                }
                if attempt >= cfg.max_retries {
                    return Ok(res);
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
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// Convenience: send using the process-wide retry config.
pub async fn send(builder: RequestBuilder) -> Result<reqwest::Response> {
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
        let cfg: RetryConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.max_retries, 5);
        assert_eq!(cfg.initial_backoff_ms, 1000); // default preserved
        assert!((cfg.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }
}
