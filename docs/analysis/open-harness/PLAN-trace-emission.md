# PLAN: Phase 1 — Trace Emission

**Status:** Ready to start
**Depends on:** SPEC-001, ADR-0001, ADR-0002, ADR-0003, ADR-0004

This plan breaks the trace-emission work into PR-sized tasks. Each PR is
small enough to fit in a single coding-agent session, has clear acceptance
criteria, and ships with a Showboat demo per `ADR-0004`.

Sequence matters. Earlier PRs are dependencies of later ones. Don't reorder
without justification.

## Pre-flight

### PF-1: Audit retry/backoff for tokio time abstractions

**Owner:** human (cannot be safely delegated to agent without code access)
**Outputs:** an issue or note declaring whether aichat's retry path uses
`tokio::time::sleep` (good) or `std::thread::sleep` / wall-clock arithmetic
(needs fixing before Phase 2).

If the audit reveals non-tokio time usage, file a separate refactor PR
before Phase 2 starts. Phase 1 does not depend on this — but Phase 2 does.

### PF-2: Confirm aichat's existing CLI flag conventions

**Outputs:** a note in the Phase 1 implementation tracking issue
documenting how aichat parses CLI flags (clap? argh?), so PR-2 below uses
the same idiom.

## PR-1: `eridian-trace` crate skeleton

**Goal:** publishable internal crate with the schema as Rust types and a
streaming JSONL parser.

**Scope:**

- `crates/eridian-trace/Cargo.toml`
- `crates/eridian-trace/src/lib.rs` — module structure
- `crates/eridian-trace/src/schema.rs` — `TraceEvent`, `Trace`, every
  `data` payload as a typed enum or struct
- `crates/eridian-trace/src/parse.rs` — `parse_trace_file` (eager) and
  `parse_trace_stream` (streaming, tolerates trailing partial lines)
- Unit tests covering: every event type round-trips through serde,
  malformed JSON skips gracefully, partial trailing line is tolerated,
  unknown event types deserialize to a generic variant
- A `proptest`-based property test that random byte sequences don't
  panic the parser

**Out of scope:** the writer, the redactor, the channel, integration with
aichat. This is just the schema + parser.

**Acceptance:**

- `cargo test -p eridian-trace` is green
- Crate compiles standalone, no aichat dependency
- A Showboat demo at `demos/eridian-trace-crate.md` shows: a sample JSONL
  file from `SPEC-001`, parsed via `parse_trace_file`, with each event
  type rendered as Rust debug output

**Estimated size:** medium PR (300–500 LOC + tests).

## PR-2: CLI flags and config plumbing

**Goal:** wire up the user-facing config surface for tracing without yet
emitting any events.

**Scope:**

- `--trace-out <path>` flag on aichat's main CLI
- `--no-trace` flag
- `AICHAT_TRACE`, `AICHAT_TRACE_DIR`, `AICHAT_TRACE_VERBOSE`,
  `AICHAT_TRACE_CHANNEL_CAPACITY`, `AICHAT_FIXTURE_ID` env var parsing
- A `TraceConfig` struct holding the resolved settings
- The default path resolution: `${XDG_STATE_HOME:-~/.local/state}/aichat/traces/`
- A `--trace-config` (debug-only) subcommand or `aichat --info` extension
  that prints the resolved trace config without starting a session
- Unit tests for env var precedence, XDG fallback, etc.

**Out of scope:** any actual event emission, the writer thread, the
channel. Just resolving config.

**Acceptance:**

- `aichat --trace-config` prints the resolved settings
- All unit tests pass
- A Showboat demo at `demos/trace-config.md` shows: setting different
  combinations of env vars and flags, then running `aichat --trace-config`
  to confirm resolution

**Estimated size:** small PR (~200 LOC + tests).

## PR-3: Writer thread + bounded channel

**Goal:** the OS-thread writer infrastructure described in `ADR-0003`,
with no actual aichat integration yet.

**Scope:**

- `crates/eridian-trace/src/writer.rs` — `TraceWriter`, `TraceSender`
- `crossbeam-channel` dependency
- `TraceSender::try_send` with drop accounting (atomic counter)
- `TraceWriter` running on `std::thread`, looping on `recv_timeout` with
  `HEARTBEAT_INTERVAL` for heartbeats
- Per-event flush, no `BufWriter`
- `panic::catch_unwind` wrapping per-event work so a malformed event
  doesn't kill the writer
- Clean shutdown on channel disconnect; bounded join timeout
- Unit tests: events written in order, `seq` is monotonic, drops are
  emitted as `trace.dropped` events, panic in serialization is contained,
  heartbeat fires on quiet timeout

**Out of scope:** aichat integration, redaction, blob store. Writer takes
serializable events and writes them.

**Acceptance:**

- `cargo test -p eridian-trace` is green including new writer tests
- A test that floods the channel and observes `trace.dropped` events
  passes
- A Showboat demo at `demos/trace-writer.md` exercises the writer
  standalone (a small Rust binary in the demo) showing: normal writes,
  channel-full drops, clean shutdown

**Estimated size:** medium PR (400–600 LOC + tests).

## PR-4: Blob store

**Goal:** content-addressed payload store described in `SPEC-001` §4.

**Scope:**

- `crates/eridian-trace/src/blob.rs` — `BlobStore` with `put(bytes) -> Hash`
  and `get(hash) -> Option<Vec<u8>>`
- SHA-256 via `sha2`
- Sharded directory layout (first 4 hex chars become 2 levels deep)
- `O_EXCL`-style write-once: identical content writes are no-ops
- Unit tests: deduplication works, sharded paths land in expected places,
  concurrent writes don't corrupt

**Out of scope:** integration with the writer. Blob store is callable
standalone.

**Acceptance:**

- `cargo test -p eridian-trace` is green
- A Showboat demo at `demos/blob-store.md` shows: writing two events
  with identical large payloads, observing only one blob on disk

**Estimated size:** small PR (~250 LOC + tests).

## PR-5: Redaction layer

**Goal:** the redaction described in `SPEC-001` §6.

**Scope:**

- `crates/eridian-trace/src/redact.rs` — `Redactor` trait, default
  implementation with the rule set in §6
- `redaction.yaml` parser for user-configured patterns
- Applied to `TraceEvent`s in the `TraceWriter` before serialization
- Unit tests: API key patterns are scrubbed, `Authorization` headers in
  request bodies are stripped, allowlist of env vars is respected, custom
  patterns from YAML work

**Acceptance:**

- All unit tests pass
- A Showboat demo at `demos/redaction.md` shows: an aichat invocation
  with `OPENAI_API_KEY=sk-xxx` set, then `cat`ing the trace and
  `grep`ping for `sk-` to show no leak

**Estimated size:** small-medium PR (~300 LOC + tests).

## PR-6: Aichat integration — session lifecycle events

**Goal:** wire aichat to emit `session.start` and `session.end` only.
Establishes the integration pattern. Subsequent PRs add more event types.

**Scope:**

- `TraceSender` initialized in `main()` based on `TraceConfig` from PR-2
- `TraceSender` plumbed via `Arc` through aichat's existing context
  passing (or a `OnceCell` if cleaner)
- `session.start` emitted at the start of each turn
- `session.end` emitted at turn end with exit status, wall time, totals
- ULID generation for `session_id`
- Manifest writes for parent-session linkage (per `SPEC-001` §1)
- Per-turn JSONL files created and closed correctly

**Out of scope:** all other event types. They come in PR-7 through PR-12.

**Acceptance:**

- Running `aichat hello` produces a turn-*.jsonl with exactly two events:
  `session.start` and `session.end`
- A Showboat demo at `demos/session-lifecycle.md` shows: an aichat
  invocation, the resulting manifest update, the per-turn JSONL with
  exactly the expected events
- Existing aichat unit tests still pass; no regressions

**Estimated size:** medium PR (~400 LOC), high integration risk.

## PR-7: Provider request / response / retry / fallback events

**Goal:** wire `provider.*` events. This is the most testing-relevant
slice.

**Scope:**

- `provider.request` emitted before each HTTP call
- `provider.response` emitted on successful response
- `provider.retry` emitted per retry attempt with correct `trigger`
  classification
- `provider.fallback` emitted on provider switch
- Request/response bodies stored in blob store, hashes referenced from
  events
- Unit tests where possible (mock the HTTP client) for trigger
  classification

**Out of scope:** wiremock-driven integration tests (those are Phase 2).

**Acceptance:**

- A real aichat invocation against Anthropic/OpenAI produces correct
  `provider.request` and `provider.response` events with hashes that
  resolve to real blobs
- A Showboat demo at `demos/provider-events.md` shows: an aichat
  invocation, the resulting `provider.*` events, and a blob resolution
  via `aichat trace show`

**Estimated size:** large PR (~600 LOC + tests). Consider splitting if it
balloons.

## PR-8: Context events

**Goal:** wire `context.*` events.

**Scope:**

- `context.system_prompt_built` after assembly
- `context.role_applied` when a role is in effect
- `context.rag_retrieved` per RAG query

**Acceptance:**

- An aichat invocation with a role and RAG produces all three events
- A Showboat demo at `demos/context-events.md`

**Estimated size:** small-medium PR (~300 LOC + tests).

## PR-9: Tool events

**Goal:** wire `tool.*` events.

**Scope:**

- `tool.requested` when the model emits a tool call
- `tool.denied` when the whitelist blocks
- `tool.executed` after execution with stdout/stderr captured (with
  truncation cap from `SPEC-001` §3.4)

**Acceptance:**

- An aichat invocation that triggers a tool call produces correct events
- A Showboat demo at `demos/tool-events.md` includes both an allowed and
  a denied tool call

**Estimated size:** medium PR (~400 LOC + tests).

## PR-10: Output events

**Goal:** wire `output.final` and `output.chunk` (verbose).

**Scope:**

- `output.final` emitted at end of model response
- `output.chunk` gated behind `AICHAT_TRACE_VERBOSE=1`

**Acceptance:**

- Both verbose and non-verbose modes produce correct events
- A Showboat demo at `demos/output-events.md`

**Estimated size:** small PR (~200 LOC + tests).

## PR-11: `aichat trace show` command

**Goal:** the CLI command described in `SPEC-001` §4 for human inspection.

**Scope:**

- New `aichat trace show <session_id>` subcommand
- Resolves blob hashes inline
- Pretty-prints events in chronological order
- A `--json` flag that emits the resolved trace as JSON for piping

**Acceptance:**

- The command resolves blobs correctly
- A Showboat demo at `demos/trace-show.md` walks through running aichat,
  then inspecting the resulting trace

**Estimated size:** small PR (~250 LOC + tests).

## PR-12: Integration acceptance suite + Phase 1 close-out

**Goal:** verify the eight acceptance criteria from `SPEC-001` §10
end-to-end.

**Scope:**

- A test harness in `tests/phase1_acceptance/` that exercises each
  criterion
- A burst-load benchmark using `criterion` to verify the p99 latency
  invariant (criterion 7)
- A SIGKILL-mid-turn test for criterion 8
- Documentation updates: a Phase 1 retrospective in
  `docs/architecture/RETRO-phase1.md` capturing what we'd do differently

**Acceptance:**

- All eight criteria from `SPEC-001` §10 pass
- A Showboat demo at `demos/phase1-acceptance.md` walks through each
  criterion with output

**Estimated size:** medium PR (~500 LOC + benches + tests).

## Sequencing summary

```text
PF-1, PF-2 (audit, no code)
   ↓
PR-1 (eridian-trace crate)
   ↓
PR-2 (CLI / config) ─────────┐
   ↓                          │
PR-3 (writer thread)          │
   ↓                          │
PR-4 (blob store)             │
   ↓                          │
PR-5 (redaction) ─────────────┤
                              ↓
                         PR-6 (session lifecycle, integration begins)
                              ↓
                         PR-7 (provider events)
                              ↓
                         PR-8 (context) ──┬── these three are independent
                         PR-9 (tool)      │   and can land in any order
                         PR-10 (output) ──┘
                              ↓
                         PR-11 (trace show)
                              ↓
                         PR-12 (acceptance suite)
```

PR-1 through PR-5 build the `eridian-trace` crate independently of aichat.
They can ship as soon as they're ready and don't risk the aichat user
experience. PR-6 is the integration milestone — once it lands, aichat is
emitting traces (just not full-fidelity ones) and the rest is fleshing
out event types.

## Open questions

These are tracked here so they're not lost; resolve before the affected
PR starts:

1. **PR-2:** Does aichat's existing CLI live in clap or another framework?
   Match the idiom.
2. **PR-3:** What's the right `HEARTBEAT_INTERVAL` default? 30s is a
   guess. Revisit after PR-12.
3. **PR-6:** REPL mode multi-turn — is the parent-session ID generated
   at REPL start or at process start? `SPEC-001` §1 implies REPL start.
   Confirm by reading aichat's session code.
4. **PR-7:** Does aichat have a single HTTP client or one per provider?
   Determines where to instrument.
5. **PR-9:** Is the tool whitelist enforced before or after tool args are
   built? Affects whether `tool.requested` always precedes `tool.denied`.

When in doubt, file an "Open Question" in the PR description and pause
for human resolution rather than guessing.
