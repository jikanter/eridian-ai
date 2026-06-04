# EVAL-0002: Gaps to End-to-End Caching of Model Responses

**Status:** Analysis, 2026-05-18
**Inputs:** `src/cache.rs`, `src/pipe.rs`, `src/client/{claude,openai,gemini,common,model}.rs`,
`src/config/{mod,session}.rs`, `src/serve.rs`, `src/cli.rs`, `CLAUDE.md`,
`SPEC-001-trace-format.md` (open-harness workstream)
**Question:** What stands between aichat as it exists today and *end-to-end caching of
model responses* — caching that both **removes turns** (the model is never called) and
**shrinks the hot path** (when the model *is* called, it does less work)?

This is a critical gap analysis, not a design doc. It inventories what is built,
names every gap against a layered model of response caching, grounds that model in
the current provider state of the art, and ranks the gaps by payoff against the
project's governing constraint — **cost-conscious above all**.

---

## 1. Framing: "caching a model response" is four different things

"End-to-end caching" collapses four mechanisms that live at different layers and
buy different things. Conflating them is the main reason the current codebase looks
"mostly done" when it is not. The four layers, ordered from closest-to-user to
closest-to-model:

| # | Layer | What it caches | What it removes | Where it lives |
|---|---|---|---|---|
| L1 | **Exact response cache** | Full final response for an identical request | The entire turn — zero provider calls | aichat process / disk |
| L2 | **Semantic response cache** | Response keyed by *meaning* of the request | The entire turn for near-duplicate requests | aichat process / disk + embeddings |
| L3 | **Provider prompt cache** | The provider's KV-cache of the prompt prefix | ~85% of prefill latency, 75–90% of input-token cost — but the turn still happens | Provider, opted into on the wire |
| L4 | **Conversation / session reuse** | Already-computed context inside a multi-turn loop | Re-sending and re-billing stable context every turn | aichat ↔ provider |

L1/L2 *reduce the number of turns*. L3/L4 *shrink the hot path of a turn that still
happens*. The brief asked for **both**. aichat today has a partial L1 and nothing
defensible at L2, L3, or L4. That is the headline.

---

## 2. What is actually built today

### 2.1 `StageCache` — a partial L1, scoped to two callers

`src/cache.rs` is a content-addressable file cache: key = `SHA-256(role \0 model \0 input)`,
value = the stage's text output, one file per entry under `.cache/stages/`, expiry by
file mtime vs. a TTL. It is sound for what it is — null-byte-delimited keys, TTL
expiry, unicode-safe, dir auto-create, eight unit tests.

But it is **not a response cache for aichat**. It is a *pipeline stage* cache. Its two
and only two callers are:

- `src/pipe.rs:410` — pipeline stage execution. Gated by `cache_enabled =
  !no_cache && !dry_run && !has_tools`.
- `src/knowledge/compile.rs:370` — per-file knowledge extraction.

The single ordinary path a user takes — `aichat "question"`, `aichat -r role <input>`,
or any REPL / pi turn — runs through `call_chat_completions` / `call_react`
(`src/client/common.rs`) and **never consults `StageCache`**. The `--no-cache` flag is
declared in `src/cli.rs:197` with `requires = "pipe"`: caching is structurally a
pipeline-only feature. A user asking the same question twice in a row pays for it
twice.

### 2.2 No provider prompt caching anywhere on the wire

`claude_build_body` (`src/client/claude.rs:292`) assembles `system`, `messages`, and
`tools` and emits them raw. There is no `cache_control` block on the system prompt,
on tool definitions, or on any message — verified: the string `cache_control` does
not appear in `src/`. The same is true of `gemini.rs` (no `cachedContent`,
no `CachedContent` resource) and `openai.rs`.

OpenAI and Gemini 2.5 apply prefix caching *automatically* server-side, so aichat
gets some L3 benefit on those providers **by luck of message ordering, not by
design**. Anthropic prompt caching is **explicit and opt-in** — aichat gets exactly
zero of it. On the project's own stated priority (local + frontier parity, token
cost), leaving Anthropic caching on the table is the single most expensive omission.

### 2.3 No cache accounting — the system is blind to caching it isn't doing

`CallMetrics` (`src/client/common.rs:340`) carries `input_tokens` and `output_tokens`
and nothing else. The Claude streaming and non-streaming extractors read
`usage.input_tokens` / `usage.output_tokens` only (`claude.rs:164`, `claude.rs:410`)
and **silently drop** `cache_creation_input_tokens` and `cache_read_input_tokens`.
The OpenAI extractor does not read `prompt_tokens_details.cached_tokens`. Gemini's
`cachedContentTokenCount` is ignored.

`compute_cost` (`common.rs:362`) is therefore a flat
`input·input_price + output·output_price`. It cannot represent a cache-read token at
0.1× or a cache-write token at 1.25×. Even on providers doing automatic caching,
aichat **over-reports cost** and cannot tell a user how much caching saved — or
failed to save. The `cacheRead`/`cacheWrite` keys in `session.rs:1128` are hard-coded
zeros in the pi-export shim; they are a schema placeholder, not a measurement.

### 2.4 `serve.rs` is an uncached pass-through

The OpenAI-compatible server (`src/serve.rs`) proxies `/v1/chat/completions`
straight to the upstream client with no response cache and explicit
`Cache-Control: no-cache` headers on its own responses. Every identical request a
downstream tool (or the pi REPL bridge) makes is a fresh, fully-billed model call.
For a server whose *point* is to sit between many callers and one model, this is the
highest-leverage missing cache in the codebase.

### 2.5 Session compression is context management, not caching

`session.rs` compresses history past a token threshold. That bounds context growth;
it does not cache anything. Compression actually *fights* L3/L4: rewriting the
history prefix invalidates whatever provider prefix cache had formed. The two
features need to be made aware of each other (see §5.4).

**Summary of the build state:** a correct L1 primitive wired to 2 of N callers; no
L2; no L3; no L4; no cache accounting; an uncached server. aichat is roughly 15% of
the way to end-to-end response caching, and the built 15% is the cheap part.

---

## 3. Current state of the art (provider + ecosystem, May 2026)

Caching is no longer optional infrastructure — it is how the providers expect to be
called, and pricing is now built around it.

**Anthropic — explicit, developer-controlled.** Mark up to 4 cacheable breakpoints
with `cache_control: {type: "ephemeral"}`; the cache covers the whole prefix
(`tools` → `system` → `messages`) up to and including the marked block. 5-minute
write costs 1.25× base input price, 1-hour write 2×, **cache read 0.1× (a 90%
discount)**. Up to ~85% latency reduction on long prefixes. Bedrock exposes the same.
Nothing happens unless you send the breakpoint — this is the gap that costs real
money in §2.2.

**OpenAI — automatic, prefix-based.** Prompts ≥1024 tokens automatically reuse the
longest previously-computed prefix, in 128-token increments. ~50% discount on cached
input, up to 80% latency reduction. No API change required to *get* it — but
`usage.prompt_tokens_details.cached_tokens` must be *read* to know it happened, and
prompt construction must be **prefix-stable** to *earn* it (volatile content — a
timestamp, a fresh RAG block — at the front kills the hit).

**Google Gemini — implicit + explicit.** Gemini 2.5+ has implicit caching on by
default (75–90% discount on prefix hits); explicit caching via a `CachedContent`
resource referenced by `cachedContent` gives a guaranteed 90% discount with a
storage fee. `cachedContentTokenCount` reports the hit.

**The cross-provider rule:** L3 is *prefix-stable prompt construction* plus, for
Anthropic, *explicit breakpoints*. It is a property of how aichat orders bytes, not
a feature it bolts on.

**Ecosystem L1/L2.** GPTCache established the standard shape: exact-match keying
backed by an embedding-similarity tier in a vector store, pluggable eviction.
Published semantic-cache results report 60–69% hit rates and ~68% fewer API calls
on duplicate-heavy workloads, with >97% positive-hit accuracy when the similarity
threshold is tuned. 2025 work (Proximity, LSH-bucketed caches) focuses on cutting
cache *lookup* latency so the cache never becomes its own hot path. LiteLLM and
similar gateways now ship L1 response caching as a standard proxy feature — directly
relevant to what `serve.rs` is missing.

---

## 4. Gap analysis, layer by layer

### Gap A — L1 exact cache is not on the ordinary request path *(high payoff, low cost)*

`StageCache` works; it is simply not called from `call_chat_completions` /
`call_react`. The gap is integration, not invention:

1. A cache lookup at the top of the non-tool chat path, keyed over the full
   determinant of the response — **not** the current `(role, model, input)` triple,
   which is insufficient for a general turn. The real key must cover: model id,
   resolved system prompt, the full message list, temperature/top_p/sampling params,
   tool set, and output schema. Any of these changing must miss.
2. **Determinism gating.** Cache only when the result is reproducible: `temperature == 0`
   (or provider-deterministic), no tools (side effects), not a dry run, caching not
   disabled. `pipe.rs` already encodes this judgement for stages — lift it, don't
   re-derive it.
3. Drop `requires = "pipe"` from `--no-cache`; add a global `cache:`/`cache_ttl`
   config block and per-role `cache:` frontmatter so a role author can opt a
   non-deterministic role out.

This is the one gap that *removes whole turns* for the price of wiring up code that
already exists and is tested.

### Gap B — no provider prompt caching (L3) *(highest cost payoff, medium effort)*

Two sub-gaps:

- **B1 — Anthropic `cache_control` not emitted.** `claude_build_body` should place an
  `ephemeral` breakpoint after the last stable block. The stable prefix for aichat is
  `tools` + `system` (role body, MCP tool schemas, knowledge context) — exactly the
  large, reused content. One breakpoint at the end of `system`, optionally one after
  `tools`, captures most of the 90% read discount. Gate on prompt size (a breakpoint
  below the provider minimum is wasted) and on the model advertising support.
- **B2 — prompt assembly is not prefix-stability-aware.** Automatic caching on
  OpenAI/Gemini and explicit caching on Anthropic both require the *volatile* parts of
  the prompt (timestamps, per-turn RAG retrieval, dynamic agent `_instructions`) to sit
  **after** the stable parts. There is no audit that aichat's assembly order in
  `config/input.rs` + the per-client builders satisfies this. A reordering pass —
  stable system/tools first, volatile context last — is a prerequisite for *any* L3
  benefit and costs nothing at runtime.

Per the open-harness goal of an honest cost ledger, B is also where the trace format
should grow: see Gap E.

### Gap C — no cache accounting (L3 observability) *(low cost, unblocks everything)*

Extend `CallMetrics` with `cache_read_tokens` and `cache_write_tokens`. Teach each
extractor to populate them: Claude `cache_read_input_tokens` /
`cache_creation_input_tokens`, OpenAI `prompt_tokens_details.cached_tokens`, Gemini
`cachedContentTokenCount`. Make `compute_cost` price them at the provider's
multipliers (0.1× read, 1.25×/2× write for Anthropic; 0.5× read for OpenAI; 0.1× for
Gemini 2.5). Without this, B is unmeasurable and the project cannot prove the
cost-consciousness it claims as constraint #1. This should land **first** — it is the
instrument that tells you B is working.

### Gap D — `serve.rs` has no response cache (L1 at the server) *(high payoff)*

The server is the natural home for an L1 — possibly L2 — cache because it sees many
callers and the deterministic-request question is answerable there too
(`temperature`, `stream`, tool presence are all in the request body). A bounded
in-memory LRU keyed on the canonicalized request body, gated on determinism, with
`cached_tokens`-style fields surfaced in the response `usage`, turns the server into
a genuine turn-elimination layer for the pi REPL bridge and any downstream tool.
Streaming responses need either a no-cache carve-out or replay-from-buffer.

### Gap E — no cache events in the trace schema *(blocks the harness story)*

`CLAUDE.md` makes the trace the keystone of testing and training-data
extraction. SPEC-001's 13 event types have **no cache event**. A cache hit that
removes a turn currently emits only a `debug!` log (`pipe.rs:425`) — invisible to the
harness, so a regression that silently disables caching cannot be caught by a
control-flow test, and training-data extraction cannot distinguish a real model
response from a replayed one (a correctness hazard: replayed responses must not be
mined as fresh model outputs). Add a `cache.lookup` event `{layer: L1|L2|L3, outcome:
hit|miss|write, key_hash, tokens_saved, cost_saved}`. This is a `schema_version` bump
and should be coordinated with the trace workstream, not bolted on after.

### Gap F — no semantic cache (L2) *(deferred — real value, real risk)*

aichat already has the substrate: an embedding pipeline and a hybrid HNSW+BM25 store
in `src/rag/`. An L2 cache (embed the request, nearest-neighbour search cached
entries, return on cosine similarity above a threshold) is therefore *buildable*
without new dependencies. But L2 trades correctness for hit rate: a too-loose
threshold returns a confidently wrong answer with zero signal to the user. It should
be **explicitly opt-in per role**, ship after L1/L3 are solid and measured, and reuse
the RAG store rather than adding a vector DB. Treat it as a fast follow, not Phase 1.

### Gap G — multi-turn context reuse (L4) is unmanaged *(medium)*

In `call_react`'s tool loop and in REPL sessions, the stable context (system prompt,
tool schemas, early history) is re-sent every turn. With B in place this is *mostly*
subsumed by provider prefix caching — but two aichat behaviours actively break it:
session compression rewrites the prefix (§2.5), and any per-turn volatile injection
(Gap B2) shifts it. L4 is therefore not a separate cache to build but a **discipline
to enforce**: keep the prefix byte-stable across turns within a session, and when
compression *must* rewrite it, do so on a cadence that amortizes the cache-write
cost rather than every turn.

---

## 5. Recommendations, ranked by payoff ÷ cost

Ranked against constraint #1 (cost-conscious) and the brief's "reduce turns + shrink
hot path":

1. **Gap C — cache accounting.** Smallest change, and it is the instrument for
   everything else. Do it first; you cannot manage what you cannot measure.
2. **Gap B1 — Anthropic `cache_control`.** Largest single cost win (90% read
   discount on the stable prefix), self-contained to `claude_build_body`. Pair with
   B2's stability audit or it under-delivers.
3. **Gap A — L1 on the ordinary path.** Removes whole turns; the primitive is built
   and tested. Pure integration work. The only real design task is the correct cache
   key (§4 Gap A item 1) and honest determinism gating.
4. **Gap D — server response cache.** High leverage for the `--serve` / pi-bridge
   topology; reuses A's keying and gating logic.
5. **Gap E — trace cache events.** Sequence with the trace workstream's next
   `schema_version` bump; do not let A/D ship caching the harness is blind to.
6. **Gap B2 / Gap G — prefix-stability discipline.** Cheap at runtime, but a
   cross-cutting audit; fold into the B1 work.
7. **Gap F — semantic L2.** Deferred. Real value on duplicate-heavy workloads, real
   correctness risk; opt-in, post-measurement, on the existing RAG store.

### Sequencing note

C → B → A → D is the spine. C and B touch only the client layer and can land
independently of the open-harness trace work. A and D should not ship before E, or
the project acquires a turn-elimination path with no observability — exactly the
failure mode ADR-0001 was written to prevent.

---

## 6. Risks and correctness hazards

- **Stale replay.** A role/prompt/tool-schema edit must invalidate L1 entries. The
  key must hash *everything* that determines output (§4 Gap A); mtime TTL alone is a
  backstop, not the mechanism. `StageCache`'s current `(role, model, input)` key is
  too narrow for a general turn — it omits system prompt, params, tools, schema.
- **Caching non-determinism.** Caching a `temperature > 0` turn replays one sample as
  if it were canonical. Determinism gating is a correctness requirement, not a tuning
  knob.
- **Tool side effects.** Never cache a turn that ran tools — `pipe.rs` already refuses
  this; the general path must inherit the rule.
- **Concurrency.** `StageCache::put` is a plain `fs::write`; two processes writing the
  same key can interleave. Move to write-temp-then-atomic-rename before A multiplies
  the caller count.
- **Unbounded growth.** `.cache/stages/` has TTL expiry but no size cap and no
  eviction. A general L1 needs an LRU or size budget, or it becomes a disk leak.
- **Semantic false hits (L2).** Covered in Gap F — the dominant reason to defer it.
- **Training-data contamination.** Per Gap E, a replayed cached response must be
  flagged in the trace so the deferred training pipeline never mines it as a fresh
  model output.

---

## 7. Verdict

aichat has a correct L1 cache *primitive* and zero of the three layers that matter
most for the brief's goal. The distance to "end-to-end caching of model responses"
is not a research distance — every gap here is a known, scoped engineering task and
two of the highest-payoff ones (C, B1) are confined to the client layer. The work
divides cleanly:

- **Removes turns:** Gap A (L1 on the request path), Gap D (server cache), Gap F (L2).
- **Shrinks the hot path:** Gap B (provider prompt caching), Gap G (prefix stability).
- **Makes both legible:** Gap C (accounting), Gap E (trace events).

Do C first, B second, A third. That ordering buys the largest cost reduction
earliest, keeps every change measurable, and never ships a cache the open harness
cannot see.

---

## Sources

- [Anthropic — Prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- [Anthropic — Pricing](https://platform.claude.com/docs/en/about-claude/pricing)
- [OpenAI — Prompt Caching in the API](https://openai.com/index/api-prompt-caching/)
- [OpenAI — Prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching)
- [Google — Context caching (Gemini API)](https://ai.google.dev/gemini-api/docs/caching)
- [Google — Gemini 2.5 models now support implicit caching](https://developers.googleblog.com/gemini-2-5-models-now-support-implicit-caching/)
- [Amazon Bedrock — Prompt caching](https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html)
- [GPTCache — Semantic cache for LLMs](https://github.com/zilliztech/GPTCache)
- [GPT Semantic Cache: Reducing LLM Costs and Latency via Semantic Embedding Caching (arXiv:2411.05276)](https://arxiv.org/abs/2411.05276)
- [LiteLLM — Prompt caching](https://docs.litellm.ai/docs/completion/prompt_caching)
- [ngrok — Prompt caching: 10x cheaper LLM tokens, but how?](https://ngrok.com/blog/prompt-caching)
