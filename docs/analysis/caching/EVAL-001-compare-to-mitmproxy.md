# EVAL-001: Open-Harness Trace Path vs. mitmproxy + Custom Tracer

**Status:** Analysis, 2026-05-18
**Inputs:** `SPEC-001-trace-format.md`, `SPEC-002-test-harness.md`, ADR-0001..0004,
`ECOSYSTEM.md`, `PLAN-trace-emission.md`
**Question:** Should Eridian build the native, in-process trace emitter described
in SPEC-001/002, or get equivalent observability by routing aichat's provider
traffic through [mitmproxy](https://mitmproxy.org/) and bolting on a small custom
tracer for whatever the proxy misses?

This is a critical evaluation, not a defense of the existing plan. It concludes
the open-harness path is correct **as the source of truth**, but that the
mitmproxy proposal contains one genuine insight the current plan under-weights —
capturing the provider interaction at the transport boundary rather than
reconstructing it pre-send — and that insight is better realized in-process.

---

## 1. The two options, stated precisely

**Option A — open harness (SPEC-001/002).** aichat is instrumented internally.
A `eridian-trace` crate defines the schema; an OS-thread writer emits 13 typed
event classes to JSONL + a content-addressed blob store. ~12 PRs, all Rust,
mostly inside aichat. Tracing is on by default.

**Option B — mitmproxy + custom tracer.** Point aichat's provider HTTP through
mitmproxy (env proxy vars + a trusted MITM CA). A mitmproxy addon serializes
flows to JSONL. A separate, smaller "custom tracer" inside aichat covers what
the proxy structurally cannot see. Two event streams, merged downstream.

The framing of Option B already concedes the core problem: it needs a custom
tracer *in addition to* the proxy. The real question is therefore not "proxy vs.
instrumentation" but **"how much does the proxy actually let the custom tracer
shrink, and is that saving worth running a second system?"**

---

## 2. What mitmproxy can and cannot see

SPEC-001 defines 13 event types. Split them by whether they cross the
provider network boundary:

| Event type | Crosses the wire? | mitmproxy can produce it? |
|---|---|---|
| `provider.request` | yes | **yes — better** (ground-truth bytes) |
| `provider.response` | yes | **yes — better** (ground-truth bytes, real SSE timing) |
| `provider.retry` | partially | **no** — sees attempt N and N+1, not aichat's `trigger` classification, `backoff_ms`, or `will_fallback` |
| `provider.fallback` | partially | **no** — sees traffic shift A→B, not the `reason` or that aichat *decided* to fall back |
| `output.chunk` | yes | yes (raw SSE frames) |
| `session.start` / `session.end` | **no** | **no** — args, cwd, `config_hash`, exit status, cost |
| `context.system_prompt_built` | indirectly | partial — prompt is inside the request body, but the *assembly event* is not on the wire |
| `context.role_applied` | **no** | **no** — role name, tool whitelist are local decisions |
| `context.rag_retrieved` | **no** | **no** — retrieval, RRF scoring, chunk IDs, thresholds are entirely local |
| `tool.requested` | partially | partial — the model's *ask* is in the response body; needs parsing |
| `tool.denied` | **no** | **no** — a whitelist denial produces *zero network traffic* |
| `tool.executed` | **no** | **no** — tools are local subprocesses (llm-functions bash/py/js); no HTTP |
| `error` | **no** | **no** — config errors, rescued panics, exhausted retries |
| `trace.heartbeat` / `trace.dropped` | n/a | n/a — emitter meta |

The proxy nails exactly **two** event classes outright (`provider.request`,
`provider.response`), helps with two more (`output.chunk`, `tool.requested`),
and is **blind to roughly half the taxonomy** — every event SPEC-002's
control-flow tests are specifically designed to assert on.

This is not incidental. ADR-0001's rejected Alt-4 ("no structured trace; scrape
stdout") was rejected because *"the retry layer emits no observable signal."*
mitmproxy is a strictly better Alt-4 for the network layer — but it is still
**observation from outside the process**, and the things Eridian most needs to
test (retry *classification*, fallback *reason*, tool *denial*, RAG *scoring*)
are semantic judgments that exist only inside aichat. A proxy can see "the
stream ended early"; it cannot see "aichat classified this as
`stream_interrupted`, counted it as attempt 2 of 3, and scheduled a 1000ms
backoff." That sentence *is* the test target.

---

## 3. mitmproxy displaces only one PR

Map Option B onto `PLAN-trace-emission.md`. The custom tracer in Option B still
has to deliver:

- PR-1 `eridian-trace` crate (schema + parser) — **still needed.** A flow file
  is not a test-assertion substrate; SPEC-002 Track 2 parses typed events with
  this crate. The schema (SPEC-001) is the contract regardless of who emits it.
- PR-2 CLI/config — **still needed.**
- PR-3 writer thread — **still needed** for the non-network events.
- PR-4 blob store — **still needed** (tool stdout, prompts, RAG context).
- PR-5 redaction — **still needed**, and *larger* (see §4.5).
- PR-6 session lifecycle — **still needed.**
- PR-7 provider events — **this is the only PR mitmproxy displaces.**
- PR-8 context, PR-9 tool, PR-10 output, PR-11 `trace show`, PR-12 acceptance —
  **all still needed.**

mitmproxy removes one PR — the one PR the plan itself flags as large
(~600 LOC) and refactor-fragile — and **adds** new work that is not in the plan
at all:

- A mitmproxy addon (Python) that serializes flows to a SPEC-001-shaped schema.
- Process lifecycle: who launches/tears down the proxy, per-invocation or
  daemon, port allocation, readiness wait.
- A stream-merge step: two JSONL files, two clocks, two `seq` spaces, joined
  into one causal timeline. SPEC-001 §7's "causal ordering by emission" and §8's
  "single writer generates `seq`" invariants exist precisely so consumers
  *don't* have to do this. Option B reintroduces the merge problem it was
  designed to avoid.
- A correlation mechanism: the proxy sees anonymous HTTP. To tie a flow to a
  turn/role/parent-session, aichat must inject a correlation header
  (`X-Eridian-Session-Id`) into every request — which is itself instrumenting
  aichat. Option B cannot even achieve correlation without touching the code it
  was meant to avoid touching.

Net: Option B is **two systems in two languages** to deliver what Option A does
as one. It does not let you skip the load-bearing work (schema, writer, parser
crate, blob store, redaction, all non-network instrumentation). It only swaps
the *source* of the provider events — for a worse-positioned source.

---

## 4. Point-by-point comparison

### 4.1 Fidelity of the provider interaction — mitmproxy wins

This is the real point in Option B's favor and it should not be dismissed.
SPEC-001's `provider.request` is emitted *before the send*, from aichat's view
of the request. That view can differ from the wire: HTTP-client header
injection, gzip, connection-level retries by `reqwest`, a body mutated between
attempts. `messages_hash` computed pre-send is a hash of aichat's *intent*, not
of what the provider received. For the deferred training-data consumer
(ECOSYSTEM §train), ground-truth wire capture is materially more trustworthy.
mitmproxy captures exactly what crossed the socket, including real inter-chunk
SSE timing. Option A, as specified, does not.

**This is a legitimate gap in SPEC-001 — see §5.**

### 4.2 "Tracing on by default" — mitmproxy loses, structurally

SPEC-001 §1: *"Default behavior is on. Traces are too valuable as accumulating
training data to be opt-in."* A default-on external MITM proxy is not a
realistic posture: it requires a CA cert trusted by the host, an extra process
running for every `aichat` invocation, and proxy env vars set. You cannot ship
that as the zero-config default. Option B forces tracing back to opt-in, which
contradicts a stated, load-bearing requirement.

### 4.3 TLS / CA interception — friction unique to mitmproxy

aichat is `reqwest`-based (confirmed: `src/client/*.rs`, `src/utils/request.rs`,
`src/client/retry.rs`). Intercepting its TLS means installing mitmproxy's CA
into whichever trust store `reqwest` is built against (rustls vs. native-tls) or
threading `SSL_CERT_FILE`. This is per-environment fragility — CI containers,
dev machines, the future `--serve` path — that the in-process writer simply
does not have.

### 4.4 Time-mocking — mitmproxy actively breaks SPEC-002

SPEC-002 §3 requires `tokio::time::pause()` so a test with a 30s backoff runs
instantly. Time-mocking only works in-process. mitmproxy is a real process on
real wall-clock; routing tests through it makes the backoff tests take their
full real duration. wiremock-rs runs in-process and is compatible with tokio
time control. For the test harness specifically, mitmproxy is a **regression**,
not an improvement.

### 4.5 Redaction & security posture — mitmproxy loses

SPEC-001 §6 redacts API keys *before bytes hit disk*. A MITM proxy must hold the
plaintext `Authorization` header — that is how it authenticates upstream — and
its flow store contains live provider keys until a scrubbing pass runs. A
default-on component that decrypts all provider TLS and persists it is a
strictly worse security surface than an in-process writer that redacts inline.

### 4.6 Dependency & ecosystem fit — mitmproxy loses

CLAUDE.md's brief lists "significant increase in number of dependencies" and
"introduction of new programming languages" as **Ask First** items. aichat is
Rust + bash. mitmproxy puts a Python runtime on the *critical path of default-on
tracing*. ECOSYSTEM.md deliberately scopes Python tooling (marimo, Inspect) as
*deferred* and lists Langfuse/LangSmith/Braintrust as rejected for the
local-first principle. An external proxy daemon is closer to that rejected
category than to the local-first one.

### 4.7 Refactor resilience — split decision

Option A's honest weakness: `provider.*` emission lives in aichat's HTTP code;
a refactor of `src/client/retry.rs` can silently break it. The proxy is immune
to that for the network layer — a real point for Option B. But the proxy is
*fully exposed* to refactors of role/RAG/tool internals, which it cannot see at
all, and to any change in how aichat formats requests (it would faithfully
record the new format with no signal that anything regressed). Resilience is a
wash: each approach is brittle to a different half of the system.

### 4.8 Replay / cassettes — mitmproxy wins, but for deferred work

mitmproxy flows are essentially VCR cassettes; `mitmdump` records and replays
them. ECOSYSTEM.md's deferred "VCR cassettes" item (Phase 3) is genuine
mitmproxy territory. This is a real strength — but it governs *deferred* work
and must not drive the Phase 1 architecture.

### 4.9 Performance — minor edge to Option A

SPEC-001 §10 criterion 7 caps request-path p99 regression at 5%. Option A's
`try_send` is non-blocking by construction (ADR-0003). Option B adds a
localhost proxy hop to every provider call — usually small, but unconditional
and now in the request path.

---

## 5. The insight worth salvaging

Strip the rhetoric and the mitmproxy proposal makes one correct observation:

> The provider interaction should be captured as **ground truth at the transport
> boundary**, not reconstructed from aichat's pre-send intent.

SPEC-001's `provider.request`/`provider.response` are, as written, vulnerable to
intent-vs-wire drift (§4.1). That is a real defect.

But the fix is **not** an external MITM proxy. The same ground truth is
available *in-process* by capturing at the `reqwest` boundary:

- A `reqwest_middleware` / `tower` layer, or a thin wrapper at the small number
  of client construction points (`src/utils/request.rs`, the per-provider
  builders in `src/client/`), observes the actual serialized request body and
  the actual response stream — post-serialization, post-header-injection,
  per-attempt — and feeds the existing `TraceSender`.

This captures exactly what mitmproxy would, and:

- stays on by default (no CA, no second process, no env vars);
- keeps redaction inline, before disk;
- keeps one writer, one clock, one `seq` space — no merge step;
- needs no correlation header — it already has the session context;
- is compatible with `tokio::time::pause()` for SPEC-002.

So the recommendation is **adopt the insight, reject the mechanism.** Add an
HTTP-layer interceptor to PR-7's scope and have `provider.request` carry a hash
of the *actual wire body*, not the pre-send body. This closes the only real gap
mitmproxy exposed, in-process.

---

## 6. Verdict

**Build the open harness (Option A). Do not route default tracing through
mitmproxy.**

Rationale, ranked:

1. mitmproxy is blind to ~half of SPEC-001 — every event SPEC-002's
   control-flow tests assert on (`tool.denied`, `provider.retry` classification,
   `provider.fallback` reason, `context.rag_retrieved`). Those are the *point*
   of the harness and they never touch the wire.
2. Option B still requires the custom tracer to build the schema, writer, blob
   store, redaction, parser crate, and all non-network instrumentation. It
   displaces exactly one PR (PR-7) and adds proxy lifecycle, a Python addon,
   stream-merge, and a correlation header in return. That is a net increase in
   moving parts, in two languages.
3. mitmproxy breaks two stated requirements outright: "tracing on by default"
   (SPEC-001 §1) and `tokio::time::pause()` time-mocking (SPEC-002 §3).
4. It worsens the security posture (default-on TLS decryption with live keys
   at rest) and the dependency posture (Python on the critical path — an
   "Ask First" item per CLAUDE.md).

**Adopt from Option B:** the ground-truth-at-the-transport-boundary insight.
Implement it in-process via a `reqwest`-layer interceptor (§5), folded into
PR-7. This is the one place SPEC-001 is genuinely weaker than the proxy, and it
is cheaply fixed without any of the proxy's costs.

**Revisit mitmproxy for one deferred, scoped use:** Phase 3 VCR cassettes
(ECOSYSTEM.md). `mitmdump` record/replay is a natural fit there. That decision
is independent of the Phase 1 trace architecture and should be made on its own
merits when Phase 3 is planned.

---

## 7. Residual risk in Option A (not introduced by this eval, but worth stating)

ADR-0001 itself concedes the multi-consumer framing is "partially speculative":
if only the test harness ever materializes, the blob store, heartbeat, and
redaction machinery are somewhat over-built. That critique is real — but it
applies **equally** to Option B's custom tracer, which needs the same machinery
for the same non-network events. It is therefore not a differentiator between
the two options, and does not change the verdict. The honest mitigation is the
one ADR-0001 already names: ship v0.1 fast, get the test harness reading it
within the quarter, and let real consumers drive v0.2.
