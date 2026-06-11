//! Phase 42D: the `--trace` / `AICHAT_TRACE` surface that wires the SPEC-001
//! emitter into the live request path, plus the `X-Eridian-Session-Id`
//! correlation header.
//!
//! Two pieces live here:
//!
//! - [`SpecTraceConfig`] + [`start_turn`]: the runtime settings (resolved once
//!   at startup) and the best-effort factory that mints a [`TraceSession`] for
//!   a turn. A failure to open the trace files is logged and swallowed —
//!   tracing must never break a user's invocation.
//! - The **current-session** process global: `start_turn` records the active
//!   turn's ULID so the HTTP layer (`client::retry::send`) can stamp every
//!   outgoing provider request with `X-Eridian-Session-Id`, the header
//!   astrophage (Phase 45D) echoes back into each `cache.lookup`. The global is
//!   sound for batch use, where turns are sequential within a process.

use std::path::PathBuf;
use std::sync::LazyLock;

use parking_lot::RwLock;

use super::session::{StartInfo, TraceSession};
use super::layout::TraceLayout;

/// The correlation header stamped on every provider request while a trace turn
/// is active (SPEC-001 §3.3 / Phase 42 cross-repo seam).
pub const SESSION_HEADER: &str = "X-Eridian-Session-Id";

/// Resolved SPEC-001 trace settings. `Some(_)` in the global config means
/// tracing is enabled for this invocation.
#[derive(Debug, Clone)]
pub struct SpecTraceConfig {
    /// Base directory holding `traces/` and `blobs/` (SPEC §1).
    pub base_dir: PathBuf,
    /// Parent (multi-turn) session id, if any. `None` for one-shot batch runs.
    pub parent_session_id: Option<String>,
    /// Test fixture id from `AICHAT_FIXTURE_ID`, surfaced in `session.start`.
    pub fixture_id: Option<String>,
}

/// The ULID of the turn currently being traced, read by the HTTP layer to stamp
/// the correlation header. `None` when no trace turn is active.
static CURRENT_SESSION: LazyLock<RwLock<Option<String>>> =
    LazyLock::new(|| RwLock::new(None));

/// Record the active turn's session id (called by [`start_turn`]).
pub fn set_current_session(session_id: &str) {
    *CURRENT_SESSION.write() = Some(session_id.to_string());
}

/// The active turn's session id, if a trace turn is in progress.
pub fn current_session() -> Option<String> {
    CURRENT_SESSION.read().clone()
}

/// Clear the active turn (called after `session.end`).
pub fn clear_current_session() {
    *CURRENT_SESSION.write() = None;
    // Drop any request captured but never emitted, so a failed turn cannot
    // leak its wire bytes into the next turn's `provider.request`.
    *CAPTURED_REQUEST.write() = None;
}

/// Phase 42E-1: a non-streaming provider request captured at the `reqwest`
/// boundary — the real serialized body and endpoint, awaiting emission by the
/// active trace turn as a wire-true `provider.request`.
#[derive(Debug, Clone)]
pub struct WireRequest {
    pub endpoint: String,
    pub body: Vec<u8>,
}

/// The most recent captured request, awaiting drain by `call_react`. Single
/// slot: like [`CURRENT_SESSION`] it is sound for batch use, where a turn's
/// provider calls are sequential and each is drained before the next is sent.
static CAPTURED_REQUEST: LazyLock<RwLock<Option<WireRequest>>> =
    LazyLock::new(|| RwLock::new(None));

/// Record the request `send` is about to dispatch. Best-effort; only called
/// while a trace turn is active.
pub fn capture_wire_request(endpoint: String, body: Vec<u8>) {
    *CAPTURED_REQUEST.write() = Some(WireRequest { endpoint, body });
}

/// Take and clear the captured request, if any. The active turn calls this
/// right after a provider call returns to emit a wire-true `provider.request`.
pub fn take_wire_request() -> Option<WireRequest> {
    CAPTURED_REQUEST.write().take()
}

/// Best-effort start of a trace turn. Mints the [`TraceSession`], records the
/// current-session global for header correlation, and returns the session. On
/// any I/O failure it logs and returns `None` so tracing never breaks a run.
pub fn start_turn(cfg: &SpecTraceConfig, mut info: StartInfo) -> Option<TraceSession> {
    info.fixture_id = cfg.fixture_id.clone();
    let layout = TraceLayout::new(&cfg.base_dir);
    match TraceSession::start(&layout, cfg.parent_session_id.clone(), info) {
        Ok(session) => {
            set_current_session(session.session_id());
            Some(session)
        }
        Err(e) => {
            log::warn!("eridian-trace: failed to start trace session: {e}");
            None
        }
    }
}

/// End a trace turn (if any) and clear the current-session global. A no-op when
/// `session` is `None`.
pub fn end_turn(
    session: Option<TraceSession>,
    exit_status: i32,
    tokens_in_total: u64,
    tokens_out_total: u64,
    cost_usd: Option<f64>,
) {
    if let Some(session) = session {
        session.end(exit_status, tokens_in_total, tokens_out_total, cost_usd);
    }
    clear_current_session();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_base(tag: &str) -> PathBuf {
        let id = format!("{:?}", std::thread::current().id());
        let dir = std::env::temp_dir()
            .join("aichat-wiring-test")
            .join(format!("{tag}-{}", id.replace(['(', ')', ' '], "")));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn header_constant_is_stable() {
        assert_eq!(SESSION_HEADER, "X-Eridian-Session-Id");
    }

    // Phase 42E-1: the wire-capture slot holds the most recent non-streaming
    // provider request (real serialized body + endpoint) until the active turn
    // drains it to emit a wire-true `provider.request`. Batch-sequential, like
    // CURRENT_SESSION.
    #[test]
    fn capture_and_take_wire_request_roundtrip() {
        capture_wire_request(
            "https://api.example.com/v1/chat".into(),
            b"{\"model\":\"m\"}".to_vec(),
        );
        let taken = take_wire_request().expect("a captured request");
        assert_eq!(taken.endpoint, "https://api.example.com/v1/chat");
        assert_eq!(taken.body, b"{\"model\":\"m\"}");
        // Draining clears the slot — the next turn must not re-read stale bytes.
        assert!(take_wire_request().is_none());
    }

    // The current-session global is process-wide, so the lifecycle assertions
    // that read/write it live in one test to avoid races with parallel threads.
    #[test]
    fn start_and_end_turn_manage_current_session() {
        let base = temp_base("start");
        let cfg = SpecTraceConfig {
            base_dir: base.clone(),
            parent_session_id: None,
            fixture_id: Some("fix-1".into()),
        };
        let session = start_turn(&cfg, StartInfo::default()).expect("session starts");
        let sid = session.session_id().to_string();
        // Current-session global now reflects the active turn.
        assert_eq!(current_session().as_deref(), Some(sid.as_str()));
        // The turn file exists under the configured base.
        let layout = TraceLayout::new(&base);
        assert!(layout.turn_path(&sid).exists());

        end_turn(Some(session), 0, 0, 0, None);
        // ...and is cleared after the turn ends.
        assert!(current_session().is_none());

        // end_turn with no session still clears a stale global.
        set_current_session("STALE");
        end_turn(None, 0, 0, 0, None);
        assert!(current_session().is_none());
    }
}
