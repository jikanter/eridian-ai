//! Phase 42C: the turn-level lifecycle orchestrator.
//!
//! [`TraceSession`] ties the 42A emitter ([`writer`](super::writer)), the 42B
//! blob store ([`blob`](super::blob)) and the SPEC §1 layout
//! ([`layout`](super::layout)) into one ergonomic surface. It:
//!
//! - mints the turn ULID, spawns the writer onto `traces/turn-<id>.jsonl`,
//!   records the parent↔turn binding in `manifest.jsonl`, and emits
//!   `session.start` — all in [`TraceSession::start`];
//! - offers one method per lifecycle event, each offloading large payloads to
//!   the blob store and referencing them by `sha256:<hex>`;
//! - emits `session.end` and drains the writer in [`TraceSession::end`].
//!
//! The producer call sites in `call_react` / `main` are wired to this in 42D
//! (the `--trace` surface unification); 42C delivers the coverage layer.

use serde_json::Value;

use super::blob::{self, BlobStore};
use super::event::{
    self, ErrorEvent, EventKind, OutputFinal, ProviderRequest, ProviderResponse, ProviderRetry,
    RoleApplied, SessionEnd, SessionStart, SystemPromptBuilt, ToolExecuted, ToolRequested,
};
use super::layout::{append_manifest, TraceLayout};
use super::redact;
use super::writer::{self, TraceHandle, TraceSender};

/// Static facts about a turn, captured at `session.start`.
#[derive(Debug, Clone, Default)]
pub struct StartInfo {
    pub aichat_version: String,
    pub config_hash: String,
    pub role: Option<String>,
    pub model_spec: Option<String>,
    pub fixture_id: Option<String>,
    pub cwd: String,
    pub args: Vec<String>,
}

/// One conversational turn's trace. Emits `session.start` on construction and
/// `session.end` on [`end`](Self::end).
pub struct TraceSession {
    sender: TraceSender,
    handle: Option<TraceHandle>,
    blobs: BlobStore,
    session_id: String,
    start_ns: u64,
}

impl TraceSession {
    /// Begin a turn: mint the ULID, spawn the writer, append the manifest
    /// binding, and emit `session.start`.
    pub fn start(
        layout: &TraceLayout,
        parent_session_id: Option<String>,
        info: StartInfo,
    ) -> std::io::Result<Self> {
        let session_id = super::ulid::new_ulid();
        let start_ns = event::now_ns();

        let (sender, handle) = writer::spawn_to_path(
            &layout.turn_path(&session_id),
            session_id.clone(),
            parent_session_id.clone(),
        )?;
        append_manifest(
            &layout.manifest_path(),
            parent_session_id.as_deref(),
            &session_id,
            start_ns,
        )?;

        let session = Self {
            sender,
            handle: Some(handle),
            blobs: BlobStore::new(layout.blobs_dir()),
            session_id,
            start_ns,
        };

        session.sender.emit(EventKind::SessionStart(SessionStart {
            aichat_version: info.aichat_version,
            config_hash: info.config_hash,
            role: info.role,
            model_spec: info.model_spec,
            fixture_id: info.fixture_id,
            cwd: info.cwd,
            args: info.args,
            env_subset: redact::env_subset_from_process(),
        }));

        Ok(session)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Store `bytes` in the blob store and return their `sha256:<hex>`
    /// reference. Best-effort: a write failure still yields the correct address
    /// so the event stays self-consistent (the referenced blob is simply
    /// absent, which a consumer can detect).
    fn put_blob(&self, bytes: &[u8]) -> String {
        let hex = match self.blobs.put(bytes) {
            Ok(h) => h,
            Err(_) => blob::sha256_hex(bytes),
        };
        blob::hash_ref(&hex)
    }

    /// `context.system_prompt_built` — full prompt goes to the blob store.
    pub fn system_prompt_built(&self, prompt: &str) {
        let content_hash = self.put_blob(prompt.as_bytes());
        self.sender.emit(EventKind::SystemPromptBuilt(SystemPromptBuilt {
            content_hash,
            byte_len: prompt.len() as u64,
        }));
    }

    /// `context.role_applied`.
    pub fn role_applied(
        &self,
        role_name: String,
        tool_whitelist: Option<Vec<String>>,
        rag_sources_enabled: Vec<String>,
    ) {
        self.sender.emit(EventKind::RoleApplied(RoleApplied {
            role_name,
            tool_whitelist,
            rag_sources_enabled,
        }));
    }

    /// `provider.request` — the messages body is auth-stripped, stored, and
    /// referenced by a key-independent `messages_hash`. Returns the minted
    /// `request_id` so the matching response can correlate.
    pub fn provider_request(
        &self,
        provider: &str,
        model: &str,
        params: Value,
        messages: &Value,
        endpoint: &str,
    ) -> String {
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut redacted = messages.clone();
        redact::strip_auth_headers(&mut redacted);
        let body = serde_json::to_vec(&redacted).unwrap_or_default();
        let messages_hash = self.put_blob(&body);

        self.sender.emit(EventKind::ProviderRequest(ProviderRequest {
            request_id: request_id.clone(),
            provider: provider.to_string(),
            model: model.to_string(),
            params,
            messages_hash,
            request_body_bytes: body.len() as u64,
            endpoint: endpoint.to_string(),
        }));
        request_id
    }

    /// `provider.response` — the response body is stored and referenced.
    #[allow(clippy::too_many_arguments)]
    pub fn provider_response(
        &self,
        request_id: &str,
        request_body_hash: &str,
        status: u16,
        finish_reason: &str,
        tokens_in: u64,
        tokens_out: u64,
        latency_ns: u64,
        response_body: &[u8],
    ) {
        let response_body_hash = self.put_blob(response_body);
        self.sender.emit(EventKind::ProviderResponse(ProviderResponse {
            request_id: request_id.to_string(),
            request_body_hash: request_body_hash.to_string(),
            status,
            finish_reason: finish_reason.to_string(),
            tokens_in,
            tokens_out,
            latency_ns,
            response_body_hash,
        }));
    }

    /// `provider.retry` — first-class, one event per attempt.
    pub fn provider_retry(
        &self,
        request_id: &str,
        attempt: u32,
        trigger: &str,
        details: &str,
        backoff_ms: u64,
        will_fallback: bool,
    ) {
        self.sender.emit(EventKind::ProviderRetry(ProviderRetry {
            request_id: request_id.to_string(),
            attempt,
            trigger: trigger.to_string(),
            details: details.to_string(),
            backoff_ms,
            will_fallback,
        }));
    }

    /// `tool.requested` — small args inline (<1KB), larger args to the blob
    /// store with the `args` field cleared.
    pub fn tool_requested(&self, tool_call_id: &str, tool_name: &str, args: Value) {
        let serialized = serde_json::to_vec(&args).unwrap_or_default();
        let args_hash = self.put_blob(&serialized);
        let inline = if serialized.len() < 1024 {
            args
        } else {
            Value::Null
        };
        self.sender.emit(EventKind::ToolRequested(ToolRequested {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: inline,
            args_hash,
        }));
    }

    /// `tool.executed` — stdout/stderr go to the blob store; empty streams
    /// reference `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn tool_executed(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        exit_status: i32,
        duration_ns: u64,
        stdout: &[u8],
        stderr: &[u8],
        stdout_truncated: bool,
    ) {
        let stdout_hash = (!stdout.is_empty()).then(|| self.put_blob(stdout));
        let stderr_hash = (!stderr.is_empty()).then(|| self.put_blob(stderr));
        self.sender.emit(EventKind::ToolExecuted(ToolExecuted {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            exit_status,
            duration_ns,
            stdout_bytes: stdout.len() as u64,
            stdout_hash,
            stderr_bytes: stderr.len() as u64,
            stderr_hash,
            stdout_truncated,
        }));
    }

    /// `output.final` — the assistant's final text goes to the blob store.
    pub fn output_final(&self, content: &str, tokens_out: u64) {
        let content_hash = self.put_blob(content.as_bytes());
        self.sender.emit(EventKind::OutputFinal(OutputFinal {
            content_hash,
            byte_len: content.len() as u64,
            tokens_out,
        }));
    }

    /// `error` — failures not covered by `provider.retry`.
    pub fn error(&self, kind: &str, message: &str, context: Option<Value>) {
        self.sender.emit(EventKind::Error(ErrorEvent {
            kind: kind.to_string(),
            message: message.to_string(),
            context,
        }));
    }

    /// Emit `session.end` and drain the writer thread.
    pub fn end(
        self,
        exit_status: i32,
        tokens_in_total: u64,
        tokens_out_total: u64,
        cost_usd: Option<f64>,
    ) {
        let wall_time_ns = event::now_ns().saturating_sub(self.start_ns);
        self.sender.emit(EventKind::SessionEnd(SessionEnd {
            exit_status,
            wall_time_ns,
            tokens_in_total,
            tokens_out_total,
            cost_usd,
        }));
        // Move the sender and handle out, drop the sender to disconnect the
        // channel, then join the writer so all events are flushed to disk.
        let TraceSession { sender, handle, .. } = self;
        drop(sender);
        if let Some(handle) = handle {
            handle.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_base(tag: &str) -> PathBuf {
        let id = format!("{:?}", std::thread::current().id());
        let dir = std::env::temp_dir()
            .join("aichat-session-test")
            .join(format!("{tag}-{}", id.replace(['(', ')', ' '], "")));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn read_turn_events(layout: &TraceLayout, session_id: &str) -> Vec<serde_json::Value> {
        let content = std::fs::read_to_string(layout.turn_path(session_id)).unwrap();
        content
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn full_turn_emits_ordered_lifecycle_with_blob_offload() {
        let base = temp_base("full-turn");
        let layout = TraceLayout::new(&base);

        let session = TraceSession::start(
            &layout,
            Some("PARENT".into()),
            StartInfo {
                aichat_version: "0.7.0-eridian".into(),
                config_hash: "sha256:cfg".into(),
                role: Some("rust-reviewer".into()),
                model_spec: Some("anthropic:claude-opus-4-7".into()),
                cwd: "/work".into(),
                args: vec!["aichat".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let sid = session.session_id().to_string();

        session.system_prompt_built("You are a careful reviewer.");
        let messages = serde_json::json!({
            "headers": {"Authorization": "Bearer sk-SECRET"},
            "messages": [{"role": "user", "content": "review this"}]
        });
        let req_id = session.provider_request(
            "anthropic",
            "claude-opus-4-7",
            serde_json::json!({"temperature": 0.0}),
            &messages,
            "https://api.anthropic.com/v1/messages",
        );
        session.provider_response(
            &req_id,
            "sha256:req",
            200,
            "tool_use",
            100,
            50,
            1_000,
            b"{\"content\":\"...\"}",
        );
        session.tool_requested("tc-1", "fs_read", serde_json::json!({"path": "src/main.rs"}));
        session.tool_executed("tc-1", "fs_read", 0, 5_000, b"file contents", b"", false);
        session.output_final("Looks good.", 50);
        session.end(0, 100, 50, Some(0.01));

        let events = read_turn_events(&layout, &sid);
        let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(
            types,
            vec![
                "session.start",
                "context.system_prompt_built",
                "provider.request",
                "provider.response",
                "tool.requested",
                "tool.executed",
                "output.final",
                "session.end",
            ]
        );
        // seq strictly monotonic from 0.
        for (i, e) in events.iter().enumerate() {
            assert_eq!(e["seq"], i as u64);
            assert_eq!(e["session_id"], sid);
        }
    }

    #[test]
    fn manifest_records_parent_binding() {
        let base = temp_base("manifest-bind");
        let layout = TraceLayout::new(&base);
        let session =
            TraceSession::start(&layout, Some("PARENT".into()), StartInfo::default()).unwrap();
        let sid = session.session_id().to_string();
        session.end(0, 0, 0, None);

        let manifest = std::fs::read_to_string(layout.manifest_path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(manifest.trim()).unwrap();
        assert_eq!(v["parent_session_id"], "PARENT");
        assert_eq!(v["turn_session_id"], sid);
    }

    #[test]
    fn referenced_blobs_exist_and_messages_hash_is_redacted() {
        let base = temp_base("blob-refs");
        let layout = TraceLayout::new(&base);
        let session = TraceSession::start(&layout, None, StartInfo::default()).unwrap();
        let sid = session.session_id().to_string();

        session.system_prompt_built("PROMPT-BYTES");
        let messages = serde_json::json!({"headers": {"Authorization": "Bearer sk-XYZ"}});
        session.provider_request("p", "m", Value::Null, &messages, "e");
        session.output_final("FINAL", 1);
        session.end(0, 0, 0, None);

        let events = read_turn_events(&layout, &sid);
        let blobs = BlobStore::new(layout.blobs_dir());

        // Every *_hash referenced by an event must resolve in the blob store.
        for e in &events {
            for key in ["content_hash", "messages_hash", "response_body_hash"] {
                if let Some(h) = e["data"][key].as_str() {
                    let hex = h.strip_prefix("sha256:").unwrap();
                    assert!(blobs.contains(hex), "missing blob for {key}={h}");
                }
            }
        }
        // The stored request body must NOT contain the plaintext key.
        let req = events
            .iter()
            .find(|e| e["type"] == "provider.request")
            .unwrap();
        let hex = req["data"]["messages_hash"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap();
        let stored = String::from_utf8(blobs.get(hex).unwrap()).unwrap();
        assert!(!stored.contains("sk-XYZ"), "leaked key into blob: {stored}");
        assert!(stored.contains("<redacted>"));
    }
}
