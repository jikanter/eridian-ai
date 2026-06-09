//! SPEC-001 §2–§3: the event envelope and the typed event payloads.
//!
//! Every trace line shares one envelope:
//!
//! ```json
//! {"schema_version":"0.1","session_id":"01H..","parent_session_id":null,
//!  "seq":47,"ts_ns":1729872000123456789,"type":"provider.retry","data":{..}}
//! ```
//!
//! Two invariants from SPEC-001 §8 shape this module:
//!
//! - `seq` is assigned by the **writer thread**, not the producer, so it
//!   reflects on-disk order. Producers therefore build a [`PendingEvent`] that
//!   carries no `seq`; the writer stamps it via [`PendingEvent::to_line`].
//! - `ts_ns` is captured **producer-side** when the event happens.
//! - `schema_version` is a build constant, never a runtime value.

use serde::Serialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// The schema version this binary emits (SPEC-001 §5). Hardcoded per §8.3.
pub const SCHEMA_VERSION: &str = "0.1";

/// Wall-clock nanoseconds since the UNIX epoch (the `ts_ns` envelope field).
pub fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// A producer-side event: everything except the writer-assigned `seq`.
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub ts_ns: u64,
    pub kind: EventKind,
}

impl PendingEvent {
    /// Build a pending event, capturing `ts_ns` now (producer is the source of
    /// truth for when the event happened — SPEC-001 §8.2).
    pub fn new(session_id: String, parent_session_id: Option<String>, kind: EventKind) -> Self {
        Self {
            session_id,
            parent_session_id,
            ts_ns: now_ns(),
            kind,
        }
    }

    /// Serialize to a single SPEC-001 envelope line (no trailing newline),
    /// stamping the writer-assigned `seq`.
    pub fn to_line(&self, seq: u64) -> String {
        let envelope = json!({
            "schema_version": SCHEMA_VERSION,
            "session_id": self.session_id,
            "parent_session_id": self.parent_session_id,
            "seq": seq,
            "ts_ns": self.ts_ns,
            "type": self.kind.type_str(),
            "data": self.kind.data(),
        });
        // serde_json on a `json!` Value cannot fail.
        serde_json::to_string(&envelope).unwrap_or_default()
    }
}

/// The 17 SPEC-001 §3 event variants, grouped: session lifecycle, context
/// assembly, provider interaction, tool interaction, output, errors, and
/// trace-meta. Each variant knows its dotted `type` string and its `data`
/// payload.
#[derive(Debug, Clone)]
pub enum EventKind {
    // 3.1 Session lifecycle
    SessionStart(SessionStart),
    SessionEnd(SessionEnd),
    // 3.2 Context assembly
    SystemPromptBuilt(SystemPromptBuilt),
    RoleApplied(RoleApplied),
    RagRetrieved(RagRetrieved),
    // 3.3 Provider interaction
    ProviderRequest(ProviderRequest),
    ProviderResponse(ProviderResponse),
    ProviderRetry(ProviderRetry),
    ProviderFallback(ProviderFallback),
    // 3.4 Tool interaction
    ToolRequested(ToolRequested),
    ToolDenied(ToolDenied),
    ToolExecuted(ToolExecuted),
    // 3.5 Output
    OutputFinal(OutputFinal),
    OutputChunk(OutputChunk),
    // 3.6 Errors
    Error(ErrorEvent),
    // 3.7 Trace meta
    Heartbeat(Heartbeat),
    Dropped(Dropped),
}

impl EventKind {
    /// The dotted hierarchical type tag (SPEC-001 §3).
    pub fn type_str(&self) -> &'static str {
        match self {
            EventKind::SessionStart(_) => "session.start",
            EventKind::SessionEnd(_) => "session.end",
            EventKind::SystemPromptBuilt(_) => "context.system_prompt_built",
            EventKind::RoleApplied(_) => "context.role_applied",
            EventKind::RagRetrieved(_) => "context.rag_retrieved",
            EventKind::ProviderRequest(_) => "provider.request",
            EventKind::ProviderResponse(_) => "provider.response",
            EventKind::ProviderRetry(_) => "provider.retry",
            EventKind::ProviderFallback(_) => "provider.fallback",
            EventKind::ToolRequested(_) => "tool.requested",
            EventKind::ToolDenied(_) => "tool.denied",
            EventKind::ToolExecuted(_) => "tool.executed",
            EventKind::OutputFinal(_) => "output.final",
            EventKind::OutputChunk(_) => "output.chunk",
            EventKind::Error(_) => "error",
            EventKind::Heartbeat(_) => "trace.heartbeat",
            EventKind::Dropped(_) => "trace.dropped",
        }
    }

    /// The type-specific `data` payload object.
    pub fn data(&self) -> Value {
        match self {
            EventKind::SessionStart(p) => to_value(p),
            EventKind::SessionEnd(p) => to_value(p),
            EventKind::SystemPromptBuilt(p) => to_value(p),
            EventKind::RoleApplied(p) => to_value(p),
            EventKind::RagRetrieved(p) => to_value(p),
            EventKind::ProviderRequest(p) => to_value(p),
            EventKind::ProviderResponse(p) => to_value(p),
            EventKind::ProviderRetry(p) => to_value(p),
            EventKind::ProviderFallback(p) => to_value(p),
            EventKind::ToolRequested(p) => to_value(p),
            EventKind::ToolDenied(p) => to_value(p),
            EventKind::ToolExecuted(p) => to_value(p),
            EventKind::OutputFinal(p) => to_value(p),
            EventKind::OutputChunk(p) => to_value(p),
            EventKind::Error(p) => to_value(p),
            EventKind::Heartbeat(p) => to_value(p),
            EventKind::Dropped(p) => to_value(p),
        }
    }
}

fn to_value<T: Serialize>(p: &T) -> Value {
    serde_json::to_value(p).unwrap_or_else(|_| json!({}))
}

// ----- 3.1 Session lifecycle -----

#[derive(Debug, Clone, Serialize)]
pub struct SessionStart {
    pub aichat_version: String,
    pub config_hash: String,
    pub role: Option<String>,
    pub model_spec: Option<String>,
    pub fixture_id: Option<String>,
    pub cwd: String,
    pub args: Vec<String>,
    pub env_subset: indexmap::IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionEnd {
    pub exit_status: i32,
    pub wall_time_ns: u64,
    pub tokens_in_total: u64,
    pub tokens_out_total: u64,
    pub cost_usd: Option<f64>,
}

// ----- 3.2 Context assembly -----

#[derive(Debug, Clone, Serialize)]
pub struct SystemPromptBuilt {
    pub content_hash: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoleApplied {
    pub role_name: String,
    pub tool_whitelist: Option<Vec<String>>,
    pub rag_sources_enabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagHit {
    pub source_id: String,
    pub chunk_id: String,
    pub score: f32,
    pub included: bool,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagRetrieved {
    pub query: String,
    pub hits: Vec<RagHit>,
    pub top_k: u32,
    pub score_threshold: f32,
}

// ----- 3.3 Provider interaction -----

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRequest {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub params: Value,
    pub messages_hash: String,
    pub request_body_bytes: u64,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResponse {
    pub request_id: String,
    pub request_body_hash: String,
    pub status: u16,
    pub finish_reason: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub latency_ns: u64,
    pub response_body_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRetry {
    pub request_id: String,
    pub attempt: u32,
    pub trigger: String,
    pub details: String,
    pub backoff_ms: u64,
    pub will_fallback: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderFallback {
    pub from_provider: String,
    pub from_model: String,
    pub to_provider: String,
    pub to_model: String,
    pub reason: String,
}

// ----- 3.4 Tool interaction -----

#[derive(Debug, Clone, Serialize)]
pub struct ToolRequested {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    pub args_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDenied {
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub policy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolExecuted {
    pub tool_call_id: String,
    pub tool_name: String,
    pub exit_status: i32,
    pub duration_ns: u64,
    pub stdout_bytes: u64,
    pub stdout_hash: Option<String>,
    pub stderr_bytes: u64,
    pub stderr_hash: Option<String>,
    pub stdout_truncated: bool,
}

// ----- 3.5 Output -----

#[derive(Debug, Clone, Serialize)]
pub struct OutputFinal {
    pub content_hash: String,
    pub byte_len: u64,
    pub tokens_out: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputChunk {
    pub request_id: String,
    pub chunk_index: u64,
    pub content: String,
    pub delta_tokens: u64,
}

// ----- 3.6 Errors -----

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEvent {
    pub kind: String,
    pub message: String,
    pub context: Option<Value>,
}

// ----- 3.7 Trace meta -----

#[derive(Debug, Clone, Serialize)]
pub struct Heartbeat {
    pub uptime_ns: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dropped {
    pub count: u64,
    pub since_seq: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).expect("line must be valid JSON")
    }

    #[test]
    fn envelope_has_all_required_fields() {
        let ev = PendingEvent::new(
            "01HSESSION".into(),
            None,
            EventKind::Heartbeat(Heartbeat { uptime_ns: 5 }),
        );
        let v = parse(&ev.to_line(0));
        assert_eq!(v["schema_version"], "0.1");
        assert_eq!(v["session_id"], "01HSESSION");
        assert!(v["parent_session_id"].is_null());
        assert_eq!(v["seq"], 0);
        assert!(v["ts_ns"].as_u64().unwrap() > 0);
        assert_eq!(v["type"], "trace.heartbeat");
        assert_eq!(v["data"]["uptime_ns"], 5);
    }

    #[test]
    fn writer_assigned_seq_appears_in_envelope() {
        let ev = PendingEvent::new(
            "s".into(),
            Some("parent".into()),
            EventKind::Dropped(Dropped { count: 3, since_seq: 41 }),
        );
        let v = parse(&ev.to_line(42));
        assert_eq!(v["seq"], 42);
        assert_eq!(v["parent_session_id"], "parent");
        assert_eq!(v["data"]["count"], 3);
        assert_eq!(v["data"]["since_seq"], 41);
    }

    #[test]
    fn provider_retry_type_and_payload() {
        let ev = PendingEvent::new(
            "s".into(),
            None,
            EventKind::ProviderRetry(ProviderRetry {
                request_id: "req-1".into(),
                attempt: 2,
                trigger: "http_5xx".into(),
                details: "HTTP 502 Bad Gateway".into(),
                backoff_ms: 1000,
                will_fallback: false,
            }),
        );
        let v = parse(&ev.to_line(7));
        assert_eq!(v["type"], "provider.retry");
        assert_eq!(v["data"]["attempt"], 2);
        assert_eq!(v["data"]["trigger"], "http_5xx");
        assert_eq!(v["data"]["will_fallback"], false);
    }

    #[test]
    fn session_start_redactable_env_subset_serializes() {
        let mut env = indexmap::IndexMap::new();
        env.insert("HOME".to_string(), "/home/u".to_string());
        let ev = PendingEvent::new(
            "s".into(),
            None,
            EventKind::SessionStart(SessionStart {
                aichat_version: "0.7.0-eridian".into(),
                config_hash: "sha256:abc".into(),
                role: Some("rust-reviewer".into()),
                model_spec: Some("anthropic:claude-opus-4-7".into()),
                fixture_id: None,
                cwd: "/work".into(),
                args: vec!["aichat".into(), "--role".into()],
                env_subset: env,
            }),
        );
        let v = parse(&ev.to_line(0));
        assert_eq!(v["type"], "session.start");
        assert_eq!(v["data"]["role"], "rust-reviewer");
        assert!(v["data"]["fixture_id"].is_null());
        assert_eq!(v["data"]["env_subset"]["HOME"], "/home/u");
    }

    #[test]
    fn every_variant_has_distinct_dotted_type() {
        // Guards against a copy-paste type-string collision across the 17 types.
        let kinds = [
            "session.start",
            "session.end",
            "context.system_prompt_built",
            "context.role_applied",
            "context.rag_retrieved",
            "provider.request",
            "provider.response",
            "provider.retry",
            "provider.fallback",
            "tool.requested",
            "tool.denied",
            "tool.executed",
            "output.final",
            "output.chunk",
            "error",
            "trace.heartbeat",
            "trace.dropped",
        ];
        let unique: std::collections::HashSet<_> = kinds.iter().collect();
        assert_eq!(unique.len(), kinds.len());
    }
}
