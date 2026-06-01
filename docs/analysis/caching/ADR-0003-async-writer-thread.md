# ADR-0003: Trace writes happen on a dedicated OS thread

**Status:** Accepted, 2026-04-25
**Decider:** project lead

## Context

Trace emission sits on the hot path of every aichat invocation. Every prompt
the user types, every model call, every tool execution generates events. The
trace writer must satisfy three constraints simultaneously:

1. **Never block aichat's request path.** A slow disk, a full disk, or a
   crashed external monitor process must not stall the user's prompt.
2. **Never lose events silently.** If we drop events under load, the test
   harness produces wrong results. Drops must be observable.
3. **Never panic the parent process.** A trace-writer bug that brings down
   the user's chat session is unacceptable.

These constraints are the standard async-logging design problem. There are
three viable Rust approaches:

- **Synchronous, in-line writes.** Simple but violates constraint 1 under
  any disk pressure.
- **A dedicated tokio task on aichat's existing tokio runtime.** Avoids
  the OS-thread overhead. But couples trace I/O scheduling to aichat's
  request scheduling — under load, both compete for the same workers.
  And bugs in the trace path can affect aichat's task scheduling
  (e.g., a CPU-bound bug in serialization stalls request handlers).
- **A dedicated OS thread with channel-based handoff.** Strong isolation:
  trace I/O runs on a thread that cannot affect aichat's tokio runtime.
  Slightly higher resource cost (one extra OS thread) and more
  ceremony.

## Decision

**Use a dedicated OS thread with a bounded channel for handoff,** built on
native Rust thread-safety primitives (`std::thread::spawn`, `crossbeam_channel`).

### Concurrency design

```text
       aichat tokio runtime                       OS thread
       ────────────────────                       ─────────
        request handlers
              │
              │ TraceEvent
              ▼
       ┌──────────────┐ try_send  ┌─────────────────────┐
       │ TraceSender  │ ────────► │ bounded channel      │
       │ (Clone)      │           │ (crossbeam, cap N)   │
       └──────────────┘           └─────────────────────┘
                                            │
                                            │ recv (blocking)
                                            ▼
                                  ┌─────────────────────┐
                                  │  TraceWriter         │
                                  │  - serialize         │
                                  │  - redact            │
                                  │  - blob store        │
                                  │  - flush+fsync       │
                                  └─────────────────────┘
                                            │
                                            ▼
                                  files on disk
```

**Sender side (in aichat's tokio runtime).** A cheap clone-able
`TraceSender` holding a `crossbeam_channel::Sender<TraceEvent>`. Calls to
emit events use `try_send`, which is non-blocking:

- Channel has capacity → event enqueued, return immediately.
- Channel is full → `try_send` returns `Err(TrySendError::Full(_))`.
  We increment a `dropped_count` (atomic) and discard the event.

This means aichat **never blocks on trace I/O** under any condition. Trace
emission is best-effort. We accept event loss under sustained pressure
in exchange for guaranteed non-blocking semantics on the request path.

**Receiver side (dedicated OS thread).** Spawned once at aichat startup via
`std::thread::Builder::new().name("eridian-trace").spawn()`. Holds the
file handles for the current turn's JSONL file, the blob-store directory,
and the manifest file. Loops:

```rust
loop {
    match rx.recv_timeout(HEARTBEAT_INTERVAL) {
        Ok(event)  => write_event(event),
        Err(RecvTimeoutError::Timeout) => write_heartbeat(),
        Err(RecvTimeoutError::Disconnected) => break,
    }
}
```

After the turn completes (signaled by a `session.end` event flowing
through), the writer flushes and `fsync`s, then awaits the next turn.

**Drop accounting.** The `dropped_count` atomic is checked on every send.
When it transitions from 0 to nonzero, the *next* successful send queues a
`trace.dropped` event reporting the count and resets to zero. This means
drops are visible in the trace itself, not silently swallowed.

**Shutdown.** On aichat exit:

1. Send a `session.end` event with exit status.
2. Drop the `TraceSender`. This closes the channel.
3. The writer thread drains pending events, writes the final flush, and
   exits cleanly via `RecvTimeoutError::Disconnected`.
4. The main thread `join()`s the writer thread with a short timeout
   (5 seconds). If the writer doesn't exit in time — likely due to a
   stuck disk write — we log a warning and exit anyway. We never let
   trace cleanup block process exit indefinitely.

### Crate selection

- **`crossbeam-channel`** for the bounded channel. `std::sync::mpsc::sync_channel`
  is technically sufficient, but `crossbeam-channel` has better
  performance, a richer API (`recv_timeout`, `select!`), and is the
  community standard for this pattern. Tradeoff: one external crate.
  Acceptable.
- **`std::thread::spawn`** for the writer thread. No need for tokio-rs
  on this thread; the work is purely synchronous file I/O.
- **`sha2`** for content hashing in the blob store.
- **`serde` + `serde_json`** for event serialization.

### Configuration

- `EVENT_CHANNEL_CAPACITY` — default 1024 events. Tunable via
  `AICHAT_TRACE_CHANNEL_CAPACITY` env var. At 1024 events of ~1KB each,
  worst-case memory is ~1MB.
- `HEARTBEAT_INTERVAL` — default 30 seconds. Hardcoded for v0.1; make
  configurable later if needed.
- `SHUTDOWN_TIMEOUT` — 5 seconds. Hardcoded.

## Consequences

### Positive

- **Total isolation between trace I/O and aichat's request handling.** A
  bug in the writer thread cannot deadlock the tokio runtime. A slow disk
  cannot increase user-visible request latency.
- **Observable drops, not silent ones.** `trace.dropped` events make
  pressure visible to consumers.
- **Clean shutdown semantics.** Process exit isn't gated on disk I/O.
- **Easy to test.** The writer thread can be replaced with a test double
  in unit tests by injecting a `TraceSender` whose receiving side is a
  test harness rather than a real thread.

### Negative

- **One extra OS thread** for the lifetime of the aichat process. Cost is
  trivial (one OS thread is ~8MB of stack and minimal scheduler overhead).
- **External crate dependency** on `crossbeam-channel`. Trivial.
- **Channel capacity is a tunable knob.** Setting it too low causes drops
  under bursty load; too high wastes memory. Default of 1024 is a starting
  point; revise if telemetry shows pressure.

### Risks accepted

- **The writer thread can panic.** If it does, future events are dropped
  silently because the receiver is gone — `try_send` returns `Disconnected`
  on every subsequent call. Mitigation: the writer thread wraps its
  per-event work in `std::panic::catch_unwind`; an event whose serialization
  panics is logged and skipped, not allowed to take down the writer.
- **fsync is not called per-event.** We `flush` every event but `fsync`
  only at turn boundaries. A power loss between flush and fsync can lose
  events. Acceptable for a trace log; if anyone ever wants per-event
  durability, they can add it.

## Considered alternatives

### Alt 1: Dedicated tokio task instead of OS thread

Rejected for the coupling reasons above. The OS-thread approach is the
strict superset: it does everything the tokio task would do, with stronger
isolation. The cost of one OS thread doesn't justify the lost isolation.

### Alt 2: Synchronous writes with a `RwLock`-guarded writer

Rejected because lock contention on the writer becomes a serialization
point on the request path. Under concurrent tool execution or streaming
responses, this can cause noticeable latency spikes.

### Alt 3: `tracing` crate with a custom subscriber

Tempting because `tracing` is the Rust ecosystem standard for structured
logs. Rejected because:

- `tracing`'s subscriber model is shaped around log levels and span
  hierarchies, not the event-typed schema we need.
- Bridging `tracing` events to our schema would add an extra serialization
  step.
- We'd be fighting `tracing`'s model rather than building cleanly on
  primitives.

We may add a `tracing` *layer* later that emits to our trace channel — for
the convenience of using `info!`/`warn!` macros — but the channel-and-thread
machinery is the source of truth, not the `tracing` subscriber registry.

### Alt 4: Write to a fixed-size ring buffer in shared memory; let an
external process tail it

Rejected as over-engineered for v0.1. May make sense if Eridian ever ships
a daemon companion process for trace ingestion, but that's out of scope.
JSONL on disk is universally tooling-friendly.

## Sources and prior art

- `crossbeam-channel` documentation:
  <https://docs.rs/crossbeam-channel/>
- The `tracing` crate's `tracing-appender` pattern, which uses a similar
  thread-and-channel design for non-blocking file output:
  <https://docs.rs/tracing-appender/>
- Conversations with the project lead establishing native-Rust-thread-safety
  as the chosen primitive set, deliberately avoiding tokio runtime coupling.
