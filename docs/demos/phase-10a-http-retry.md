# Phase 10A: HTTP Retry with Backoff

*2026-04-17T00:50:55Z by Showboat 0.6.1*
<!-- showboat-id: f0f3a573-4831-4ee4-8093-8527416f1aff -->

Phase 10A adds HTTP-level retry with exponential backoff to every provider call. Transient failures (HTTP 429, 500, 502, 503) now retry with backoff (default: 3 retries at 1s/2s/4s/cap 30s). Non-retryable statuses (401/403/404) fail fast. When a 429 response carries a `Retry-After` header, the server's delta-seconds hint overrides our computed backoff. Streaming (SSE) is not retried — once events reach the terminal, retry would duplicate output. All non-streaming providers share one retry helper via `src/client/retry.rs`.

## Retry module public API

```bash
grep -E "pub (fn|async fn|struct) " src/client/retry.rs | head -20
```

```output
pub struct RetryConfig {
pub fn set_global(cfg: RetryConfig) {
pub fn global() -> RetryConfig {
pub fn is_retryable_status(status: u16) -> bool {
pub fn parse_retry_after_seconds(header: &str) -> Option<Duration> {
pub fn backoff_delay(cfg: &RetryConfig, attempt: usize) -> Duration {
pub async fn send_with_retry(
pub async fn send(builder: RequestBuilder) -> Result<reqwest::Response> {
```

## Unit tests: classification, backoff math, Retry-After parsing

```bash
cargo test --bin aichat -- client::retry::tests 2>&1 | grep -E "^test client::retry|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/"
```

```output
test client::retry::tests::backoff_doubles_and_caps ... ok
test client::retry::tests::backoff_respects_custom_multiplier ... ok
test client::retry::tests::defaults_match_spec ... ok
test client::retry::tests::global_roundtrip ... ok
test client::retry::tests::non_retryable_statuses_fail_fast ... ok
test client::retry::tests::retry_after_parses_delta_seconds ... ok
test client::retry::tests::retry_after_rejects_http_date_and_junk ... ok
test client::retry::tests::retry_config_deserializes_partial ... ok
test client::retry::tests::retryable_statuses_are_transient_5xx_and_429 ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## Every non-streaming provider call site goes through retry

```bash
grep -rE "super::retry::send\(" src/client/*.rs
```

```output
src/client/bedrock.rs:    let res = super::retry::send(builder).await?;
src/client/bedrock.rs:    let res = super::retry::send(builder).await?;
src/client/bedrock.rs:    let res = super::retry::send(builder).await?;
src/client/claude.rs:    let res = super::retry::send(builder).await?;
src/client/cohere.rs:    let res = super::retry::send(builder).await?;
src/client/cohere.rs:    let res = super::retry::send(builder).await?;
src/client/gemini.rs:    let res = super::retry::send(builder).await?;
src/client/openai_compatible.rs:    let res = super::retry::send(builder).await?;
src/client/openai.rs:    let res = super::retry::send(builder).await?;
src/client/openai.rs:    let res = super::retry::send(builder).await?;
src/client/vertexai.rs:    let res = super::retry::send(builder).await?;
src/client/vertexai.rs:    let res = super::retry::send(builder).await?;
src/client/vertexai.rs:    let res = super::retry::send(builder).await?;
```

13 call sites across 7 providers. Azure OpenAI inherits via the shared OpenAI helpers.

## Config surface

```bash
grep "pub retry: RetryConfig\|retry: RetryConfig::default\|retry::set_global" src/config/mod.rs
```

```output
    pub retry: RetryConfig,
            retry: RetryConfig::default(),
        crate::client::retry::set_global(config.retry.clone());
```

Users configure the retry policy via YAML; every field has a default, so the section is fully optional. Example `config.yaml` snippet (4-space indented to keep showboat from trying to execute a `yaml` block):

    retry:
      max_retries: 3          # default 3
      initial_backoff_ms: 1000
      max_backoff_ms: 30000
      backoff_multiplier: 2.0

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```
