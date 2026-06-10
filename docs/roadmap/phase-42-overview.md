# Phase 42 — Trace Emission (SPEC-001 Ph1) : Overview — Epic 15 (Observability Keystone)

**Status:** **Done** — 42A–D shipped (2026-06-09) · **Owner:** aichat · **Horizon:** Now (pulled forward)

> **Goal.** Build the **structured-trace keystone** — the single artifact every downstream
> consumer reads: astrophage replay (Epic 16), the test harness (Phase 43), training extraction
> (Phase 44), and observability. aichat today has ad-hoc `--trace` / `AICHAT_TRACE=1` JSONL
> (Phases 8F/8G); Phase 42 promotes that to the **SPEC-001 contract** — a versioned event
> schema, a content-addressed blob store, a dedicated async writer thread, and a record-mode
> redaction gate. 37E's `cache.lookup` event lands on top of this emitter, which is why the
> emitter must exist first.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 42A | SPEC-001 event schema (v0.1, the 13 event types) + dedicated **OS writer thread** (bounded MPSC, streaming-safe per [`ADR-0002`](../analysis/caching/ADR-0002-streaming-safe.md)/[`ADR-0003`](../analysis/caching/ADR-0003-async-writer-thread.md)) | **Done** (`src/utils/trace_spec/`: ULID + 17-variant envelope + `LineSink`/`TraceSender` writer thread + env_subset redaction gate; std `sync_channel`, zero new deps) |
| 42B | Content-addressed blob store (`traces/blobs/<sha256>`) + **record-mode redaction gate** (strip auth headers, pattern-scrub secrets *before* any byte hits disk) | **Done** (`blob.rs`: SHA-256, two-level sharded `blobs/ab/cd/<hex>`, write-once via `create_new`/O_EXCL; `redact.rs`: recursive `strip_auth_headers` + `redacted_body_hash` so `messages_hash` is key-independent) |
| 42C | Full lifecycle event coverage (request / response / tool / pipeline-stage / retry / error / `cache.lookup`) + per-parent `manifest.jsonl` | **Done** (`layout.rs`: SPEC §1 paths + `manifest.jsonl`; `session.rs`: `TraceSession` orchestrator emitting the full lifecycle set with large payloads offloaded to the blob store. Call-site wiring into `call_react`/`main` is 42D.) |
| 42D | `--trace` / `AICHAT_TRACE` surface unification (supersede ad-hoc 8F/8G), `schema_version` stamping, session-ULID correlation (`X-Eridian-Session-Id`) | **Done** (`wiring.rs`: `SpecTraceConfig` + `start_turn`/`end_turn` + current-session global; `TraceSession` minted in `call_react` from global config and emitting session.start → provider.request/response → tool.requested/executed → output.final/error → session.end; `X-Eridian-Session-Id` stamped at the single `retry::send` chokepoint. Opt-in via the existing `--trace`/`AICHAT_TRACE`; SPEC §1 default-on held back as an Ask-First behavior change. Verified end-to-end.) |
| 42E | **Transport-boundary capture** — move `provider.*` capture to the `reqwest` boundary so the events carry ground-truth wire bytes (real body / status / finish_reason, per-attempt, SSE chunk timing) instead of 42D's pre-send intent stubs. In-process, no mitmproxy. | Planned → [phase-42e-overview.md](phase-42e-overview.md) |

> **42D carry-over.** 42D's `provider.request`/`response` payloads are reconstructed from
> pre-send *intent* (a `{"text":…}` stub, hardcoded `200`, inferred `finish_reason`); the
> `OutputChunk` event is unwired. **Phase 42E** closes this [`EVAL-001`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md) §5 gap in-process.

## Cross-repo seams

- Emits the **`X-Eridian-Session-Id`** correlation ULID that astrophage (Phase 45D) echoes into
  every `cache.lookup` event, so a cache/cassette hit correlates to its originating turn.
- The trace **blob store** is the source for aichat-side deterministic **tool-replay**
  (Phase 46C), keyed `(tool_name, args_hash)`.

## Dependencies

- **Upstream:** none — foundational. Realizes [`PLAN-trace-emission.md`](../analysis/caching/PLAN-trace-emission.md) Phase 1.
- **Blocks:** Phase 43 (test harness), Phase 44 (projections / training), Phase 45D + Phase 46 (astrophage correlation + tool-replay).
- **Supersedes:** the ad-hoc trace from Phases 8F/8G ([`phase-8-data-observability.md`](phase-8-data-observability.md)).

## Acceptance criteria

1. A turn emits one `traces/<session_id>.jsonl` of **versioned** events plus a content-addressed blob store.
2. **No event contains a plaintext key** — the redaction gate runs at record time, not as a later pass.
3. Trace writes **never block the request path** — async thread, bounded channel with defined backpressure/drop policy ([`ADR-0003`](../analysis/caching/ADR-0003-async-writer-thread.md)).
4. Every event carries `schema_version`; the `cache.lookup` event slot is reserved for 37E.

## Grounding docs

[`SPEC-001-trace-format.md`](../analysis/caching/SPEC-001-trace-format.md) ·
[`ADR-0001-trace-as-keystone.md`](../analysis/caching/ADR-0001-trace-as-keystone.md) ·
[`ADR-0002`](../analysis/caching/ADR-0002-streaming-safe.md) ·
[`ADR-0003`](../analysis/caching/ADR-0003-async-writer-thread.md) ·
[`PLAN-trace-emission.md`](../analysis/caching/PLAN-trace-emission.md) ·
[`ECOSYSTEM.md`](../analysis/caching/ECOSYSTEM.md)
