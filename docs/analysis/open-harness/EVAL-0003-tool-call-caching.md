# EVAL-0003: Tool-Call Caching — Gaps to an Intelligent Tool-Call Compiler

**Status:** Analysis, 2026-05-18
**Inputs:** `src/function.rs`, `src/client/common.rs`, `src/client/{claude,openai,gemini}.rs`,
`src/config/mod.rs`, `src/cache.rs`, `src/pipe.rs`, `CLAUDE_GENERATED.md`,
`EVAL-0002-full-caching.md` (open-harness workstream)
**Question:** Does aichat — or the L4 layer of `EVAL-0002` — contain anything resembling
an *intelligent tool-call compiler*? Is the hypothesis correct that **tool calls are not
yet built into enterprise cache pipelines**?

This is a critical gap analysis. It (1) confirms what `EVAL-0002` does and does not cover
for tool calls, (2) inventories what aichat actually does with tool calls today, (3)
defines "intelligent tool-call compiler" against the May-2026 research and provider state
of the art, and (4) validates the user's hypothesis with the necessary distinctions.

---

## 1. Direct answer: L4 of `EVAL-0002` does *not* include a tool-call compiler

The first instruction was to inspect **L4** of `EVAL-0002-full-caching.md` and confirm
whether it contains such a compiler. It does not — and the omission is structural, not
accidental.

`EVAL-0002` §1 defines L4 as **"Conversation / session reuse"**: caching *already-computed
context inside a multi-turn loop* so stable context is not re-sent and re-billed every
turn. Gap G (`EVAL-0002` §4) then explicitly reframes L4 as **"not a separate cache to
build but a discipline to enforce"** — keep the prompt prefix byte-stable across turns.
L4 is a *prefix-stability rule*. It plans nothing, schedules nothing, and memoizes no
tool result. There is no planner, no DAG, no executor — none of the three parts of a
compiler.

More telling: every time `EVAL-0002` mentions tool calls, it mentions them as a reason
**not** to cache:

- §4 Gap A item 2 — determinism gating: cache only when there are *"no tools (side
  effects)"*.
- §6 Risks — *"Tool side effects. **Never cache a turn that ran tools** — `pipe.rs`
  already refuses this; the general path must inherit the rule."*

So `EVAL-0002` treats a tool-using turn as an atomic, uncacheable black box. Its four
layers (L1 exact / L2 semantic / L3 provider prefix / L4 session reuse) all cache *around*
tool calls and never *into* them. **`EVAL-0002` has a blind spot exactly where this
document begins.** That blind spot is defensible for `EVAL-0002`'s scope — it analyzes
*response* caching — but it means the project currently has no analysis of the tool layer
at all. This document fills that gap.

---

## 2. What aichat does with tool calls today

Verified against source. The picture is consistent with `EVAL-0002` §2's verdict on
caching generally: a correct primitive or two, and nothing that touches the tool layer.

### 2.1 Dispatch is flat parallelism, not a plan

`eval_tool_calls` (`src/function.rs:36`) collects every tool call the model emitted in one
assistant turn and runs them concurrently with `futures_util::future::join_all`
(`function.rs:56-75`). That is the *entire* scheduling strategy:

- **No dependency analysis.** aichat does not inspect whether tool B's arguments depend
  on tool A's result. It cannot — by the time it sees the calls, the model has already
  serialized them into one batch or into sequential ReAct steps. aichat executes the
  batch the model gave it; it does not re-plan it.
- **No speculative or look-ahead execution.** There is no attempt to start a likely-next
  tool before the model asks for it.
- **`ToolCall::dedup`** (`function.rs:320-337`) removes duplicates *by tool-call ID
  within a single batch*. It is not a cache: it never spans turns and keys on the
  provider-assigned ID, not on `(name, args)`.

### 2.2 No tool-result memoization anywhere

There is no `HashMap<(tool, args), result>`, no disk cache, no TTL store for tool
output. The same tool invoked with identical arguments in turn 3 and turn 7 of the same
session executes twice. `StageCache` (`src/cache.rs`) is a *pipeline-stage* cache and is
**explicitly forbidden from caching tool-using work**: `cache.rs:8` documents *"Not
cached: tool-using stages (non-deterministic side effects)"*, and `pipe.rs:402` computes
`cache_enabled = !no_cache && !dry_run && !has_tools`. aichat's one caching primitive is
hard-wired to refuse the tool layer.

### 2.3 No provider prompt caching of tool definitions

`claude_build_body` (`src/client/claude.rs:311-346`) serializes the `tools` array raw.
The string `cache_control` does not appear anywhere in `src/` (confirmed). Gemini's
`cachedContent` is likewise absent. The `tools` block — typically the *largest stable
prefix* in an agentic request — is re-sent and (on Anthropic) re-billed at full price
every single turn.

### 2.4 Tool definition order is non-deterministic — an active L3 hazard

`select_functions` (`src/config/mod.rs:2226-2405`) assembles the tool list by iterating
collections seeded from a `HashSet` (`config/mod.rs:2249-2315`). The resulting `tools`
array is **not sorted**. Two invocations of the same role can serialize the same tool set
in different byte orders.

This is worse than "no caching." OpenAI and Gemini apply prefix caching *automatically*,
and on both, `tools` sits in the cached prefix. A `tools` array whose byte order changes
run-to-run **defeats the provider's automatic cache before any aichat code runs** — and on
Anthropic, per the official rule, *any* change to tool definitions invalidates the tools
cache *and everything downstream* (system + messages). Non-deterministic tool ordering is
a silent, unmeasured cache-busting bug.

### 2.5 No tool-aware accounting

`CallMetrics` (`src/client/common.rs:340`) carries `input_tokens` / `output_tokens` and no
cache fields (this is `EVAL-0002` Gap C). It additionally has no notion of tool-token
cost, tool latency, or per-tool cache outcome. The system cannot even *observe* a
tool-cache hit, which means it cannot be regression-tested — see §6.

### 2.6 The Phase 21 DAG is not a tool-call compiler

aichat *does* have a DAG executor (`src/pipe.rs`, Phase 21). It is worth being precise:
it schedules **roles / pipeline stages** — multi-model orchestration, parallel branches,
switch arms — where each node is itself a full `call_react` turn. It does **not** build a
DAG over the *tool calls within* a turn. The compiler-shaped machinery exists one level
too high. The tool layer underneath each DAG node is still flat `join_all`.

---

## 3. What "an intelligent tool-call compiler" actually means

The user's phrase maps onto a well-defined and now-substantial body of work. A compiler,
by analogy to classical compilers, has a **front end** (parse the model's intent into an
IR), a **middle end** (optimize: schedule, parallelize, memoize), and a **back end**
(execute). Four research systems, taken together, define the target:

**Planning + parallel scheduling — `LLMCompiler` (Kim et al., ICML 2024).** The canonical
"LLM compiler." Three components: a *Function Calling Planner* that emits a **DAG of tasks
with inter-dependencies**, a *Task Fetching Unit* that dispatches ready tasks, and an
*Executor* that runs them in parallel. Reports up to **3.7× latency** and **6.7× cost**
reduction vs. ReAct. This is the front-end + scheduler aichat lacks: aichat never builds
the DAG, so it can only run whatever the model already serialized.

**Result memoization — request-scoped and cross-turn.** The practitioner consensus
(LiteLLM, agent-backend write-ups) is to memoize **deterministic** tool I/O *separately
from* LLM responses, keyed on `toolName + normalizedArgs + permissionScope`, because a
deterministic tool result stays valid even when the surrounding conversation drifts.
aichat's `dedup` is the degenerate, single-batch case of this.

**Caching plans with invalidation — `ToolCacheAgent` (OpenReview, 2025).** An
"agent-for-agents" that auto-generates, per tool, a **caching plan** specifying
*cacheability*, *expiration*, and *inter-tool invalidation rules* so a stateful tool
(a write) correctly evicts the cached reads it affects. Reports up to **1.69× latency**
with no accuracy loss. This is the middle-end correctness machinery that makes memoizing
non-pure tools safe — the thing `cache.rs:8` gave up on by banning tool stages outright.

**Trajectory / prefix caching — `TVCACHE` (arXiv:2602.10986, 2026).** Maintains a *tool
call graph* — a tree of observed tool-call sequences — and does **longest-prefix matching**:
an exact trajectory match returns the cached result; a partial match **forks the sandbox
at the last matching node and executes only the unmatched tail**. Up to **70% hit rate**
and **6.9× faster** median tool execution. This is the same prefix-cache idea L3 applies
to *tokens*, applied instead to *tool-call sequences* — and it is exactly the construct
`EVAL-0002`'s L1–L4 lattice has no slot for.

So an **intelligent tool-call compiler** = (a) plan the model's tool intent into a
dependency DAG, (b) schedule it for maximum safe parallelism, (c) memoize deterministic
results with proper invalidation, (d) cache whole trajectories by prefix, and (e) keep the
serialized tool definitions byte-stable so the *provider's* KV cache also hits. aichat has
**(b) in its weakest form** (flat `join_all`) and **none of (a), (c), (d), (e)**.

---

## 4. State of the art: are tool calls in provider / enterprise cache pipelines?

This is the empirical half of the hypothesis. The answer requires splitting "cache
pipeline" into the two layers it conflates.

### 4.1 Provider prefix/KV caches — tool *definitions* and *blocks* ARE supported

The hypothesis ("tool calls are not built into enterprise cache pipelines") is, at the
**provider prefix-cache layer (L3)**, **already false** — and aichat is simply not using
what exists:

- **Anthropic.** You may place `cache_control: {type:"ephemeral"}` on the **last tool in
  the `tools` array** to cache *all* tool definitions; the cache hierarchy is explicitly
  `tools → system → messages`. `tool_use` and `tool_result` blocks inside `messages` are
  also cacheable like any content block. Pricing: cache **read = 0.1×**, 5-min write =
  1.25×, 1-hour write = 2×. **Caveat:** any edit to a tool name/description/parameter
  invalidates the tools cache *and all downstream levels*.
- **OpenAI.** Automatic prefix caching (≥1024 tokens) covers the `tools`/function-schema
  block because it sits in the prompt prefix; cached input is ~90% cheaper, and 2026
  *extended caching* holds prefixes up to 24h. No opt-in needed — but it only fires if
  the prefix, **tool definitions included**, is byte-stable (directly indicting §2.4).
- **Google Gemini.** Implicit caching (2.5+) and explicit `CachedContent` both cover tool
  declarations as part of the cached context.

So providers *do* build the **tool-definition prefix** and the **tool_use/tool_result
bytes** into their cache pipelines. What they cache is **tokens** — the cost of *prefilling
the description of a tool* and of *re-reading the transcript of a past tool call*.

### 4.2 What providers and enterprise gateways do NOT cache — the real gap

No provider, and no L1/L2 response-cache gateway, caches the **execution semantics** of a
tool call. Specifically, absent from every shipping enterprise pipeline:

1. **Tool-result memoization.** No provider returns "you already called `get_weather(NYC)`
   2 minutes ago, here is the result, no execution." The provider never runs the tool — it
   only caches the *token representation* of a result you supplied. Memoizing the
   *execution* is left entirely to the application.
2. **Trajectory caching.** Nothing TVCACHE-shaped — longest-prefix match over tool-call
   graphs — exists in any provider API or in GPTCache/LiteLLM-class gateways.
3. **L1/L2 response caches actively *exclude* tool turns.** GPTCache and LiteLLM cache the
   *final* response; a turn that emits a `tool_use` is non-terminal and non-deterministic,
   so it is skipped — the same exclusion `cache.rs:8` and `EVAL-0002` §6 make.

This is why §3's systems are **research papers (2024–2026), not API features**.
`LLMCompiler`, `ToolCacheAgent`, `TVCACHE`, and cross-region work like `Asteria` are the
active frontier precisely *because* the providers stop at the token-prefix layer.

### 4.3 Verdict on the hypothesis

**The hypothesis is correct, with one precision.** Restated accurately:

> Tool-call **execution semantics** — result memoization, trajectory caching, dependency
> planning — are **not** built into provider or enterprise cache pipelines. They remain a
> research frontier and an application responsibility. What providers *do* cache is the
> **token prefix**: tool *definitions* and the *bytes* of past tool_use/tool_result
> blocks.

And the sharper, aichat-specific finding: **aichat captures neither half.** It does not
memoize tool execution (the part nobody ships), *and* it does not opt into the provider
tool-definition prefix caching that everybody ships — in fact §2.4's non-deterministic
ordering means aichat *defeats* the provider caching it would otherwise get for free.

---

## 5. Gap analysis — aichat vs. an intelligent tool-call compiler

| # | Gap | Compiler stage | State in aichat | Payoff | Cost |
|---|---|---|---|---|---|
| **T1** | Tool definitions not byte-stable | back-end / L3 enabler | `select_functions` emits `HashSet`-ordered `tools` (`config/mod.rs:2249`) — defeats automatic provider caching | **High** (free L3 on OpenAI/Gemini once fixed) | **Trivial** — sort the array |
| **T2** | No `cache_control` on `tools` | back-end / L3 | absent from `claude_build_body` (`claude.rs:311`) | High (0.1× re-read of the whole tool block on Anthropic) | Low — one breakpoint on the last tool |
| **T3** | No tool-result memoization | middle-end | none; `dedup` is single-batch only (`function.rs:320`) | High on read-heavy / idempotent tools | Medium — needs purity classification |
| **T4** | No caching plan / invalidation model | middle-end | `cache.rs:8` bans tool stages outright instead | Unlocks T3 safely | Medium — per-tool `cache:` metadata + invalidation rules |
| **T5** | No tool-call dependency DAG | front-end + scheduler | flat `join_all` (`function.rs:56`); Phase 21 DAG is role-level only | Medium (latency; LLMCompiler-class 3.7×) | High — a planner is real work |
| **T6** | No trajectory cache | middle-end | none | Medium; high in eval/replay & training-data loops | High — TVCACHE-class, research-grade |
| **T7** | No tool-aware accounting | observability | `CallMetrics` (`common.rs:340`) has no tool/cache fields | Unblocks measuring T1–T6 | Low |
| **T8** | No trace event for tool-cache outcomes | observability | SPEC-001 has no tool-cache event (extends `EVAL-0002` Gap E) | Blocks regression-testing the above | Low — schema bump |

### 5.1 The cheap, unambiguous wins: T1, T2, T7

T1 and T2 are not "compiler" features — they are **hygiene that lets the provider's
existing cache pipeline do its job**. T1 is a one-line sort with no design question and no
downside; it should land regardless of any other decision here. T2 is a single
`cache_control` breakpoint. T7 (extend `CallMetrics`) is `EVAL-0002` Gap C plus tool
fields — do it once for both. These three are pure upside and gate the measurement of
everything else.

### 5.2 The real design work: T3 + T4 (and only then T5/T6)

Memoizing tool *results* (T3) is where `cache.rs:8` and `EVAL-0002` §6 drew a hard "no,"
and they were right *given no invalidation model*. The unlock is T4: a per-tool **purity /
caching-plan** declaration. The llm-functions ecosystem already authors tools with
argc comment-driven schemas — that is the natural place for a `cache:` annotation
(`pure` / `ttl=N` / `never`, plus inter-tool invalidation hints, à la `ToolCacheAgent`).
A `pure` tool (a unit-conversion, a hash, a deterministic DB read) is safe to memoize on
`(name, normalizedArgs)`; a stateful tool is not. Without T4, T3 is a correctness hazard;
with it, T3 is the highest-value tool-layer cache aichat can ship.

T5 (a `LLMCompiler`-style planner) and T6 (a `TVCACHE`-style trajectory cache) are
genuinely larger — a planner changes the agent loop's control flow, and trajectory caching
needs sandbox/state snapshotting aichat does not have. They are **deferred**, exactly as
`EVAL-0002` deferred its L2. But note T6's special relevance to the open-harness workstream:
a trajectory cache and the trace format are the *same data structure viewed twice* — the
trace already records tool-call sequences; a trajectory cache is that recording made
executable. If the trace schema is being designed now (`CLAUDE_GENERATED.md`), it is cheap
to keep it trajectory-cache-shaped and expensive to retrofit later.

---

## 6. Interaction with the open-harness workstream

`CLAUDE_GENERATED.md` makes the trace the keystone for testing and training-data
extraction. Two consequences for the tool layer:

- **Extend `EVAL-0002` Gap E with a tool-cache outcome.** A `tool.executed` event needs a
  `cache: {outcome: hit|miss|memoized|replayed, key_hash}` field. Without it, T3's
  memoization is invisible to SPEC-002 control-flow tests — a regression that silently
  re-executes every tool cannot be caught, the precise failure mode ADR-0001 exists to
  prevent.
- **Replayed tool results must be flagged — a correctness hazard.** `EVAL-0002` §6 already
  flags this for model responses: a replayed cached response must not be mined as a fresh
  model output. The same is true one layer down — a *memoized tool result* must not be
  mined as evidence the tool *ran*. The trace must distinguish a live tool execution from
  a cache replay, or the deferred training pipeline learns from fiction.

---

## 7. Verdict

1. **L4 of `EVAL-0002` contains no tool-call compiler.** L4 is session prefix-stability, a
   discipline. `EVAL-0002`'s entire L1–L4 model caches *around* tool calls and explicitly
   refuses to cache *into* them. The tool layer is an unanalyzed blind spot in that
   document — which this one closes.

2. **The hypothesis holds, precisely stated.** Tool-call *execution semantics* — result
   memoization, trajectory caching, dependency planning — are **not** in any provider or
   enterprise cache pipeline as of May 2026; they are a 2024–2026 research frontier
   (`LLMCompiler`, `ToolCacheAgent`, `TVCACHE`). What providers *do* cache is the **token
   prefix**: tool *definitions* (Anthropic `cache_control` on `tools`; OpenAI/Gemini
   automatic) and the *bytes* of past tool_use/tool_result blocks.

3. **aichat is behind on both counts.** It builds none of the research-grade compiler
   (no planner, no memoization, no trajectory cache) — expected, that is frontier work —
   *and* it fails to use the provider tool-definition caching that already ships, because
   `select_functions` emits a non-deterministically ordered `tools` array (`config/mod.rs:2249`)
   that defeats automatic provider caching before aichat's code even runs.

4. **Sequencing.** T1 (sort `tools`) → T2 (`cache_control` on tools) → T7 (tool/cache
   accounting) are trivial-to-low cost, pure upside, and should land first and together —
   they make the provider's *existing* tool-cache pipeline work for aichat and make it
   measurable. T3 + T4 (memoization gated on a per-tool purity/caching-plan declaration)
   is the real, worthwhile design work and the highest-value tool-layer cache aichat can
   own. T5 (planner) and T6 (trajectory cache) are deferred — but the open-harness trace
   schema should be kept trajectory-shaped *now*, because the trace and a trajectory cache
   are the same structure and retrofitting is expensive.

The distance from aichat to an intelligent tool-call compiler is not uniform: the L3
hygiene gap (T1/T2) is a same-day fix the project is simply leaving on the table, and the
compiler proper (T5/T6) is a real research-adjacent build. The middle — safe tool-result
memoization (T3/T4) — is the part that is both genuinely valuable and genuinely
attainable, and it is exactly the part `EVAL-0002` declared out of bounds.

---

## Sources

- [LLMCompiler — An LLM Compiler for Parallel Function Calling (arXiv:2312.04511, ICML 2024)](https://arxiv.org/abs/2312.04511)
- [LLMCompiler — reference implementation](https://github.com/SqueezeAILab/LLMCompiler)
- [ToolCacheAgent — Accelerating LLM Agents Through Intelligent Tool Call Caching (OpenReview)](https://openreview.net/forum?id=tX3YcbNa5w)
- [TVCACHE — A Stateful Tool-Value Cache for Post-Training LLM Agents (arXiv:2602.10986)](https://arxiv.org/abs/2602.10986)
- [Asteria — Semantic-Aware Cross-Region Caching for Agentic LLM Tool Access (arXiv:2509.17360)](https://arxiv.org/html/2509.17360v1)
- [Anthropic — Prompt caching (tool definitions, tool_use/tool_result, hierarchy, pricing)](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- [OpenAI — Prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching)
- [OpenAI — Prompt Caching 201 cookbook](https://developers.openai.com/cookbook/examples/prompt_caching_201)
- [Google — Context caching (Gemini API)](https://ai.google.dev/gemini-api/docs/caching)
- [How Local Prompt Caching Reduces Tokens in Tool-Driven LLM Workflows (Kerno)](https://www.kerno.io/blog/how-local-prompt-caching-rediuces-tokens-in-tool-driven-llm-workflows)
- [12 Inference Caching Plays for LLM Backends (Modexa)](https://medium.com/@Modexa/12-inference-caching-plays-for-llm-backends-4bbc2ba96bc8)
