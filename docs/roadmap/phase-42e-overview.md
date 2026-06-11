# Phase 42E — Transport-Boundary Capture : Overview — Epic 15 (Observability Keystone)

**Status:** In progress — 42E-1 shipped (2026-06-11); 42E-2/3 planned · **Owner:** aichat · **Horizon:** Next

> **Goal.** Close the **intent-vs-wire fidelity gap** in the keystone trace. Phase 42D
> emits `provider.request` / `provider.response` from `call_react`, reconstructed from
> aichat's *pre-send intent* — not from what actually crossed the socket. 42E moves provider
> capture to the **`reqwest` boundary**, in-process, so those events carry **ground-truth wire
> content**: the real serialized request body, real status, real finish reason, per-attempt,
> and real SSE chunk timing. This is the one place [`EVAL-001`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md) §4.1/§5 found SPEC-001 genuinely weaker than an external proxy — fixed without the proxy.

## Why now

Phase 42D shipped the emitter and the correlation seam, but the provider payloads are
placeholders. Verified current state (`src/client/common.rs` `call_react`):

| Field | Today | Should be |
|---|---|---|
| `provider.request.messages_hash` | hash of `{"text": input.text()}` stub | hash of the actual serialized wire body (post header-injection, post-redaction) |
| `provider.request.endpoint` | `""` | real request URL |
| `provider.request.params` | `{"stream": bool}` only | real sampling params (temperature, top_p, …) |
| `provider.response.status` | hardcoded `200` | real HTTP status |
| `provider.response.finish_reason` | inferred from tool presence | from the wire response |
| `provider.response.request_body_hash` | `""` | linked to the request blob |
| `provider.response.response_body_hash` | parsed assistant *text* | raw response bytes |
| `output.chunk` (SSE frames + timing) | **never emitted** (`OutputChunk` variant exists in `event.rs`, unwired in `session.rs`) | one per frame with real inter-chunk timing |
| per-retry events | retries inside `retry.rs` are invisible above it | one `provider.request`/`response`/`retry` per attempt |
| tokens in/out, latency | real (ride `CallMetrics`) | unchanged |

Downstream impact: training (Phase 44) attributes high-fidelity entity labels (52D) onto
low-fidelity, reconstructed provider bytes; cassette correlation (45D / 46C) keys on
`messages_hash`, which is currently a hash of *intent*, not the wire — weakening replay
key-stability ([`SPEC-astrophage`](../architecture/integrated-architecture/SPEC-astrophage.md) §9.2).

## Decision record — in-process, not mitmproxy

Both [`EVAL-001`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md) (§6) and
[`EVAL-0005`](../analysis/caching/EVAL-0005-build-vs-integrate-replay.md) (§6) reject
mitmproxy/`mitmdump`. The load-bearing reason: aichat's capture targets are **reverse
gateways** (the trace emitter; astrophage over `base_url`), and a forward MITM proxy's only
unique power — decrypting TLS for a connection it doesn't control — is moot when aichat
*chooses* to send to an endpoint it owns. Every proxy cost (CA trust, Python on the hot path,
foreign event model, breaks default-on + `tokio::time::pause()` + inline redaction) survives;
the lone benefit evaporates. **42E realizes EVAL-001 §5 in-process.**

**Boundary caveat (honest scope).** A `reqwest_middleware`/`tower` layer observes the
`reqwest::Request` *before* hyper serializes/compresses/h2-frames it, and the *decoded*
`Response` after transport. It captures the same **semantic** wire content (JSON body,
headers aichat set, status, decoded stream) — not the literal on-socket encoding (gzip/h2
frames). For LLM training and replay keys that is the right layer (consumers want the JSON,
not the frames). Literal socket bytes would need a custom hyper connector/TLS tap and are
**out of scope** — more than the project needs.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 42E-1 | **Request capture at the `reqwest` boundary.** Capture the actual serialized request body + real endpoint at the single `send()` chokepoint (`src/client/retry.rs`); the active turn drains it to emit a wire-true `provider.request`. Redaction stays inline, before the blob store. Replaces the `{"text":…}` stub. | **Done** (aichat) |
| 42E-2 | **Response + per-attempt capture.** Real status, wire `finish_reason`, raw response body, `request_body_hash` linkage; one `provider.request`/`provider.response`/`provider.retry` triple **per attempt** (retries currently invisible above `retry.rs`). | Planned |
| 42E-3 | **Streaming chunk capture.** Wire the `OutputChunk` event: add a `TraceSession::output_chunk` emitter, call it per SSE frame on the streaming path with real inter-chunk timing. Also closes the correlation-header gap (streaming bypasses `retry::send`). | Planned |

### 42E-1 implementation note (shipped)

- **Capture point.** `retry::send` (the single non-streaming chokepoint) recovers
  the wire-true endpoint + serialized body via the pure `wire_from_builder`
  helper (`try_clone().build()` → `url()` + `body().as_bytes()`) and stores it in
  a per-turn slot (`trace_spec::wiring`: `capture_wire_request` / `take_wire_request`,
  a `WireRequest { endpoint, body }`). **Guarded on an active trace turn**, so
  tracing-off (the default) pays nothing on the hot path. `clear_current_session`
  drops any undrained capture so a failed turn cannot leak bytes into the next.
- **Emission moved after the call.** `call_react` now emits `provider.request`
  *after* the provider call returns (still before `provider.response`, preserving
  the request→response seq order), draining the slot so `messages_hash` and
  `endpoint` reflect the real wire body. Single-writer / one-seq is unchanged —
  only the *bytes* are captured at the wire; emission stays session-side.
- **Streaming + failure paths.** The streaming path bypasses `retry::send`, so no
  capture exists and `provider.request` falls back to the input-text stub (42E-3).
  A failed non-streaming call still emits the (captured) request before the error
  event — the request *was* sent.
- **Verified.** `wire_from_builder_extracts_endpoint_and_body` (retry.rs) +
  `capture_and_take_wire_request_roundtrip` (wiring.rs) unit tests; end-to-end
  `--trace` run shows `endpoint: …/v1/chat/completions` and a 429-byte body blob
  containing the real `{model, messages:[system,user]}` payload (was the
  `{"text":…}` stub). 787 tests pass. No `schema_version` change.

## Dependencies

- **Upstream:** Phase 42 (emitter + the `retry::send` seam) — **Done**.
- **Feeds:** Phase 44 (trustworthy training bytes); Phase 45D / 46C (wire-true `messages_hash`
  strengthens cassette/replay key-stability); Phase 43 (control-flow tests can assert real
  status / per-attempt structure).
- **Independent of:** 52D (entity attribution) — orthogonal; 52D labels *whose capabilities
  ran*, 42E records *what crossed the wire*. Both land in `session.start` / `provider.*`
  without touching each other.

## Acceptance criteria

1. `provider.request.messages_hash` is the hash of the **actual serialized wire body**
   (post header-injection, post-redaction), not the pre-send intent stub.
2. `provider.response` carries the **real** status and `finish_reason` from the wire;
   `request_body_hash` links to the stored request blob.
3. A retried call emits **one `provider.request`/`response`/`retry` per attempt**.
4. The streaming path emits `output.chunk` per SSE frame with real inter-chunk timing.
5. **Request-path p99 regression < 5%** ([`SPEC-001`](../analysis/caching/SPEC-001-trace-format.md) §10 crit 7) — capture stays non-blocking (reuse the bounded `TraceSender`, `try_send` drop policy per [`ADR-0003`](../analysis/caching/ADR-0003-async-writer-thread.md)).
6. No `schema_version` bump — 42E fills existing event fields with truthful values; it does
   not change the schema.

## Risks

- **Hot-path placement.** Capture sits on every provider call (`retry.rs send()`). Must reuse
  the non-blocking writer; never `await` disk on the request path.
- **Refactor fragility** ([`EVAL-001`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md)
  §4.7). `provider.*` emission lives in HTTP code; a `retry.rs` refactor can silently break it.
  Mitigation: a control-flow test (Phase 43) that asserts a real status + per-attempt count.

## Grounding docs

[`EVAL-001-compare-to-mitmproxy.md`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md) (§4.1, §5, §6) ·
[`EVAL-0005-build-vs-integrate-replay.md`](../analysis/caching/EVAL-0005-build-vs-integrate-replay.md) (§6) ·
[`SPEC-001-trace-format.md`](../analysis/caching/SPEC-001-trace-format.md) (§3.3 provider events, §10 perf) ·
[`ADR-0003-async-writer-thread.md`](../analysis/caching/ADR-0003-async-writer-thread.md) ·
[`phase-42-overview.md`](phase-42-overview.md)
