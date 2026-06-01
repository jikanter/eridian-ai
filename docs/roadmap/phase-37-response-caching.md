# Phase 37: Transparent Response Caching — Deep Design

Detail companion to [`phase-37-overview.md`](phase-37-overview.md). The overview lists what 37A–F do; this doc explains *why* the 4-layer model is the right framing (the spec's 3-pattern table missed the cheapest one), *how* the pi integration falls out of the existing server topology, and *what* the trace + flag posture demand of the writer.

## Why four layers, not three

The spec the user supplied (exact / semantic / proxy) is the standard 3-pattern teaching framing — it covers the patterns a developer *chooses between* on day one. It is incomplete for a production cost-conscious CLI in 2026 because it omits the layer the providers themselves built and price for: **L3 provider prompt caching**.

| # | Layer | What it caches | What it removes | Spec name |
|---|---|---|---|---|
| L1 | Exact response cache | Full final response for an identical request | The entire turn — zero provider calls | "Exact match" |
| L2 | Semantic response cache | Response keyed by *meaning* of the request | The entire turn for near-duplicate requests | "Semantic" |
| L3 | Provider prompt cache | The provider's KV-cache of the prompt prefix | ~85% of prefill latency, 75–90% of input-token cost — but the turn still happens | *(omitted from spec)* |
| L4 | Conversation / session reuse | Already-computed context inside a multi-turn loop | Re-sending and re-billing stable context every turn | *(omitted from spec)* |

The spec's "proxy gateway" is an *implementation strategy* for L1/L2, not a fifth layer — it answers "where does the cache live" (in the gateway), not "what does it cache." Phase 37D adopts the proxy-gateway implementation for L1 specifically because the pi REPL's substrate is already a proxy gateway (`aichat --serve`). The spec is well-suited to the language-agnostic "if I were building this from scratch" question; the project's reality is that the proxy is already there.

L1 + L2 *remove whole turns*. L3 + L4 *shrink the hot path of a turn that still happens*. The brief said **both**. Shipping only the spec's 3 patterns would leave the largest single cost reduction on the floor — Anthropic's 90% read discount on the stable prefix, which is opt-in and costs nothing at runtime once `cache_control` is emitted.

## Why L3 is the largest single payoff and why aichat ships zero of it

[`src/client/claude.rs:180`](../../src/client/claude.rs) (`claude_build_chat_completions_body`) assembles `system`, `messages`, and `tools` and emits them raw. The string `cache_control` does not appear in `src/`. The Claude streaming and non-streaming extractors read `usage.input_tokens` / `usage.output_tokens` only and silently drop `cache_creation_input_tokens` and `cache_read_input_tokens`. `compute_cost` ([`src/client/common.rs:362`](../../src/client/common.rs)) is therefore a flat `input·input_price + output·output_price` — it cannot represent a cache-read token at 0.1× input or a cache-write token at 1.25× input, even when the provider would price it that way.

On the project's own stated priority (local + frontier parity, token cost), leaving Anthropic caching on the table is the single most expensive omission. Worst-case math: a 10-turn pipeline reusing a 4000-token role body (`tools` + `system`) on Claude Sonnet 4.6 ($3 / Mtok input, $15 / Mtok output). Without L3: 4000 × 10 × $3/1M = $0.12 just for the prefix. With L3 (one 1.25× write, nine 0.1× reads): 4000 × $3/1M × (1.25 + 9 × 0.1) = $0.026. The savings are 78% of the prefix bill — and the prefix dominates input tokens once roles include MCP tool schemas and knowledge context.

OpenAI and Gemini 2.5 apply prefix caching *automatically* server-side. aichat gets some L3 benefit on those providers **by luck of message ordering, not by design** — the moment a per-turn timestamp or a freshly-retrieved RAG block lands at the start of the prompt, the implicit cache breaks and the savings vanish. 37B2's prefix-stability audit is therefore a prerequisite even for the providers that don't require a flag — it locks in benefits aichat is *already entitled to* and currently squanders.

## Why 37D *is* the pi integration

The pi REPL is not a separate caller layered on top of aichat — it is a TUI that delegates inference to an in-process `aichat --serve`. Concretely, [`src/repl/pi.rs`](../../src/repl/pi.rs) mints a localhost bridge port, starts the OpenAI-compatible server in-process, stages [`assets/pi-extensions/aichat-bridge.js`](../../assets/pi-extensions/aichat-bridge.js) into `<cwd>/.pi/extensions/`, and execs `pi` with `AICHAT_BRIDGE_URL` + `AICHAT_BRIDGE_TOKEN` in the env. Every pi turn — every `/role`, `/agent`, `/macro`, every freeform chat message — is an HTTP call into `chat_completions` at [`src/serve.rs:978`](../../src/serve.rs).

That topology means:

1. **A cache layer in `serve.rs` is automatically a cache layer for pi.** No pi-side change is needed for the cache to fire — the HTTP call goes through the cache check just like it would for any other caller.
2. **The pi extension surface (`pi-extensions/src/index.ts`) is the visibility plane, not the cache itself.** Slash commands wrap `/v1/cache/*` HTTP routes; the cache is server-side and transparent.
3. **Any downstream tool pointed at the same server inherits the cache for free.** Claude Code, Cursor, a custom script — they all benefit from a cache hit on the pi user's previous identical prompt.

This is why 37D ships the pi-bridge endpoints inside the server work, not as a separate phase. Splitting them would imply the cache and its visibility are independently meaningful — they are not. A cache the pi user cannot inspect is a regression hazard waiting to happen (the `cache.lookup` trace event in 37E is the *audit* answer; the slash commands in 37D are the *interactive* answer).

## Why `--transparent-cache` and not broadening `--no-cache`

Per the user's explicit guidance: `--no-cache` keeps its existing scope (pipeline stage caching only, gated `requires = "pipe"` at [`src/cli.rs:197`](../../src/cli.rs)). The new flag is `--transparent-cache`, opting in to L1 caching on the ordinary path.

Three reasons this is the right call:

1. **Semantic stability.** `--no-cache` is currently *off by default* for pipeline stages — i.e., pipeline caching is on, and the flag disables it. If 37C broadened the flag to also gate L1 on the ordinary path, the flag would mean two things ("disable pipeline cache AND disable transparent cache") and would interact awkwardly with role-level opt-outs.
2. **Default-flip pathway.** 37C ships with `transparent_cache: false` as the config default. After 37E lands and the trace confirms the hit rate, the default flips to `true`. The flag is `--transparent-cache` (opt-in) on day one; it becomes `--no-transparent-cache` (opt-out) in shape *only by config flip*, not by CLI rename. `--no-cache` is reserved for the pipeline meaning users already know.
3. **Composability with role frontmatter.** A role with `cache: false` in its frontmatter opts out per-role regardless of the CLI flag. The role-level opt-out is the *correctness* answer (a non-deterministic creative-writing role should never cache); the CLI flag is the *ergonomics* answer (a user wants to bust the cache for a one-off run). Keeping the flag scoped to L1-on-ordinary-path makes both layers cleanly independent.

The Unix posture is also clearer this way: `--transparent-cache` is opt-in, the cache is *not* on by default until 37E proves it safe, and the flag name describes what it does. "No-cache" is a negation; "transparent-cache" is a name.

## The training-data contamination hazard

Per [`docs/analysis/caching/CLAUDE.md`](../analysis/caching/CLAUDE.md), the open-harness trace format is the keystone for testing, evaluation, and *training data extraction*. A replayed cached response is not a fresh model output — if the deferred training pipeline mines a `chat.response` event without knowing it was a cache replay, the same canonical reply enters the training set N times (one for the original turn, N-1 for the replays), distorting the distribution.

This is the failure mode that makes 37E ("trace `cache.lookup` event + `cache_hit: true` on `chat.response`") *blocking* for 37C/D, not optional. Two specific guarantees:

1. **Every cache hit emits a `cache.lookup` event** with `outcome: hit`, `layer: L1|L2|L3`, and `key_hash` — so the training pipeline can correlate the response back to its origin.
2. **The corresponding `chat.response` event carries `cache_hit: true`** — so a training extractor that only reads `chat.response` events (the simpler default path) cannot accidentally mine a replay.

The `cache.lookup` event is also how a regression that silently disables caching is *catchable*. Without the trace event, a refactor that breaks the cache lookup logic produces a passing test suite, no user-visible error, and a 10× cost bill at the end of the month. ADR-0001's "trace as keystone" framing exists exactly because this class of regression is invisible to unit tests and visible only at the wire.

## The streaming replay-from-buffer tradeoff

The OpenAI-compatible server in [`src/serve.rs`](../../src/serve.rs) supports both streaming (`stream: true`) and non-streaming requests. The pi REPL bridge defaults to streaming because pi is a TUI and partial-token rendering is part of the UX. A naive cache that skips streaming responses (the "carve-out" option in the overview's 37D) loses the cache for the dominant pi use case.

The "replay-from-buffer" alternative buffers the streamed response server-side into a single body, caches the buffered body, and on hit replays the body as a synthesized SSE stream with realistic chunking. Implications:

- **Latency profile.** A cache hit on a streaming request emits the entire body in one synthesized burst (or with simulated chunk delays). The user sees the response complete in <10ms — a feature, not a bug. The "feel" of streaming on a hit is *gone*, which is fine because the response is instantaneous.
- **Buffering cost.** A long streaming response (10k tokens) must be buffered before it can be cached. Worst case: the user cancels mid-stream; the buffer is discarded and nothing is cached. This is acceptable — streaming cancels are rare on a turn that ran successfully.
- **Cache key includes `stream: true|false`.** A streaming request and a non-streaming request for otherwise-identical inputs are distinct cache entries. They could share storage with stream-on-replay synthesized from the non-streaming entry, but that adds complexity for a marginal hit-rate gain (the two modes are usually called by different consumers).

Recommendation in the overview is replay-from-buffer. Pi is the dominant caller and streams by default; carving streaming out defeats the integration. The buffering cost is amortized against the cache hit savings on the next call.

## Why semantic (37F) is deferred and opt-in per role

The spec's "semantic cache" framing is compelling — GPTCache's published numbers (60–69% hit rates, ~68% fewer API calls, >97% positive-hit accuracy with tuned thresholds) are large enough to be worth chasing. But L2's correctness profile is fundamentally different from L1's:

- **L1 false-hit rate is zero by construction.** The cache key is the SHA-256 of everything that determines the response. A hit *cannot* be wrong.
- **L2 false-hit rate is bounded by the similarity threshold, not by zero.** A threshold of 0.95 cosine similarity is empirically good but mathematically permits a small fraction of confidently-wrong replays. Two prompts that differ only in a critical noun ("summarize this contract" vs "summarize this contact") can score above threshold on `all-MiniLM-L6-v2` because most of the sentence is shared.

The mitigation is per-role opt-in: a role author who knows their domain accepts the tradeoff. A `faq-bot` role with `cache: semantic` is sound because the answer space is bounded. A `code-review` role with `cache: semantic` is *unsound* because the user's intent is the diff, not the prose — paraphrasing the prose doesn't change the right answer to a degree that would dominate the similarity-distance metric.

This is why 37F ships after A–E are measured. A semantic-cache false-hit that ships before the trace can observe it is a correctness regression with no detection path. Once 37E lands, the `cache.lookup` event carries `similarity_score` on L2 hits, and the user can grep the trace for low-scoring hits to audit the threshold. Without that observability, L2 is a "trust me" feature — exactly what ADR-0001 was written against.

## File layout (storage)

```
$AICHAT_CONFIG_DIR/                          # e.g. ~/.config/aichat/
├── .cache/
│   ├── stages/                              # existing — Phase 10B StageCache (unchanged)
│   │   └── <sha256>.out
│   ├── transparent/                         # new — Phase 37C L1 ordinary-path cache
│   │   ├── <sha256>.out                     # response body
│   │   └── <sha256>.meta                    # CallMetrics JSON (input/output/cache tokens, cost, model, ts)
│   ├── server/                              # new — Phase 37D server-side L1 cache
│   │   └── <sha256>.out                     # canonicalized response (replay-from-buffer)
│   └── semantic/                            # new — Phase 37F L2 (opt-in roles only)
│       ├── index.hnsw                       # reused from src/rag/
│       └── entries.jsonl                    # (query_text, key_hash, embed_id, role, model, ts)
```

The three new directories are siblings of the existing `stages/` directory. They share the atomic-write + LRU-eviction infrastructure (37C uplift) but do not share entries — a `stages/` hit is not a `transparent/` hit, even when the inputs would collide, because the call sites are different and the determinism gates are different.

## How 37 composes with prior phases

| Prior phase | Composition |
|---|---|
| **Phase 10B** (StageCache for pipeline stages) | 37 reuses the `StageCache` primitive. 37C broadens the key shape; 37D adds an LRU front; 37F layers semantic lookup on top. 10B's existing two callers (`src/pipe.rs`, `src/knowledge/compile.rs`) are unchanged. |
| **Phase 11** (Context budget) | Cache hits return without consuming the per-turn budget. A `--budget` constraint that would otherwise truncate context is preserved verbatim from the cached response. |
| **Phase 17** (Server execution) | 37D's server cache sits in front of the existing `chat_completions_via_role` and `chat_completions` paths at [`src/serve.rs:1007`](../../src/serve.rs) and [`src/serve.rs:978`](../../src/serve.rs). Role-execution requests get cached the same way model-execution requests do. |
| **Phase 21** (DAG primitives) | DAG branches share the cache substrate. Phase 22D ("DAG stage caching") consumes 37's `cache.lookup` event to surface per-branch hit/miss in the DAG trace. |
| **Phase 32** (Pi cutover) | 37D's pi-bridge endpoints follow the existing `/v1/state/*` pattern — same bearer-token auth, same `bridgeFetch` plumbing in [`pi-extensions/src/index.ts`](../../pi-extensions/src/index.ts). |
| **Phase 36** (Pipeline stage config isolation) | Orthogonal. Stage config overrides do not change cache-key shape — a stage with `config_override: { use_tools: [read_file] }` produces a different `tools` element in the key, so cache entries are naturally distinct per override. |

## What 37 does *not* try to solve

- **L4 (multi-turn context reuse) as a separate cache.** L4 is mostly subsumed by L3 once prefix-stability is enforced (37B2). The remaining gap — session compression invalidating the prefix mid-conversation — is flagged in the overview as a follow-up. Not in 37's scope.
- **Distributed cache.** (Future Phase Work — now [Phase 39](phase-39-overview.md)) : Within Phase 37 the cache is per-machine; a team sharing roles via git does not share cache entries. The original "cost-benefit on a CLI is poor" reasoning stands *for the default build* — so [Phase 39](phase-39-overview.md) ports LiteLLM's Redis/S3/GCS/Azure backends **cargo-gated** behind the [Phase 38](phase-38-overview.md) `CacheBackend` trait, off by default and adding zero dependencies unless opted in. [Phase 39D](phase-39-overview.md) adds the opt-in cross-machine team cache (shared back tier, per-namespace). See [`EVAL-0004`](../analysis/caching/EVAL-0004-litellm-cache-parity.md) §2.8/§3.2.
- **Cache warming.** No "pre-populate the cache from a corpus" CLI command in 37. The cache fills naturally as users invoke roles. (Becomes trivial once [Phase 41B](phase-41-overview.md) makes entries addressable by key — still not on 37's critical path.)
- **Cache compression.** Stored response bodies are uncompressed text. A 4 KiB response at zero compression is fine on modern disk; gzip-on-disk could be added later but is not on the critical path.
- **Cross-provider cache transfer.** A response from Claude Sonnet 4.6 is not a cache hit for the same prompt to OpenAI gpt-5. The cache key includes `model_id` precisely because models produce different responses.

## Risks and correctness hazards (summary)

Already enumerated in detail in EVAL-0002 §6. Re-stated here in the order they bite:

1. **Stale replay.** A role/prompt/tool-schema edit must invalidate L1 entries. The 37C key broadening covers all six determinants (model, system, messages, sampling, tools, schema); mtime TTL is a backstop.
2. **Caching non-determinism.** Caching a `temperature > 0` turn replays one sample as if it were canonical. Determinism gating is a correctness requirement, not a tuning knob.
3. **Tool side effects.** Never cache a turn that ran tools. The general path inherits the rule from `pipe.rs`.
4. **Concurrency.** Atomic-write upgrade (write-temp-then-rename) is part of 37C, not deferred.
5. **Unbounded growth.** LRU eviction with a 500 MiB default budget is part of 37C.
6. **Semantic false hits.** Mitigated by per-role opt-in (37F) and `similarity_score` in the trace.
7. **Training-data contamination.** Mitigated by `cache_hit: true` on `chat.response` events (37E).

## References

- [`docs/analysis/caching/EVAL-0002-full-caching.md`](../analysis/caching/EVAL-0002-full-caching.md) — the gap inventory; this phase implements its recommendations.
- [`docs/analysis/caching/EVAL-0004-litellm-cache-parity.md`](../analysis/caching/EVAL-0004-litellm-cache-parity.md) — LiteLLM feature-for-feature parity map; the 37→41 sub-track that turns this phase's `StageCache` into a pluggable-backend stack.
- [`phase-37-overview.md`](phase-37-overview.md) — the table, the per-item designs, and the file list. Read first.
- [Phase 38](phase-38-overview.md) / [39](phase-39-overview.md) / [40](phase-40-overview.md) / [41](phase-41-overview.md) — the rest of the caching sub-track.
- [`docs/analysis/caching/CLAUDE.md`](../analysis/caching/CLAUDE.md) — the open-harness workstream that 37E coordinates with.
- [`src/cache.rs`](../../src/cache.rs) — the existing primitive being broadened.
- [`src/repl/pi.rs`](../../src/repl/pi.rs) — why 37D is the pi integration.
- Anthropic Prompt Caching: https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching
