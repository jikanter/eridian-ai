//! Phase 20A/20B/20D: remote aichat federation.
//!
//! `RemoteRoleResolver` calls another aichat instance's HTTP API to discover
//! (`GET /v1/roles/{name}`) and invoke (`POST /v1/roles/{name}/invoke`)
//! roles that live there. Resolution is purely an HTTP round trip; the
//! caller is responsible for mapping `EntityRef::Remote { target, role }`
//! to a concrete `(endpoint, api_key)` pair via [`resolve_target`].
//!
//! This module sits below the rest of `Config` in the dependency graph —
//! it imports `RemoteConfig` and the public-view shape but knows nothing
//! about how roles are loaded locally. That separation makes it cheap to
//! exercise from the pipeline path without dragging in the world.

use super::{RemoteConfig, RolePublicView};
use crate::client::CallMetrics;
use crate::pipe::{InvokeResult, StageTrace};
use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Instant;

/// Resolved address for a remote role: where to call, how to authenticate.
#[derive(Debug, Clone)]
pub struct ResolvedRemote {
    pub base_url: String,
    pub role: String,
    pub api_key: Option<String>,
}

/// Phase 20A: turn a parsed `EntityRef::Remote { target, role }` into a
/// concrete `(base_url, role, api_key)` triple.
///
/// `target` resolution order:
/// 1. Named lookup in `remotes:` config. Found ⇒ use its endpoint + key.
/// 2. Otherwise treat `target` as a literal `host[:port]` authority and
///    synthesize `http://<target>` as the base URL (no TLS, no auth — the
///    user is expected to point at a local or trusted endpoint when using
///    the raw-authority form).
pub fn resolve_target(
    remotes: &IndexMap<String, RemoteConfig>,
    target: &str,
    role: &str,
) -> Result<ResolvedRemote> {
    if let Some(cfg) = remotes.get(target) {
        if cfg.endpoint.is_empty() {
            bail!("Remote '{target}' has empty endpoint in config");
        }
        return Ok(ResolvedRemote {
            base_url: cfg.base_url().to_string(),
            role: role.to_string(),
            api_key: cfg.resolved_api_key(),
        });
    }
    // Raw authority — `remote:host:8080/role`. Require it to look at least
    // host-ish (no spaces, contains a dot or colon) so a typo like
    // `remote:typo/role` surfaces as "unknown remote" rather than a
    // misleading "connection refused".
    if target.contains(char::is_whitespace) {
        bail!("Remote target '{target}' contains whitespace");
    }
    if !target.contains('.') && !target.contains(':') {
        bail!(
            "Remote target '{target}' is not in `remotes:` config and does not look like a \
             host:port authority. Add it under `remotes:` in config.yaml or use \
             `remote:<host:port>/<role>`."
        );
    }
    Ok(ResolvedRemote {
        base_url: format!("http://{target}"),
        role: role.to_string(),
        api_key: None,
    })
}

/// Phase 20B: GET /v1/roles/{name} on the remote, parse the `RolePublicView`.
///
/// This is a discovery probe — used at preflight time to confirm the remote
/// knows about the role before we commit to invoking it. Resolution
/// failures surface as `Err` with the HTTP body included for debugging.
pub async fn discover(client: &Client, target: &ResolvedRemote) -> Result<RolePublicView> {
    let url = format!("{}/v1/roles/{}", target.base_url, target.role);
    let mut req = client.get(&url);
    if let Some(key) = &target.api_key {
        req = req.bearer_auth(key);
    }
    let res = req
        .send()
        .await
        .with_context(|| format!("Remote discovery failed: GET {url}"))?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        bail!(
            "Remote discovery returned {} for {url}: {body}",
            status.as_u16()
        );
    }
    let view: RolePublicView = res
        .json()
        .await
        .with_context(|| format!("Remote {url} returned malformed JSON for /v1/roles/{}", target.role))?;
    Ok(view)
}

/// Phase 20A/20D: POST /v1/roles/{name}/invoke on the remote, parse the
/// invoke envelope into an `InvokeResult` so the federated stage looks
/// identical to a local one upstream.
///
/// The remote response carries its own `usage.cost_usd` etc.; we carry
/// those through into `CallMetrics` and annotate `model_id` with a
/// `remote:<host>` prefix so traces show *where* the stage ran, not just
/// what model it used.
pub async fn invoke(
    client: &Client,
    target: &ResolvedRemote,
    input: &str,
    variables: &IndexMap<String, String>,
    trace: bool,
) -> Result<InvokeResult> {
    let url = format!("{}/v1/roles/{}/invoke", target.base_url, target.role);
    let mut body = json!({
        "input": input,
        "trace": trace,
    });
    if !variables.is_empty() {
        body["variables"] = serde_json::to_value(variables)?;
    }
    let mut req = client.post(&url).json(&body);
    if let Some(key) = &target.api_key {
        req = req.bearer_auth(key);
    }
    let start = Instant::now();
    let res = req
        .send()
        .await
        .with_context(|| format!("Remote invoke failed: POST {url}"))?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        bail!(
            "Remote invoke returned {} for {url}: {body}",
            status.as_u16()
        );
    }
    let envelope: Value = res
        .json()
        .await
        .with_context(|| format!("Remote {url} returned malformed JSON for invoke"))?;
    let total_latency = start.elapsed().as_millis() as u64;

    let output = envelope
        .get("output")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Remote {url} response missing 'output' field"))?
        .to_string();

    let usage = envelope.get("usage").cloned().unwrap_or(json!({}));
    let model_label = usage
        .get("model")
        .and_then(Value::as_str)
        .map(|m| format!("remote:{}:{m}", short_host(&target.base_url)))
        .unwrap_or_else(|| format!("remote:{}", short_host(&target.base_url)));

    let metrics = CallMetrics {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cost_usd: usage
            .get("cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        latency_ms: usage
            .get("latency_ms")
            .and_then(Value::as_u64)
            .unwrap_or(total_latency),
        model_id: model_label.clone(),
        turns: 1,
        cached: false,
    };

    // Preserve the remote's stage trace verbatim if present; otherwise
    // synthesize a single-stage trace so callers always see at least one
    // entry attributing the run to the remote.
    let stages: Vec<StageTrace> = if let Some(trace_obj) = envelope.get("trace") {
        if let Some(arr) = trace_obj.get("stages").and_then(Value::as_array) {
            arr.iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    Some(StageTrace {
                        role: v.get("role")?.as_str()?.to_string(),
                        model: v
                            .get("model")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        input_tokens: v.get("input_tokens")?.as_u64()?,
                        output_tokens: v.get("output_tokens")?.as_u64()?,
                        cost_usd: v.get("cost_usd")?.as_f64()?,
                        latency_ms: v.get("latency_ms")?.as_u64()?,
                        branch: v
                            .get("branch")
                            .and_then(Value::as_u64)
                            .map(|n| n as usize),
                        // Phase 22A/22D: preserve the remote's grouping/cache
                        // flags when present; default to flat/uncached.
                        node_index: v
                            .get("node_index")
                            .and_then(Value::as_u64)
                            .map(|n| n as usize)
                            .unwrap_or(i),
                        cached: v.get("cached").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        } else {
            vec![single_trace(&target.role, &metrics)]
        }
    } else {
        vec![single_trace(&target.role, &metrics)]
    };

    let schema_valid = envelope
        .get("schema_valid")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    Ok(InvokeResult {
        output,
        metrics,
        stages,
        schema_valid,
    })
}

fn single_trace(role: &str, m: &CallMetrics) -> StageTrace {
    StageTrace {
        role: role.to_string(),
        model: m.model_id.clone(),
        input_tokens: m.input_tokens,
        output_tokens: m.output_tokens,
        cost_usd: m.cost_usd,
        latency_ms: m.latency_ms,
        branch: None,
        node_index: 0,
        cached: m.cached,
    }
}

fn short_host(base_url: &str) -> String {
    base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(base_url)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_remote(name: &str, endpoint: &str, key: Option<&str>) -> IndexMap<String, RemoteConfig> {
        let mut m = IndexMap::new();
        m.insert(
            name.to_string(),
            RemoteConfig {
                endpoint: endpoint.into(),
                api_key: key.map(String::from),
            },
        );
        m
    }

    #[test]
    fn resolves_named_target_with_api_key() {
        let remotes = one_remote(
            "staging",
            "http://staging.internal:8080",
            Some("token-123"),
        );
        let r = resolve_target(&remotes, "staging", "review").unwrap();
        assert_eq!(r.base_url, "http://staging.internal:8080");
        assert_eq!(r.role, "review");
        assert_eq!(r.api_key.as_deref(), Some("token-123"));
    }

    #[test]
    fn resolves_named_target_without_api_key() {
        let remotes = one_remote("security", "http://sec.internal:9000", None);
        let r = resolve_target(&remotes, "security", "scan").unwrap();
        assert!(r.api_key.is_none());
    }

    #[test]
    fn named_target_endpoint_trailing_slash_stripped() {
        let remotes = one_remote("staging", "http://staging.internal/", None);
        let r = resolve_target(&remotes, "staging", "review").unwrap();
        assert_eq!(r.base_url, "http://staging.internal");
    }

    #[test]
    fn unnamed_host_port_target_synthesizes_http_url() {
        let remotes: IndexMap<String, RemoteConfig> = IndexMap::new();
        let r = resolve_target(&remotes, "host.example:8080", "review").unwrap();
        assert_eq!(r.base_url, "http://host.example:8080");
        assert!(r.api_key.is_none());
    }

    #[test]
    fn unnamed_target_with_dot_only_accepted() {
        // `staging.internal` without a port — counts as a hostname.
        let remotes: IndexMap<String, RemoteConfig> = IndexMap::new();
        let r = resolve_target(&remotes, "staging.internal", "review").unwrap();
        assert_eq!(r.base_url, "http://staging.internal");
    }

    #[test]
    fn unknown_bare_name_target_errors_with_hint() {
        let remotes: IndexMap<String, RemoteConfig> = IndexMap::new();
        let err = resolve_target(&remotes, "staging", "review").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("staging"), "msg={msg}");
        assert!(msg.contains("remotes:"), "msg={msg}");
    }

    #[test]
    fn whitespace_in_target_errors() {
        let remotes: IndexMap<String, RemoteConfig> = IndexMap::new();
        assert!(resolve_target(&remotes, "bad host:8080", "x").is_err());
    }

    #[test]
    fn empty_endpoint_errors() {
        let remotes = one_remote("dud", "", None);
        let err = resolve_target(&remotes, "dud", "x").unwrap_err();
        assert!(err.to_string().contains("empty endpoint"));
    }

    #[test]
    fn short_host_trims_scheme_and_path() {
        assert_eq!(short_host("http://example.com:8080/foo"), "example.com:8080");
        assert_eq!(short_host("https://example.com"), "example.com");
        assert_eq!(short_host("example.com"), "example.com");
    }
}
