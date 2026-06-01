# EVAL-0005: Build vs. Integrate the Record/Replay/Cache/Mock Substrate

**Status:** Analysis, 2026-06-01
**Inputs:** [`EVAL-001-compare-to-mitmproxy.md`](EVAL-001-compare-to-mitmproxy.md) (esp. §4.8, §6),
[`EVAL-0002-full-caching.md`](EVAL-0002-full-caching.md), [`EVAL-0004-litellm-cache-parity.md`](EVAL-0004-litellm-cache-parity.md),
[`ADR-0001-trace-as-keystone.md`](ADR-0001-trace-as-keystone.md), [`SPEC-001`](SPEC-001-trace-format.md),
[`SPEC-002`](SPEC-002-test-harness.md), [`SPEC-003`](SPEC-003-cache-substrate.md),
`src/serve.rs` (37D), `src/cache.rs`, root `CLAUDE.md`
**Question:** Now that **eval-replay** is committed — deterministic, offline, token-free
regression/eval runs — should Eridian *build* the wire-level record/replay/cache/mock
substrate (a separate workspace binary, `ADR-0005`), or *integrate* an off-the-shelf tool
(mitmproxy/`mitmdump`, LiteLLM-proxy, a hosted gateway, Bifrost)?

This is a critical evaluation, not a defense of `ADR-0005`. It carries the verdict **build**
— but it **explicitly supersedes [`EVAL-001`](EVAL-001-compare-to-mitmproxy.md) §4.8 and §6
for the cassette/replay decision**, and states the honest counter-case under which the
verdict would flip.

---

## 1. What `EVAL-001` decided, and what this supersedes `TODO(ACE-id)`

`EVAL-001` evaluated mitmproxy **for the trace-emission path** and concluded (§6): *"Build
the open harness. Do not route default tracing through mitmproxy. Revisit mitmproxy for one
deferred, scoped use: Phase 3 VCR cassettes (§4.8). That decision is independent of the
Phase 1 trace architecture and should be made on its own merits when Phase 3 is planned."*

That deferral is exactly the decision this evaluation now makes. **§4.8 ("Replay /
cassettes — mitmproxy wins, but for deferred work") and the §6 "revisit mitmproxy for
cassettes" disposition are superseded by this document.** The reason §4.8 leaned mitmproxy
was that, *for the trace path*, cassettes were deferred and unscoped, and `mitmdump`
record/replay is genuinely mitmproxy's wheelhouse. Two things changed:

1. **Eval-replay is now committed and load-bearing**, not deferred. It is the decisive
   reason the substrate exists at all (`ADR-0005`).
2. **The substrate is a reverse gateway pointed at by `base_url`**, not a forward MITM. The
   single capability that made mitmproxy attractive for cassettes — transport-boundary
   capture via TLS interception — is **free** when we own the client and the seam is an
   OpenAI-compatible URL. mitmproxy's value-add evaporates; only its costs (CA trust, a
   Python runtime, live keys at rest) remain.

So §4.8's "mitmproxy wins for cassettes" was correct *given a forward-MITM framing of
cassettes*. Reframed as a reverse gateway we own, it inverts.

---

## 2. The field fragments across the three policies `TODO(ACE-id)`

The substrate serves **three policies off one mechanism**: cache (TTL/LRU), cassette
(pinned, deterministic), mock (scripted faults). The integrate option must cover all three.
No single off-the-shelf tool does:

| Tool | cache | cassette (record/replay) | mock (faults) | keystone-trace projection | leave-behind cost |
|---|---|---|---|---|---|
| **mitmproxy / `mitmdump`** | weak (not its job) | ✅ strong (flows = cassettes) | ◑ via addons (Python) | ✗ foreign event model | CA trust + Python runtime |
| **LiteLLM proxy** | ✅ strong | ✗ no cassette pinning | ✗ | ✗ foreign event model | Python service on critical path |
| **Hosted gateway** (Helicone/Portkey/CF) | ✅ strong | ◑ logs, not committed offline sets | ◑ partial | ✗ off-box, can't be local projection | data egress + network dep |
| **Bifrost** (Go gateway) | ✅ strong | ◑ partial | ◑ partial | ✗ foreign event model | Go runtime + peer-roadmap dependence |
| **`wiremock-rs`** | ✗ | ◑ in-process only | ✅ strong (in-process) | ✗ not a gateway | (kept — different niche) |
| **substrate (build)** | ✅ | ✅ | ✅ | ✅ **is a projection** | one workspace binary, zero new default deps |

Two structural facts fall out:

- **No tool unifies cache + cassette + mock.** Integrating means stitching *several*
  foreign tools (e.g., LiteLLM for cache + mitmdump for cassettes + wiremock for mock),
  each with its own data model, lifecycle, and language — the multi-system,
  multi-language cost `EVAL-001` §3 already counted against Option B, now multiplied across
  three policies.
- **None can be a keystone-trace projection from a foreign process.** `ADR-0001` and
  `SPEC-001` make the trace the single source of truth; the F1 anti-fragmentation rule
  (`SPEC-003` §6) requires every telemetry surface to be a *view* over it. A foreign
  process emitting its own event model that *cannot* be a `SPEC-001` projection is the
  exact failure being avoided. A substrate we own emits (or feeds) `SPEC-001` events
  directly.

---

## 3. mitmproxy's cassette strength is irrelevant to a reverse gateway `TODO(ACE-id)`

`EVAL-001` §4.8 credited mitmproxy with cassettes because `mitmdump` records and replays
flows. But re-run `EVAL-001`'s own objections against the *reverse-gateway* framing:

- **§4.3 TLS / CA interception** — mitmproxy's cassette capture *requires* intercepting
  TLS, which requires its CA in the trust store. The substrate captures at an
  OpenAI-compatible `base_url` it is *pointed at* — **no TLS interception, no CA**. The one
  thing mitmproxy uniquely does is the thing we explicitly do not need (`SPEC-003` §8
  non-goal).
- **§4.5 redaction / live keys at rest** — a MITM holds plaintext `Authorization` to
  authenticate upstream and persists flows with live keys until scrubbed. A committed
  *cassette* with a leaked key is a worse incident than a transient cache. The substrate
  redacts inline before disk (`SPEC-003` §6/§7), as a record-mode gate.
- **§4.6 dependency / language** — `mitmdump` puts a **Python runtime** on the path of a
  default-capable component (an *Ask First* item, root `CLAUDE.md`). The substrate is Rust
  in the same workspace, zero new default deps (`EVAL-0004` delta #2).
- **§3 correlation header** — `EVAL-001` counted `X-Eridian-Session-Id` injection *against*
  mitmproxy because it meant instrumenting the very code the proxy was meant to avoid
  touching. For the substrate that objection **inverts**: we already own and instrument the
  client, so injecting the header (`SPEC-003` §6) is cheap, correct, and is *how* the
  cassette correlates to a turn.

Every cost `EVAL-001` charged to mitmproxy survives the reframing; the lone benefit does
not. That is the substance of the supersession.

---

## 4. The build is small, owned, semantic glue `TODO(ACE-id)`

`ADR-0005` §3.8 and `SPEC-003` size the build as **~1.5k lines**, and most of it is reused,
not new:

- **Reused:** 37D's `serve.rs` HTTP gateway plumbing; 38A's `CacheBackend` trait; 37C's
  `transparent_key` canonicalization; `src/cache.rs`'s atomic-write + content-addressing
  (refactored into `replay-core`).
- **New (the glue):** the canonical-key extension, the determinism gate, the CAS wiring,
  SSE synthesis, the control protocol (38D vocabulary + mode selector), the policy selector
  (cache/cassette/mock), and trace projection.

This is **not** "build a proxy from scratch" (we are not implementing TLS, connection
pooling, or HTTP/2 — `reqwest`/`axum`/`hyper` already do that) and **not** "fork mitmproxy"
(no interception layer). It is semantic glue over plumbing we already ship. The
build/integrate math is therefore not "1.5k lines vs. free" — it is "1.5k lines of owned,
trace-native, three-policy code vs. 2–4 foreign systems in 2 languages that still don't
project onto the keystone trace and still don't give us deterministic committed cassettes."

---

## 5. The honest counter-case `TODO(ACE-id)`

The case *against* building, stated at full strength:

- **Solo-maintainer scope.** A second binary, its own CLI, demos, and tests is real
  ongoing surface for one maintainer. `ADR-0001` §residual-risk already conceded the
  multi-consumer framing is "partially speculative"; a second binary compounds that bet.
- **If eval-replay were *not* committed, integrate-or-skip wins.** Transparent cache alone
  is a commodity: a client environment that already runs LiteLLM/Helicone behind `base_url`
  gets L1/L2 caching with a config change (`ADR-0005` §3, the OpenAI-compat seam), and the
  in-tree 37C/37D `StageCache`/serve cache already covers the single-user case. For *cache
  alone*, building a separate binary is over-engineering — exactly `EVAL-001`'s residual
  critique of Option A.

**Why it does not change the verdict:** eval-replay *is* committed, and it is the one
requirement the field cannot satisfy. Deterministic, offline, token-free, **committed**
cassettes that are **a projection of the keystone trace** and that compose with Eridian-side
tool-replay (`SPEC-004` §llm-functions) are not a feature of any single off-the-shelf tool.
The substrate is justified **because of, and only because of, that commitment** — which is
why `ADR-0005` records it as an accepted, named risk rather than burying it. If the
commitment were withdrawn, this verdict should be revisited, and integrate/skip would
likely win for the residual cache-only need.

---

## 6. Verdict `TODO(ACE-id)`

**Build the substrate as a separate workspace binary (`ADR-0005`). Do not integrate
mitmproxy/`mitmdump`, LiteLLM-proxy, a hosted gateway, or Bifrost for the
cassette/replay leg.**

Rationale, ranked:

1. **No single tool unifies cache + cassette + mock**, and none can be a keystone-trace
   projection from a foreign process (§2). Integrating fragments the one artifact `ADR-0001`
   exists to keep singular.
2. **mitmproxy's sole cassette advantage — transport-boundary capture via TLS interception
   — is irrelevant to a reverse gateway we point at by `base_url`** (§3). Its costs (CA
   trust, Python, live keys at rest) remain. This supersedes `EVAL-001` §4.8/§6.
3. **The build is ~1.5k lines of owned semantic glue over plumbing we already ship** (37D,
   38A, 37C) — not a proxy from scratch, not a fork (§4).
4. **The OpenAI-compat seam de-risks the call**: if a deployment only needs commodity
   cache, it points `base_url` at a commodity gateway; we lose nothing by owning the
   trace-native, eval-replay-capable part (§5, `ADR-0005` §3).

**Carried forward unchanged from `EVAL-001`:** the in-process trace emitter remains the
source of truth; `wiremock-rs` remains the in-process, time-mockable control-flow mock
(`SPEC-004` §wiremock); the substrate is purely wire-level and does not own structure-aware
or knowledge keys (`SPEC-003` §0).

---

## Sources

- [`EVAL-001-compare-to-mitmproxy.md`](EVAL-001-compare-to-mitmproxy.md) — §4.8 and §6 superseded here for the cassette/replay decision; §4.3/§4.5/§4.6 reused.
- [`EVAL-0004-litellm-cache-parity.md`](EVAL-0004-litellm-cache-parity.md) — LiteLLM design borrowed (38A/38D), dependency rejected; the zero-new-default-deps posture.
- [`EVAL-0002-full-caching.md`](EVAL-0002-full-caching.md) — the L1/L2/L3/L4 layered model and the cache-event/trace-contamination hazards.
- [`ADR-0001-trace-as-keystone.md`](ADR-0001-trace-as-keystone.md) / [`SPEC-001`](SPEC-001-trace-format.md) — the keystone-trace invariant and the F1 anti-fragmentation rule.
- [`ADR-0005`](ADR-0005-cache-substrate-extraction.md) / [`SPEC-003`](SPEC-003-cache-substrate.md) — the decision this evaluation ratifies and its contract.
- [mitmproxy](https://mitmproxy.org/) / `mitmdump`; [LiteLLM proxy](https://docs.litellm.ai/docs/proxy/caching); [Helicone](https://helicone.ai), [Portkey](https://portkey.ai), [Cloudflare AI Gateway](https://developers.cloudflare.com/ai-gateway/); [Bifrost](https://github.com/maximhq/bifrost).
