# ADR-0005: The wire-level cache/cassette/mock substrate is a separate workspace binary

**Status:** Accepted, 2026-06-01
**Decider:** project lead
**Inputs:** `ADR-0001-trace-as-keystone.md`, `SPEC-001-trace-format.md`,
`EVAL-001-compare-to-mitmproxy.md`, `EVAL-0002-full-caching.md`,
`EVAL-0004-litellm-cache-parity.md`, `src/cache.rs`, `src/serve.rs`, root `CLAUDE.md`

> **Amendment (2026-06-02).** The "working name `eridian-replay`, **member of the existing
> Cargo workspace**" shape decided below was superseded by the cross-repo projection
> [`SPEC-astrophage.md`](../../architecture/integrated-architecture/SPEC-astrophage.md) §2.
> The substrate shipped as its **own repo**
> ([`astrophage`](https://github.com/jikanter/astrophage)) — binary renamed
> `eridian-replay` → **`astrophage`**, with `replay-core` a workspace member **of that
> repo** (SPEC-astrophage §2.1 **decision A**). aichat consumes `replay-core` as a
> **cross-repo git dependency**. The decision *rationale* below is unchanged — only the repo
> boundary moved (workspace member → sibling repo); "A workspace member is real surface" in
> Consequences now reads as "a sibling repo is real surface" (a second CI/release, the cost
> named in SPEC-astrophage §2 and `EVAL-0005` §5).

## Context

The caching sub-track (Phases 37–41, per [`EVAL-0004`](EVAL-0004-litellm-cache-parity.md))
ports LiteLLM's caching subsystem into the Eridian runtime. As that work was scoped, a
second, sharper requirement crystallized: **deterministic, offline, token-free
regression/eval runs** — "eval-replay." promptfoo (Track 1 of `SPEC-002`) and the future
Inspect AI consumer both want to run a committed corpus of model interactions without
spending tokens or touching a provider, repeatably, in CI.

Eval-replay, transparent caching, and scripted provider faults are three *policies* over
**one mechanism**: intercept a canonicalized request at the OpenAI-compatible wire
boundary and answer it from a content-addressed store (or a scripted fault) instead of
the provider. LiteLLM proves the mechanism is small and the three policies are the same
machine with different eviction/keying/fault rules.

The question this ADR settles: **where does that mechanism live?** Three shapes were
considered: keep it entirely in-tree in `aichat`; integrate an off-the-shelf tool
(mitmproxy/`mitmdump`, LiteLLM-proxy, a hosted gateway); or **extract it into a separate
binary in the same Cargo workspace, sharing a small content-addressed-store + SSE-replay
crate with `aichat`.**

`EVAL-001` already evaluated mitmproxy *for the trace path* and concluded "build
in-process; revisit mitmproxy only for deferred Phase-3 cassettes" (§4.8, §6). That
verdict was correct for trace emission but is the exact decision this work supersedes for
the **cassette/replay** leg — see [`EVAL-0005`](EVAL-0005-build-vs-integrate-replay.md),
which re-derives the verdict against the now-committed eval-replay requirement. `TODO(ACE-id)`

## Decision

**Build the wire-level record/replay/cache/mock substrate as a new member of the
existing Cargo workspace** (working name `eridian-replay`), reusing the
`serve.rs` HTTP gateway plumbing (37D) and the `CacheBackend` trait (38A), and sharing a
small content-addressed-store + SSE-synthesis crate (working name `replay-core`) with
`aichat`'s in-tree `StageCache`/serve path. The contract is
[`SPEC-003`](SPEC-003-cache-substrate.md); the ecosystem surfaces are
[`SPEC-004`](SPEC-004-ecosystem-surfaces.md); the build-vs-integrate stress test is
[`EVAL-0005`](EVAL-0005-build-vs-integrate-replay.md); the phased plan is
[`PLAN-cache-substrate.md`](PLAN-cache-substrate.md).

Three sub-decisions are load-bearing:

### 1. Three cache families, only one extractable. `TODO(ACE-id)`

Eridian's caching splits into three families keyed on three different identities. **Only
the wire-level response cache extracts.** The other two stay in `aichat`, full stop:

| Family | Keyed on | Lives | Why |
|---|---|---|---|
| **Structure-aware `StageCache`** (`stages` prefix; `.cache/knowledge`, pipeline-stage memoization) | `(role, model, input)` — runtime-internal identity | **Eridian** | The substrate cannot see role/stage identity by construction. |
| **Provider prompt caching (L3)** (37B `cache_control` emission, prefix-stability discipline) | the *request builder's* byte ordering | **Eridian** | It is a property of how aichat orders bytes before the wire, not a store the substrate owns. |
| **Wire-level response cache** (37C transparent path, 37D server cache, 38B/C/D/E backends + control protocol, 39/40/41) | the **canonicalized request body** — runtime-agnostic, HTTP-computable | **Substrate** | Byte-level, provider-facing, identical for any client. |

### 2. Build the part that carries our trace; stay free to borrow the commodity part. `TODO(ACE-id)`

The substrate is **small semantic glue, not a proxy from scratch and not a forked
mitmproxy**: canonicalization + determinism gate + CAS store + SSE synthesis + control
protocol + trace projection (~1.5k lines of owned code, per `EVAL-0005`). It is a
**reverse gateway** pointed at by `base_url` — **no TLS interception, no CA trust** (the
one thing a forward MITM adds is irrelevant when we own the client and the seam is an
OpenAI-compatible URL).

### 3. The OpenAI-compat seam keeps build and integrate non-exclusive. `TODO(ACE-id)`

Because the boundary is an OpenAI-compatible `base_url`, a client environment that already
runs a commodity caching gateway and only needs plain cache can point Eridian at it with a
config change. We build the part that carries our keystone trace and eval reproducibility;
we stay free to borrow the commodity cache. Neither choice is load-bearing on the other.

## Consequences

### Positive

- **One mechanism, three policies.** Cache, cassette, and mock are eviction/keying/fault
  variants of the same CAS+SSE machine; no tool in the field unifies all three
  (`EVAL-0005` §3).
- **Eval-replay is first-class.** A committed cassette set replayed in-process gives
  promptfoo/Inspect deterministic, token-free runs — the decisive commitment behind the
  build.
- **Accounting stays a keystone-trace projection.** The substrate's hit/miss/$-saved and
  `cache_hit` flag are emitted as / derivable from `SPEC-001` events (37A/37E), correlated
  to the originating turn via an `X-Eridian-Session-Id` header we control (§3.6/§3.7 of the
  kickoff; legitimate here because we own the client — the exact objection `EVAL-001`
  raised against a *third-party* MITM does not apply).
- **No dependency-budget regression.** Default `cargo build` adds zero new default deps;
  anything non-default is cargo-gated, consistent with `EVAL-0004` delta #2 and the root
  `CLAUDE.md` *Ask First* on dependencies.
- **Deployment is a config choice, not a fork.** "In-process vs separate process" is one
  `CacheBackend` (38A) seam plus a `base_url`; the same code runs both ways.

### Negative

- **A workspace member is real surface.** A second binary, its own `clap` CLI, its own
  Showboat demos, its own tests. Mitigated by the shared `replay-core` crate so the CAS
  and SSE logic exist once.
- **Two stores must not bleed.** The substrate and `StageCache` share a *storage
  mechanism* (`replay-core`) but **must not** share keying or semantics (§3.3). This is a
  discipline enforced by review and by the `SPEC-003` non-goals; getting it wrong is a
  migration to undo, not a refactor.
- **Correlation header is a small instrumentation in `aichat`.** Eridian injects
  `X-Eridian-Session-Id` + determinism signals on outbound requests. Cheap, and unlike the
  mitmproxy case it touches only the client we already own.

### Risks accepted

- **The substrate is justified *only because* eval-replay is committed.** `TODO(ACE-id)`
  If eval-replay were not a commitment, the honest call would be integrate-or-skip:
  transparent cache alone is well-served by a commodity gateway behind `base_url`, and the
  in-tree `StageCache`/serve cache already covers the single-user case. We name this
  explicitly: **the build is contingent on the eval-replay commitment, not on caching
  alone.** `EVAL-0005` §5 carries the counter-case in full.
- **Boundary drift.** A future feature could tempt someone to teach the substrate a
  structure-aware key "just this once." That re-imports runtime-awareness into the one
  component whose entire value is being runtime-agnostic — a category error. The
  `SPEC-003`/`SPEC-004` boundary restatements exist to make this visible in review.

## Considered alternatives

### Alt 1: Stay in-tree (no separate binary)

Rejected for the eval-replay use case, accepted for the other two families. The wire cache
*can* live in-tree (37C/37D already put it there), but the cassette leg wants to be
pointed at by **foreign processes** (promptfoo's provider, Claude Code, Inspect) over an
OpenAI-compatible URL without launching a full `aichat` turn pipeline. A separate, minimal
binary that *is just the gateway* is the cleaner leave-behind and the cleaner CI target.
The in-tree caches (`StageCache`, L3) explicitly **stay** — see Decision §1.

### Alt 2: mitmproxy / `mitmdump`

Rejected. `EVAL-001` §4.3/§4.5/§4.6 already rejected it for the trace path on TLS/CA-trust
friction, default-on infeasibility, live-key-at-rest security posture, and a Python runtime
on the critical path (an *Ask First* item). For the cassette leg specifically, `mitmdump`
record/replay *is* in mitmproxy's wheelhouse (`EVAL-001` §4.8) — but we are a **reverse
gateway pointed at by `base_url`**, so mitmproxy's sole value-add (forward TLS
interception) is irrelevant, and its CA-trust cost is pure liability in a client
leave-behind. `EVAL-0005` supersedes `EVAL-001` §4.8/§6 on this point by name.

### Alt 3: LiteLLM proxy as a dependency

Rejected. `EVAL-0004` already rejected adopting LiteLLM as a *dependency* (port
feature-for-feature instead). The verdict still holds when the consumer is a *separate
binary*: LiteLLM-proxy is a Python service that (a) puts Python on the critical path of a
default-capable component, (b) cannot emit our `SPEC-001` keystone trace from its foreign
event model (§3.6 anti-fragmentation), and (c) does not give us the cassette/mock legs as
one mechanism. We borrow LiteLLM's *design* (the `CacheBackend`/control-protocol shape via
38A/38D), not its runtime.

### Alt 4: Hosted gateway (Helicone / Portkey / Cloudflare AI Gateway)

Rejected on **leave-behind and data-egress** grounds. A hosted gateway routes every
provider call through a third party, ships prompts/responses off-box (contradicting the
local-first principle `ECOSYSTEM.md` states for rejecting Langfuse/LangSmith/Braintrust),
and cannot be the keystone-trace projection. Eval-replay specifically needs an *offline,
committed* corpus — a network dependency defeats the point.

### Alt 5: Bifrost (Go gateway, Tier-2 peer)

Rejected on **second-runtime and strategic** grounds. Bifrost is a capable Go LLM gateway,
but adopting it adds a Go runtime to the critical path (an *Ask First* "new language"),
makes us a downstream consumer of a peer project's roadmap for our most strategically
load-bearing artifact (eval-replay), and still cannot emit our keystone trace natively. The
~1.5k lines of owned glue (`EVAL-0005` §4) buys independence on the one component we least
want to outsource.

## Sources and prior art

- [`EVAL-0005-build-vs-integrate-replay.md`](EVAL-0005-build-vs-integrate-replay.md) — the build-vs-integrate stress test this ADR ratifies; supersedes `EVAL-001` §4.8/§6.
- [`EVAL-001-compare-to-mitmproxy.md`](EVAL-001-compare-to-mitmproxy.md) §4.3–4.9, §6 — the mitmproxy verdict for the trace path, partially superseded here.
- [`EVAL-0004-litellm-cache-parity.md`](EVAL-0004-litellm-cache-parity.md) — the LiteLLM parity map and Phase 37–41 vocabulary; the design borrowed, the dependency rejected.
- [`EVAL-0002-full-caching.md`](EVAL-0002-full-caching.md) — the L1/L2/L3/L4 layered model.
- [`ADR-0001-trace-as-keystone.md`](ADR-0001-trace-as-keystone.md) — the keystone-trace invariant the accounting projects onto.
- `src/cache.rs` (`StageCache`, `CacheManager` prefixes `stages`/`transparent`/`server`/`semantic`), `src/serve.rs` (37D gateway), root `CLAUDE.md` *Ask First* constraints.
