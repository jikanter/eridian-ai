# Phase 10: Resilience & Cost-Aware Routing : Overview - Epic 2

*Merges existing Phase 10 resilience with Theme 3 (cost-aware routing).*

**Status (2026-05-11):** All four resilience items shipped (10A–10D). Cost-aware model routing (`model_policy:`) appeared in earlier drafts of this overview as "10D" but was **never scoped into the shipped Phase 10**; the implementing work reused the 10D letter for pipeline model fallback. See [`docs/analysis/epic-2.md`](../../analysis/epic-2.md) note at top: *"Cost-aware model routing was discussed as a possible Phase 10 expansion but was not scoped into the shipped Phase 10; it remains future work."* The shipped letter mapping is authoritative in [`phase-10-resilience.md`](./phase-10-resilience.md).

The decision to drop `model_policy:` is recorded in [`phase-11-context-budget.md`](./phase-11-context-budget.md) "What NOT to build": automatic model selection requires per-task quality benchmarks the project lacks, so manual `fallback_models:` chains were judged more reliable.

| Item | Description | Status |
| --- | --- | --- |
| 10A | API-level retry with exponential backoff (`src/client/retry.rs`) | **Done** |
| 10B | Pipeline stage output cache (content-addressable, `sha256(role+model+input)`, configurable TTL) | **Done** (`src/cache.rs`) |
| 10C | Pipeline stage retry (configurable `stage_retries:`, retryable error classification) | **Done** |
| 10D | Pipeline model fallback (`fallback_models:` chain on stage failure) | **Done** |
| ~~Cost-aware routing (`model_policy:`)~~ | Proposed Phase 10 expansion. Design retained below for future reference only. | **Ruled out of scope** — rationale in [`phase-11-context-budget.md`](./phase-11-context-budget.md) "What to kill" table (manual `fallback_models:` chains preferred over automatic routing absent per-task quality benchmarks) |

**10D Design — Cost-Aware Model Routing:**

Static `model:` fields leave massive savings on the table. A `model_policy:` field enables deterministic routing without an LLM call for the routing decision:

```yaml
# Deterministic routing by input characteristics
model_policy:
  default: deepseek:deepseek-chat
  rules:
    - when: { token_count_gt: 2000 }
      model: claude:claude-sonnet-4-6
    - when: { schema_failures_gt: 1 }
      model: openai:gpt-4o
  fallback: openai:gpt-4o
```

**Implementation:** In `Input::create_client()`, before model resolution, evaluate `model_policy.rules` against the input. Rules are deterministic predicates — `token_count_gt`, `has_images`, `has_tools`, `schema_failures_gt` — evaluated via `estimate_token_length()` and input metadata. No LLM call needed.

For `--each` batch processing, this alone can cut costs 40-60% on mixed-complexity workloads by routing simple inputs to cheap models.

**Files:** `src/config/role.rs` (add `model_policy`), `src/config/input.rs` (evaluate rules in `create_client()`), `src/config/mod.rs` (parse policy config).

## [Epic Details](./phase-10-resilience.md)
