//! ADR-0003: the dedicated OS writer thread + the non-blocking sender.
//!
//! ```text
//!   producers (tokio)                         OS thread "eridian-trace"
//!   ─────────────────                         ─────────────────────────
//!    TraceSender::emit ── try_send ──► bounded sync_channel ──► LineSink
//!         │ (full)                                              (seq + flush)
//!         ▼
//!    dropped_count += 1
//! ```
//!
//! Invariants enforced here:
//! - `emit` never blocks the request path: a full channel drops the event and
//!   bumps `dropped_count` (SPEC-001 §3.7 `trace.dropped`).
//! - `seq` is assigned by the writer (SPEC-001 §8.1), strictly monotonic from 0.
//! - Each event is one `write_all` of a `\n`-terminated line + `flush`, so a
//!   `tail -f` consumer never sees a partial line (ADR-0002, SPEC-001 §7.1).

use super::event::{Dropped, EventKind, PendingEvent};

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Default bounded-channel capacity (ADR-0003). ~1MB worst case at ~1KB/event.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1024;
/// How long the writer waits between heartbeats when idle (SPEC-001 §3.7).
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Max time the main thread waits for the writer to drain on shutdown.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Resolve the channel capacity, honoring `AICHAT_TRACE_CHANNEL_CAPACITY`.
fn resolve_capacity() -> usize {
    std::env::var("AICHAT_TRACE_CHANNEL_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_CHANNEL_CAPACITY)
}

/// A seq-stamping, flush-per-event JSONL sink. Factored out of the thread so it
/// can be tested against an in-memory buffer.
pub struct LineSink<W: Write> {
    writer: W,
    seq: u64,
}

impl<W: Write> LineSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer, seq: 0 }
    }

    /// Write one event as a `\n`-terminated line, stamping the next seq.
    /// Returns the seq used. The write is a single `write_all` + `flush` so
    /// streaming consumers never observe a partial line.
    pub fn emit_line(&mut self, ev: &PendingEvent) -> std::io::Result<u64> {
        let seq = self.seq;
        let mut line = ev.to_line(seq).into_bytes();
        line.push(b'\n');
        self.writer.write_all(&line)?;
        self.writer.flush()?;
        self.seq += 1;
        Ok(seq)
    }

    /// The seq that will be assigned to the next event.
    pub fn next_seq(&self) -> u64 {
        self.seq
    }
}

/// Process one received event: if drops have accumulated since the last event,
/// emit a `trace.dropped` first (so loss is visible in-band per ADR-0003),
/// then emit the event itself.
fn handle_event<W: Write>(
    sink: &mut LineSink<W>,
    dropped: &AtomicU64,
    session_id: &str,
    parent: &Option<String>,
    ev: PendingEvent,
) {
    let n = dropped.swap(0, Ordering::Relaxed);
    if n > 0 {
        let since_seq = sink.next_seq();
        let drop_ev = PendingEvent::new(
            session_id.to_string(),
            parent.clone(),
            EventKind::Dropped(Dropped { count: n, since_seq }),
        );
        let _ = sink.emit_line(&drop_ev);
    }
    let _ = sink.emit_line(&ev);
}

/// Cheap-to-clone, non-blocking event emitter (SPEC-001 §8.4: `Clone + Send +
/// Sync`). Holds the session identity so producers only pass an [`EventKind`].
#[derive(Clone)]
pub struct TraceSender {
    tx: SyncSender<PendingEvent>,
    dropped: Arc<AtomicU64>,
    session_id: Arc<str>,
    parent_session_id: Option<Arc<str>>,
}

impl TraceSender {
    /// Build a [`PendingEvent`] for `kind` and enqueue it without blocking. A
    /// full or disconnected channel drops the event and bumps `dropped_count`.
    pub fn emit(&self, kind: EventKind) {
        let ev = PendingEvent::new(
            self.session_id.to_string(),
            self.parent_session_id.as_ref().map(|p| p.to_string()),
            kind,
        );
        match self.tx.try_send(ev) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Number of events dropped so far (test/telemetry hook).
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

/// Owns the writer thread; joining it (or dropping the handle) shuts tracing
/// down cleanly.
pub struct TraceHandle {
    join: Option<JoinHandle<()>>,
}

impl TraceHandle {
    /// Join the writer thread, bounded by `SHUTDOWN_TIMEOUT`. The caller must
    /// drop every [`TraceSender`] first so the channel disconnects and the
    /// writer loop exits.
    pub fn shutdown(mut self) {
        if let Some(join) = self.join.take() {
            // Best-effort bounded join: poll `is_finished` so a stuck disk
            // write can never block process exit indefinitely (ADR-0003).
            let start = std::time::Instant::now();
            while !join.is_finished() {
                if start.elapsed() >= SHUTDOWN_TIMEOUT {
                    log::warn!("eridian-trace writer did not drain within timeout");
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = join.join();
        }
    }
}

/// The receiver loop: drain events, heartbeat on idle, exit on disconnect.
fn run_writer<W: Write>(
    rx: Receiver<PendingEvent>,
    mut sink: LineSink<W>,
    dropped: Arc<AtomicU64>,
    session_id: String,
    parent: Option<String>,
) {
    use std::sync::mpsc::RecvTimeoutError;
    let start = std::time::Instant::now();
    loop {
        match rx.recv_timeout(HEARTBEAT_INTERVAL) {
            Ok(ev) => handle_event(&mut sink, &dropped, &session_id, &parent, ev),
            Err(RecvTimeoutError::Timeout) => {
                let hb = PendingEvent::new(
                    session_id.clone(),
                    parent.clone(),
                    EventKind::Heartbeat(super::event::Heartbeat {
                        uptime_ns: start.elapsed().as_nanos() as u64,
                    }),
                );
                let _ = sink.emit_line(&hb);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Spawn the dedicated writer thread writing to `path` (append mode), returning
/// a clone-able sender and the join handle.
pub fn spawn_to_path(
    path: &Path,
    session_id: String,
    parent_session_id: Option<String>,
) -> std::io::Result<(TraceSender, TraceHandle)> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    Ok(spawn_to_writer(file, session_id, parent_session_id))
}

/// Spawn the writer thread over an arbitrary `Write` sink (used by tests).
pub fn spawn_to_writer<W: Write + Send + 'static>(
    writer: W,
    session_id: String,
    parent_session_id: Option<String>,
) -> (TraceSender, TraceHandle) {
    let (tx, rx) = sync_channel::<PendingEvent>(resolve_capacity());
    let dropped = Arc::new(AtomicU64::new(0));
    let sink = LineSink::new(writer);

    let writer_dropped = Arc::clone(&dropped);
    let writer_session = session_id.clone();
    let writer_parent = parent_session_id.clone();
    let join = std::thread::Builder::new()
        .name("eridian-trace".into())
        .spawn(move || {
            run_writer(rx, sink, writer_dropped, writer_session, writer_parent);
        })
        .expect("spawn eridian-trace writer thread");

    let sender = TraceSender {
        tx,
        dropped,
        session_id: Arc::from(session_id.as_str()),
        parent_session_id: parent_session_id.map(|p| Arc::from(p.as_str())),
    };
    (sender, TraceHandle { join: Some(join) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::trace_spec::event::{Heartbeat, SessionEnd, SessionStart};
    use std::sync::atomic::AtomicU64;

    fn hb(n: u64) -> EventKind {
        EventKind::Heartbeat(Heartbeat { uptime_ns: n })
    }

    fn parse_lines(bytes: &[u8]) -> Vec<serde_json::Value> {
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).expect("each line is valid JSON"))
            .collect()
    }

    // A Write that records into a shared buffer, so the thread can own it while
    // the test inspects it after join.
    #[derive(Clone)]
    struct SharedBuf(Arc<parking_lot::Mutex<Vec<u8>>>);
    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn line_sink_assigns_monotonic_seq_from_zero() {
        let mut sink = LineSink::new(Vec::new());
        let ev = PendingEvent::new("s".into(), None, hb(1));
        assert_eq!(sink.emit_line(&ev).unwrap(), 0);
        assert_eq!(sink.emit_line(&ev).unwrap(), 1);
        assert_eq!(sink.emit_line(&ev).unwrap(), 2);
    }

    #[test]
    fn line_sink_writes_newline_terminated_json_lines() {
        let mut sink = LineSink::new(Vec::new());
        sink.emit_line(&PendingEvent::new("s".into(), None, hb(1)))
            .unwrap();
        sink.emit_line(&PendingEvent::new("s".into(), None, hb(2)))
            .unwrap();
        let bytes = sink.writer.clone();
        assert!(bytes.ends_with(b"\n"));
        let lines = parse_lines(&bytes);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["seq"], 0);
        assert_eq!(lines[1]["seq"], 1);
    }

    #[test]
    fn handle_event_flushes_dropped_before_the_event() {
        let mut sink = LineSink::new(Vec::new());
        let dropped = AtomicU64::new(5);
        handle_event(
            &mut sink,
            &dropped,
            "s",
            &None,
            PendingEvent::new("s".into(), None, hb(9)),
        );
        let lines = parse_lines(&sink.writer);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["type"], "trace.dropped");
        assert_eq!(lines[0]["data"]["count"], 5);
        assert_eq!(lines[0]["data"]["since_seq"], 0);
        assert_eq!(lines[0]["seq"], 0);
        assert_eq!(lines[1]["type"], "trace.heartbeat");
        assert_eq!(lines[1]["seq"], 1);
        // Counter was reset by the swap.
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn handle_event_no_dropped_line_when_zero() {
        let mut sink = LineSink::new(Vec::new());
        let dropped = AtomicU64::new(0);
        handle_event(
            &mut sink,
            &dropped,
            "s",
            &None,
            PendingEvent::new("s".into(), None, hb(9)),
        );
        let lines = parse_lines(&sink.writer);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["type"], "trace.heartbeat");
    }

    #[test]
    fn sender_drops_when_channel_full_and_counts() {
        // Capacity 1, no draining receiver: 1 queued, the rest dropped.
        let (tx, _rx) = sync_channel::<PendingEvent>(1);
        let sender = TraceSender {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
            session_id: Arc::from("s"),
            parent_session_id: None,
        };
        sender.emit(hb(1)); // queued
        sender.emit(hb(2)); // dropped
        sender.emit(hb(3)); // dropped
        assert_eq!(sender.dropped_count(), 2);
    }

    #[test]
    fn full_pipeline_preserves_order_and_lifecycle_bookends() {
        let buf = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (sender, handle) =
            spawn_to_writer(SharedBuf(Arc::clone(&buf)), "01HSESS".into(), None);

        sender.emit(EventKind::SessionStart(SessionStart {
            aichat_version: "0.7.0-eridian".into(),
            config_hash: "sha256:cfg".into(),
            role: None,
            model_spec: None,
            fixture_id: None,
            cwd: "/work".into(),
            args: vec!["aichat".into()],
            env_subset: Default::default(),
        }));
        sender.emit(hb(1));
        sender.emit(EventKind::SessionEnd(SessionEnd {
            exit_status: 0,
            wall_time_ns: 100,
            tokens_in_total: 10,
            tokens_out_total: 5,
            cost_usd: None,
        }));

        // Disconnect the channel and drain.
        drop(sender);
        handle.shutdown();

        let lines = parse_lines(&buf.lock());
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["type"], "session.start");
        assert_eq!(lines[2]["type"], "session.end");
        // seq strictly monotonic from 0.
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(l["seq"], i as u64);
            assert_eq!(l["session_id"], "01HSESS");
        }
    }
}
