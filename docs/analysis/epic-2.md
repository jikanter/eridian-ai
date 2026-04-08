# Epic 2: Runtime Intelligence Layer

**Created:** 2026-03-16
**Status:** Planning
**Depends on:** Phase 8 (observability/tracing infrastructure)

> **[Updated 2026-04-07]** Unified roadmap expanded Phase 10 (10D=cost-aware routing, old 10D→10E) and Phase 11 (+11D=pipeline budget propagation). Epic cross-refs: old Epic 3→**5**, old Epic 4→**9**, old Epic 5→**10**.

---

## Motivation

The runtime between the user and the LLM is doing transport, not thinking. Every token sent to a model costs money; every CPU cycle the runtime spends reasoning about context is free. This epic builds the **deterministic intelligence layer** — the set of systems that sit between the user's input and the LLM call, reducing token waste, improving schema reliability, and recovering from failures without human intervention.

Six features are proposed. Each was identified by tracing the actual code paths in the current runtime and finding gaps where deterministic logic could replace or prevent LLM token spend.

### Governing Principle

> Every token sent to an LLM should be a token that only an LLM can process. If deterministic logic can resolve a question, it should never reach the model.

---

## Feature 1: Provider-Native Structured Output (`response_format` / tool-use-as-schema)

### Problem

When a role has `output_schema`, AIChat injects the schema into the system prompt as text (`src/config/role.rs:719-731`) and validates after the fact (`src/config/role.rs:835-848`). No provider-native structured output API is used anywhere in the codebase. This means:

- The LLM can ignore the schema entirely (returns prose, markdown, or partial JSON)
- Failure mode is **exit code 8 with no retry** — the pipeline crashes
- Cheaper models (deepseek-chat, gpt-4o-mini) are less reliable at following prompt-injected schemas
- Every schema failure wastes the tokens spent on that call

### Solution

Use provider-native `response_format` / tool-use-as-structured-output when the model supports it, falling back to prompt engineering for models that don't.

### Implementation

**New field on `ModelData`** (`src/client/model.rs:296+`):
```rust
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub supports_response_format_json_schema: bool,
```

Populated in `models.yaml` for models that support it: all OpenAI `gpt-4o-*`, `o1-*`, `o3-*`, `o4-mini-*`; all Gemini `2.0+`; all Claude `3.5+` via tool-use pattern; deepseek-chat.

**OpenAI path** (`src/client/openai.rs:226-344`):

In `openai_build_chat_completions_body`, after building the body, check if the model supports `response_format` AND the role has `output_schema` (passed through `ChatCompletionsData`):

```rust
if let Some(schema) = &data.output_schema {
    if model.data.supports_response_format_json_schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "output",
                "strict": true,
                "schema": schema
            }
        });
    }
}
```

When `response_format` is active, the system prompt schema suffix (`role.rs:719-731`) should be **suppressed** to avoid wasting tokens on redundant instructions.

**Claude path** (`src/client/claude.rs:156+`):

Claude doesn't support `response_format` directly but guarantees schema conformance via tool-use-as-structured-output. Define a synthetic tool whose `input_schema` IS the output_schema, force the model to call it via `tool_choice: {"type": "tool", "name": "structured_output"}`, and extract the tool call arguments as the output.

```rust
if let Some(schema) = &data.output_schema {
    // Add synthetic tool
    let tool = json!({
        "name": "structured_output",
        "description": "Return the structured output",
        "input_schema": schema
    });
    // ... inject into tools array, set tool_choice
}
```

**Fallback path**: Models without native support continue using the existing prompt-injection approach.

**`ChatCompletionsData` change** (`src/client/common.rs`):

Add `output_schema: Option<Value>` to `ChatCompletionsData`. Populated from `role.output_schema()` in `Input::prepare_completion_data()` (`src/config/input.rs:244-280`).

### Files to Modify

| File | Change |
|---|---|
| `src/client/model.rs` | Add `supports_response_format_json_schema` to `ModelData` |
| `models.yaml` | Add the boolean for supported models (~40 entries) |
| `src/client/common.rs` | Add `output_schema` to `ChatCompletionsData` |
| `src/config/input.rs` | Pass `output_schema` through `prepare_completion_data()` |
| `src/client/openai.rs` | Inject `response_format` in body builder when supported |
| `src/client/claude.rs` | Inject synthetic tool + `tool_choice` when output_schema present |
| `src/config/role.rs` | Suppress system prompt schema suffix when native mode active |

### Effort

Medium. ~200-300 lines changed across 7 files. The OpenAI path is straightforward. The Claude path requires careful handling of the tool_choice/tool_result extraction. Testing requires multiple provider accounts.

### Parallelization

This feature is **fully independent** of all other features in this epic. The OpenAI path and Claude path can be implemented by separate agents concurrently.

### Token Impact

Eliminates schema failure retries entirely for supported models. For unsupported models, no change. Removes ~50-200 tokens of system prompt schema injection when native mode is active.

---

## Feature 2: Schema Validation Retry Loop

### Problem

When `validate_schema("output", schema, &output)` fails (`src/config/role.rs:835-848`), the error propagates immediately to exit code 8. The LLM never sees the validation error. The entire invocation (and all preceding pipeline stages) is wasted.

This is the single most expensive failure mode: you pay for all tokens consumed, get nothing back, and the LLM had no opportunity to self-correct.

### Solution

On schema validation failure, inject the validation error into a new turn and retry. Configurable retry count (default: 1). After exhausting retries, fail with exit code 8 as today.

### Implementation

**New retry loop in `src/main.rs`** (around L367 where `validate_schema("output", ...)` is called):

```rust
// Pseudocode for the retry logic
let max_schema_retries = role.schema_retries().unwrap_or(1);
let mut output = initial_output;
let mut schema_attempt = 0;

loop {
    match validate_schema("output", schema, &output) {
        Ok(()) => break,
        Err(e) if schema_attempt < max_schema_retries => {
            schema_attempt += 1;
            // Inject validation error as a new user message
            let retry_prompt = format!(
                "Your previous output failed schema validation:\n{}\n\
                 Please regenerate conforming to the required schema.",
                e
            );
            let retry_input = input.clone().with_retry_prompt(&retry_prompt);
            let (new_output, _) = call_chat_completions(
                &retry_input, false, false, client.as_ref(), abort_signal.clone()
            ).await?;
            output = new_output;
        }
        Err(e) => return Err(e),
    }
}
```

**Same logic in `src/pipe.rs`** at lines 154-155 where per-stage output validation happens.

**New optional role frontmatter field**:
```yaml
schema_retries: 2   # default: 1 (0 = fail-fast, same as today)
```

**Interaction with Feature 1**: When `response_format` is active (Feature 1), schema validation retry is unnecessary because the API guarantees conformance. The retry loop should short-circuit when native structured output was used.

### Files to Modify

| File | Change |
|---|---|
| `src/config/role.rs` | Add `schema_retries` to role frontmatter; expose via accessor |
| `src/main.rs` | Wrap output validation in retry loop with feedback injection |
| `src/pipe.rs` | Same retry logic for per-stage output validation |
| `src/config/input.rs` | Add `with_retry_prompt()` method for injecting validation feedback |

### Effort

Small-medium. ~100-150 lines. The core logic is a simple loop. The nuance is correctly constructing the retry `Input` with the right message history (original messages + assistant response + validation error as user message).

### Parallelization

**Independent** of Features 1, 3, 5, 6. Has a logical dependency on Feature 1 (should short-circuit when native structured output is active), but can be built first and the short-circuit added later.

### Token Impact

On failure: costs 1 additional LLM call per retry attempt. On success: saves the entire cost of re-running the pipeline from scratch. Net positive for any pipeline with ≥2 stages.

---

## Feature 3: API-Level Retry with Backoff

### Problem

If a provider returns HTTP 429 (rate limit), 500 (server error), 502 (bad gateway), or 503 (service unavailable), the error propagates immediately to exit. There is no retry with backoff. For batch processing (`--each` with hundreds of records), a single rate limit response kills the entire run.

### Solution

Add a retry layer around the HTTP client calls with exponential backoff for transient errors. Fail immediately on non-transient errors (401, 403, 404).

### Implementation

**New module**: `src/client/retry.rs`

```rust
pub struct RetryConfig {
    pub max_retries: usize,        // default: 3
    pub initial_backoff_ms: u64,   // default: 1000
    pub max_backoff_ms: u64,       // default: 30000
    pub backoff_multiplier: f64,   // default: 2.0
}

pub fn is_retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503)
}

pub async fn with_retry<F, Fut, T>(config: &RetryConfig, operation: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    // Exponential backoff loop
    // On 429: check Retry-After header if present
    // On 500/502/503: fixed backoff
    // On other errors: fail immediately
}
```

**Integration points**: Wrap the HTTP calls in each provider's `chat_completions` and `chat_completions_streaming` methods. The `Client` trait methods (`src/client/common.rs:37+`) are the natural integration point — add a `retry_config()` method with sensible defaults that providers can override.

**Config**: Global `retry:` section in `config.yaml`:
```yaml
retry:
  max_retries: 3
  initial_backoff_ms: 1000
  max_backoff_ms: 30000
```

**429-specific handling**: Parse `Retry-After` header when present and use it as the backoff duration instead of the exponential default.

### Files to Modify

| File | Change |
|---|---|
| `src/client/retry.rs` | New file: retry logic with exponential backoff |
| `src/client/common.rs` | Add `retry_config()` to `Client` trait with default impl |
| `src/client/openai.rs` | Wrap HTTP calls in retry |
| `src/client/claude.rs` | Wrap HTTP calls in retry |
| `src/client/gemini.rs` | Wrap HTTP calls in retry |
| `src/client/openai_compatible.rs` | Inherits from OpenAI (may need no changes) |
| `src/client/vertexai.rs` | Wrap HTTP calls in retry |
| `src/client/bedrock.rs` | Wrap HTTP calls in retry |
| `src/client/cohere.rs` | Wrap HTTP calls in retry |
| `src/config/mod.rs` | Parse `retry:` config section |

### Effort

Medium. ~150-200 lines for the retry module. ~20-30 lines per provider for wrapping. Total ~350-400 lines across ~10 files.

### Parallelization

**Fully independent** of all other features. Provider-specific wrapping can be done by separate agents (one for OpenAI-family, one for Claude, one for Gemini/VertexAI/Bedrock).

### Token Impact

Zero token impact — this is HTTP-level retry, not LLM-level. Prevents wasted batch runs from transient failures.

---

## Feature 4: Pipeline Stage Retry and Model Fallback

### Problem

Pipeline stage failures (`src/pipe.rs:98-109`) are fatal and immediate. If stage 3 of a 4-stage pipeline fails, the successful (and paid-for) results of stages 1-2 are discarded. There is no mechanism to:

1. Retry a failed stage with the same model
2. Fall back to an alternative model on failure
3. Cache successful stage outputs for re-use

### Solution

Three additions to the pipeline runner, each independently useful:

**4A. Stage output caching**: After each successful stage, cache the output keyed on `hash(role_name + model_id + input_text)`. On re-run, check cache before executing. Configurable TTL.

**4B. Stage retry**: On stage failure, retry the stage up to N times (default: 1) before propagating the error.

**4C. Model fallback**: Roles can declare `fallback_models:` in frontmatter. On failure (including after retries), try the next model in the fallback chain.

### Implementation

**4A. Stage output caching**

New module: `src/cache.rs`

```rust
use std::path::PathBuf;
use sha2::{Sha256, Digest};

pub struct StageCache {
    dir: PathBuf,       // <config_dir>/.cache/stages/
    ttl_secs: u64,      // default: 3600 (1 hour)
}

impl StageCache {
    pub fn key(role: &str, model: &str, input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(role.as_bytes());
        hasher.update(model.as_bytes());
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn get(&self, key: &str) -> Option<String> { /* read file, check mtime vs TTL */ }
    pub fn put(&self, key: &str, output: &str) -> Result<()> { /* write file */ }
}
```

Integration in `pipe.rs:run_stage_inner()`: before calling the LLM, check cache. After successful completion, write to cache. CLI flag `--no-cache` bypasses.

**4B. Stage retry**

In `pipe.rs:run_stage()`, wrap `run_stage_inner()` in a retry loop:

```rust
let max_stage_retries = role.stage_retries().unwrap_or(1);
let mut attempt = 0;
loop {
    match run_stage_inner(config, stage, input_text, is_last, abort_signal.clone()).await {
        Ok(output) => return Ok(output),
        Err(e) if attempt < max_stage_retries && is_retryable_stage_error(&e) => {
            attempt += 1;
            warn!("Pipeline stage {}/{} failed (attempt {}), retrying...",
                  stage_index+1, stage_count, attempt);
        }
        Err(e) => return Err(/* wrap with PipelineStage error */),
    }
}
```

`is_retryable_stage_error` returns true for API errors (5/6), schema failures (8), and model errors (7). Returns false for config errors (3), auth errors (4), and user abort (9).

**4C. Model fallback**

New role frontmatter field:
```yaml
model: deepseek:deepseek-chat
fallback_models:
  - openai:gpt-4o-mini
  - openai:gpt-4o
```

In `pipe.rs:run_stage()`, after exhausting retries with the primary model, iterate through `fallback_models` and retry with each.

### Files to Modify

| File | Change |
|---|---|
| `src/cache.rs` | New file: content-addressable stage cache |
| `src/pipe.rs` | Cache check/write in `run_stage_inner`; retry loop in `run_stage`; fallback model iteration |
| `src/config/role.rs` | Add `stage_retries`, `fallback_models` to frontmatter |
| `src/cli.rs` | Add `--no-cache` flag |
| `Cargo.toml` | Add `sha2` crate (if not already present) |

### Effort

Medium-large. ~300-400 lines total.
- 4A (cache): ~120 lines, new file + pipe.rs integration
- 4B (retry): ~50 lines in pipe.rs
- 4C (fallback): ~80 lines in pipe.rs + role.rs

### Parallelization

4A, 4B, and 4C are **independently implementable** by separate agents. They integrate at different points in `pipe.rs:run_stage()`. 4A modifies `run_stage_inner` (cache check/write). 4B wraps `run_stage_inner` in a loop. 4C wraps the retry loop in a model iteration loop. The nesting order is: `4C(4B(4A(run_stage_inner)))`.

### Token Impact

- **4A (cache)**: Eliminates redundant stage executions on re-run. For `--each` batch processing with overlapping inputs, savings scale with data redundancy (20-80%).
- **4B (retry)**: Costs 1 additional stage call per retry. Saves the cost of re-running the entire pipeline.
- **4C (fallback)**: May cost more per-stage (if fallback model is more expensive), but saves the cost of total pipeline failure.

---

## Feature 5: Context Budget Allocator

### Problem

There is no system for allocating the model's context window. The only guard is `guard_max_input_tokens()` (`src/client/model.rs:284-292`) which hard-errors if total exceeds the limit. No truncation, no prioritization, no graceful degradation.

Specific gaps:
- `-f dir/` loads all files verbatim (`src/config/input.rs:57-122`), no relevance filtering
- RAG returns fixed top-k chunks regardless of remaining budget
- Session history grows unbounded until compression threshold
- No reservation for output tokens
- Token estimation is approximate (~1.3 tokens/ASCII word, `src/utils/mod.rs:75-91`)

### Solution

A `ContextBudget` allocator that:
1. Knows the model's context window and reserves space for output
2. Allocates fixed slots for non-negotiable content (system prompt, schema, user message)
3. Fills remaining budget by priority: tool schemas → file content (BM25-ranked) → RAG chunks → session history
4. Truncates intelligently instead of hard-erroring

### Implementation

**New module**: `src/context_budget.rs`

```rust
pub struct ContextBudget {
    total_budget: usize,          // model.max_input_tokens
    output_reserve: usize,        // model.max_output_tokens or default 4096
    fixed_allocations: usize,     // system prompt + schema + user message
    remaining: usize,             // total - output_reserve - fixed
}

pub struct BudgetAllocation {
    pub system_prompt: String,    // always included (non-negotiable)
    pub output_schema: Option<String>,  // always included if set
    pub user_message: String,     // always included
    pub tool_schemas: Option<Vec<FunctionDeclaration>>,  // deferred loading handles this
    pub file_contents: Vec<RankedContent>,  // BM25-ranked, truncated to budget
    pub rag_chunks: Vec<String>,  // top-k where k is budget-constrained
    pub session_history: Vec<Message>,  // most recent first, truncated to budget
}

pub struct RankedContent {
    pub path: String,
    pub content: String,
    pub relevance_score: f64,
    pub token_estimate: usize,
}

impl ContextBudget {
    pub fn allocate(
        model: &Model,
        role: &Role,
        input: &Input,
        query: &str,
    ) -> Result<BudgetAllocation> {
        // 1. Calculate fixed allocations
        // 2. Rank file contents by BM25 against query (bm25 crate already in deps)
        // 3. Fill budget greedily: files by relevance, then RAG chunks, then history
        // 4. Return allocation with everything fitting within budget
    }
}
```

**Integration point**: `Input::prepare_completion_data()` (`src/config/input.rs:244-280`). Before calling `build_messages()`, run the budget allocator to determine what content gets included. Replace the hard error in `guard_max_input_tokens()` with a warning when truncation was necessary.

**BM25 ranking for file contents**: The `bm25` crate is already a dependency. When `-f` includes multiple files or a directory, score each file (or file section) against the user's query text. Include files in descending score order until the budget is exhausted.

**Budget-aware RAG**: Pass remaining budget to `Config::search_rag()` (`src/config/mod.rs:1573-1586`) so it can compute `top_k = remaining_budget / avg_chunk_tokens` instead of using a fixed k.

**Config**:
```yaml
context_budget:
  output_reserve: 4096     # tokens reserved for output
  file_strategy: bm25      # bm25 | truncate | all (default: bm25)
  warn_on_truncation: true  # emit warning to stderr when content is truncated
```

### Files to Modify

| File | Change |
|---|---|
| `src/context_budget.rs` | New file: budget allocator with BM25 ranking |
| `src/config/input.rs` | Integrate budget allocator in `prepare_completion_data()` |
| `src/config/mod.rs` | Pass budget to RAG search; parse `context_budget:` config |
| `src/client/model.rs` | Soft-fail mode for `guard_max_input_tokens()` when budget allocator is active |
| `src/utils/mod.rs` | Expose `estimate_token_length` as public API for budget calculations |

### Effort

Large. ~400-500 lines. The BM25 ranking is the most complex part — it requires tokenizing file contents and the query, building an index, and scoring. The `bm25` crate handles the algorithm but the integration (chunking files, building the index per-invocation) is non-trivial.

### Parallelization

**Independent** of Features 1-4 and 6. However, the BM25 file ranking sub-component and the budget allocation core can be developed by separate agents:
- **Agent A**: `ContextBudget` struct, allocation algorithm, integration in `input.rs`
- **Agent B**: BM25 file ranking (scoring files against query, producing `RankedContent` list)
- **Agent C**: Budget-aware RAG (modifying `search_rag` to accept a token budget)

### Token Impact

The highest-leverage feature in this epic. Estimated savings:
- `-f dir/` with 10 files, query about 1 function: **40x reduction** (send 100 tokens vs 4000)
- RAG with budget-aware top-k: **20-50% reduction** vs fixed top-k
- Session history truncation: **3-5x reduction** on long sessions vs full history

---

## Feature 6: Capability-Aware Pre-Flight Validation

### Problem

Model capabilities (`supports_vision`, `supports_function_calling` in `ModelData`) are stored and displayed but never checked at dispatch time. A role with `use_tools` will happily send tool schemas to a model that has `supports_function_calling: false`, resulting in an API error that wastes the entire prompt's tokens.

Similarly, there is no validation that:
- A model's `max_input_tokens` is sufficient for the role's typical context
- A model supports vision when the input contains images (`-f image.png`)
- A pipeline's stage models are all configured and reachable

### Solution

Pre-flight validation that catches mismatches before any API call is made.

### Implementation

**New function**: `src/config/preflight.rs`

```rust
pub fn validate_model_capabilities(
    model: &Model,
    role: &Role,
    input: &Input,
) -> Result<Vec<PreflightWarning>> {
    let mut warnings = vec![];

    // Check: role has use_tools but model doesn't support function calling
    if role.use_tools().is_some() && !model.data.supports_function_calling {
        bail!("Role '{}' requires tool calling but model '{}' does not support it",
              role.name(), model.id());
    }

    // Check: input contains images but model doesn't support vision
    if input.has_images() && !model.data.supports_vision {
        bail!("Input contains images but model '{}' does not support vision",
              model.id());
    }

    // Warning: model context window may be too small for role's typical usage
    if let Some(max_input) = model.data.max_input_tokens {
        if max_input < 4096 {
            warnings.push(PreflightWarning::SmallContextWindow(max_input));
        }
    }

    Ok(warnings)
}

pub fn validate_pipeline(
    config: &Config,
    stages: &[PipelineStage],
) -> Result<Vec<PreflightWarning>> {
    // Check: all stage roles exist
    // Check: all stage models are configured
    // Check: output_schema of stage N is compatible with input_schema of stage N+1
    // Check: stage models support required capabilities (tools, vision)
}
```

**Integration point**: Call `validate_model_capabilities()` in `Input::prepare_completion_data()` before building the request. Call `validate_pipeline()` in `pipe.rs:run()` before the stage loop.

**Pipeline schema compatibility check**: When stage N has `output_schema` and stage N+1 has `input_schema`, validate that a document conforming to the output schema would pass the input schema validation. This is a deterministic check — no LLM needed.

### Files to Modify

| File | Change |
|---|---|
| `src/config/preflight.rs` | New file: capability validation, pipeline validation |
| `src/config/input.rs` | Call model capability check in `prepare_completion_data()` |
| `src/pipe.rs` | Call pipeline validation before stage loop |
| `src/config/input.rs` | Add `has_images()` method to Input |

### Effort

Small. ~150-200 lines. All checks are straightforward boolean comparisons against existing `ModelData` fields.

### Parallelization

**Fully independent** of all other features. Can be implemented by a single agent in one pass.

### Token Impact

Prevents wasted API calls entirely. A tool-using role sent to a non-function-calling model wastes the entire prompt (potentially thousands of tokens). Pre-flight catches this at zero cost.

---

## Cross-Feature Dependency Graph

```
Feature 1 (response_format) ──────────────────────────── Independent
Feature 2 (schema retry) ─── soft dep on F1 ──────────── Independent (add F1 shortcircuit later)
Feature 3 (API retry) ────────────────────────────────── Independent
Feature 4 (pipeline retry + cache) ──── 4A/4B/4C ────── Internally parallelizable
Feature 5 (context budget) ──── 5A/5B/5C ────────────── Internally parallelizable
Feature 6 (pre-flight) ───────────────────────────────── Independent
```

**Maximum parallelism**: 9 independent work streams:
- F1-OpenAI, F1-Claude (Feature 1 split by provider)
- F2 (schema retry)
- F3 (API retry)
- F4A (stage cache), F4B (stage retry), F4C (model fallback)
- F5A (budget allocator core), F5B (BM25 file ranking)
- F6 (pre-flight)

**Recommended implementation order** (if sequential):
1. F6 (pre-flight) — smallest, prevents the most obvious waste
2. F1 (response_format) — eliminates a class of failures entirely
3. F2 (schema retry) — recovers from remaining schema failures
4. F3 (API retry) — prevents transient failure crashes
5. F4A (cache) → F4B (retry) → F4C (fallback) — pipeline resilience
6. F5 (context budget) — largest, highest token savings, most complex

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| LiteLLM integration as a dependency | Python runtime conflicts with single-binary constraint. AIChat can already target a LiteLLM proxy via `openai-compatible` client with zero code changes. |
| Confidence scoring on LLM output | Research problem, not engineering. No reliable way to assess output quality without another LLM call (which defeats the cost-conscious purpose). |
| Automatic model selection (`model: auto`) | Requires a quality benchmark per model per task type. The compiled model database has pricing but not quality metrics. Manual model selection + fallback chains (Feature 4C) is more reliable. |
| Token-exact counting (tiktoken integration) | The `tiktoken-rs` crate only covers OpenAI tokenizers. For budget allocation (Feature 5), the existing heuristic (~1.3 tokens/ASCII word) is sufficient — budget decisions don't need token-exact precision, just order-of-magnitude correctness. |
| Prompt caching API integration | Provider-specific (Anthropic's cache_control, OpenAI's implicit caching). Worth building but belongs in a separate epic focused on provider-specific optimizations. |
| Full knowledge graph | Over-engineered for the current stage. Configuration-derived entity relationships (the implicit graph from role extends/includes/pipeline) are queryable via `--info -o json` without new infrastructure. |

---

## Success Metrics

| Metric | Current State | Target |
|---|---|---|
| Schema failure rate with `output_schema` | Unknown (no tracking) | <5% with F1, <1% with F1+F2 |
| Pipeline re-run cost after stage failure | 100% (full re-run) | Stage cost only (F4A cache) |
| Tokens wasted on transient API errors | Full prompt per failure | 0 (F3 retries transparently) |
| Context utilization for `-f dir/` | 100% of files (wasteful) | Budget-optimized (F5) |
| Pre-flight error prevention | 0 errors caught | All capability mismatches caught (F6) |

---

## Relationship to Existing Roadmap

| Epic 2 Feature | Existing Phase | Relationship |
|---|---|---|
| F1 (response_format) | None | **New** — no existing plan for native structured output |
| F2 (schema retry) | Phase 8 "per-record retry logic" was explicitly killed | **Reversal with narrow scope** — retry at schema level, not record level. The Phase 8 kill was about `--each` per-record retry; this is about schema validation retry within a single invocation. Different concern. |
| F3 (API retry) | Phase 5A notes "Connection retry/backoff handled by rmcp's ExponentialBackoff" for MCP only | **New for LLM API calls** — MCP has retry via rmcp, but LLM API calls have zero retry |
| F4A (stage cache) | Phase 8 killed "--resume / checkpoint in --each" | **Different scope** — Phase 8 kill was about resumable batch processing. This is about caching individual stage outputs for re-use across invocations. |
| F4B/C (stage retry + fallback) | Phase 7C has retry budget for tools only | **Extension** — Phase 7C's retry budget applies within `call_react` for tool calls. This extends retry to the pipeline stage level and adds model fallback. |
| F5 (context budget) | None | **New** — no existing plan for context window allocation |
| F6 (pre-flight) | Phase 7B has pre-flight for tool binaries only | **Extension** — Phase 7B checks if tool binaries exist. This checks model capabilities before API calls. |
