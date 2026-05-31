# ADR-0002: Trace format must be streaming-safe

**Status:** Accepted, 2026-04-25
**Decider:** project lead

## Context

A "structured trace" can be designed two ways:

- **Batch.** Aichat collects events during a turn and writes a complete file
  at the end. Consumers read finished files.
- **Streaming.** Aichat writes events as they happen. Consumers can `tail -f`
  the file and react in real time, mid-turn.

Streaming-safe is *not* a free extension of batch. They're the same wire
format but different engineering disciplines, and conflating them produces a
format that nominally streams but breaks under real consumers.

## Decision

**The trace format is streaming-safe from v0.1.** Every event is
self-contained, atomically written with explicit flush, and may be consumed
incrementally via `tail -f`-style patterns.

## Why streaming-safe is worth the discipline

1. **Testability.** Control-flow tests can read events as aichat emits them
   and assert on intermediate states ("retry fired before fallback was
   scheduled") rather than reconstructing causality from a final dump.
   This is the single biggest architectural payoff and justifies the
   discipline by itself.
2. **Real-time debug surfaces.** A future trace-explorer (marimo notebook,
   web UI, or a CLI `aichat trace watch`) becomes possible without a
   format change.
3. **Crash forensics.** A panic mid-turn leaves the trace in a recoverable
   state — the events written before the crash are valid and parseable.
   Batch formats with a "finalize" step lose everything if the finalize
   doesn't run.
4. **Schema discipline as a side effect.** Streaming-safe forces events to
   be self-contained, which forces clean event design. Batch formats
   tempt forward references and post-hoc reorganization, both of which
   make the schema worse over time.

## Engineering implications

The discipline has to be encoded explicitly because it's easy to erode under
deadline pressure:

- **Atomic line writes.** Each event serialized to a `Vec<u8>` ending in
  `\n`, then written with a single `write_all` followed by `flush`. No
  partial writes.
- **No buffered writes that hold events.** Rust's default `BufWriter`
  buffers up to 8KB, which makes streaming consumers see batches of events
  appear together. Either flush after every event (the chosen path) or
  use unbuffered I/O. We flush after every event.
- **No forward references.** An event that needs to reference another event
  carries enough information to be interpreted alone — typically the
  reference target's content hash. Consumers may correlate via the hash
  and the blob store, but they aren't required to.
- **Causal ordering enforced by emission order.** `provider.retry` must come
  after the `provider.request` it relates to. `provider.response` must come
  after the retry it succeeded. We don't get to reorganize for cleaner
  output.
- **Crash safety.** Each event-write is the only critical section. A panic
  between events leaves the file in a parseable state. A panic mid-write is
  rare and produces at worst one truncated line that streaming consumers
  must skip — hardening consumers against partial trailing lines is
  documented behavior.
- **Heartbeat events.** A `trace.heartbeat` event every N seconds during
  long-running operations (slow streaming responses, long tool execution)
  signals to consumers that aichat is alive. Cheap to emit, valuable for
  any real-time UI.

## Consequences

### Positive

- Testability win described above.
- Cleaner schema by construction.
- Crash-forensics value.

### Negative

- Slightly higher write throughput cost (flush per event vs. flush per
  batch). At expected event rates (<1000 events/sec sustained), this is
  noise. Measure if it ever becomes a concern.
- Schema is a little more verbose because events that would naturally
  reference one another now duplicate small amounts of metadata. Acceptable.

### Risks accepted

- **Discipline drift.** Future contributors may add events that violate the
  invariants (forward references, buffered writes, etc.). Mitigation:
  code-review checklist documented in `SPEC-001`, and a property-based test
  in the `eridian-trace` crate that fuzzes consumer behavior under partial
  reads to catch regressions.

## Implementation notes

This ADR mandates the discipline. The mechanics are in `ADR-0003`
(async writer thread) and the schema in `SPEC-001`. The relationship
between the three:

- ADR-0001: *what* artifact (a structured trace)
- ADR-0002: *how it's organized* (streaming-safe)
- ADR-0003: *how it's written* (dedicated thread, bounded channel)
- SPEC-001: *what's in it* (event types and schema)
