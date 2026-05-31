# Phase 11: Context Budget & Budget Propagation : Overview - Epic 2

*Merges existing Phase 11 with pipeline-level budget propagation.*

| Item | Description | Status |
|---|---|---|
| 11A | Context budget allocator core (`src/context_budget.rs`) | -- |
| 11B | BM25-ranked file inclusion (score files against query, fill budget by relevance) | -- |
| 11C | Budget-aware RAG (dynamic `top_k = remaining_budget / avg_chunk_tokens`) | -- |
| 11D | Pipeline budget propagation (`pipeline_budget_usd:` + per-stage `budget_weight:`, allocation + tail-truncation) | Done |

**11D Design — Pipeline Budget Propagation (shipped):**

Role frontmatter (or a pipe-def file) declares the total dollar budget; per-stage `budget_weight:` divides the share proportionally. Default weight is 1.0; zero/negative weights are treated as the default rather than starving a stage.

```yaml
# Role frontmatter form
pipeline_budget_usd: 0.05
pipeline:
  - role: extract          # gets proportional share (weight 1.0)
  - role: review
    budget_weight: 2.0     # gets 2× share
  - role: format
```

```yaml
# Pipe-def file form (--pipe-def)
budget_usd: 0.05
stages:
  - role: extract
  - role: review
    budget_weight: 2.0
  - role: format
```

**Implementation:** `run()`, `invoke_role`, and `invoke_role_streaming` all allocate per-top-level-node budgets via `context_budget::allocate_stage_budgets` and stamp the share onto `PipelineStage.budget_usd`. `run_stage_inner` converts the dollar share into an input-token cap via `budget_usd_to_input_token_cap` (model input price + `DEFAULT_OUTPUT_RESERVE`) and tail-truncates the post-knowledge input via `truncate_to_token_budget`. Truncation emits a stderr warning rather than failing — losing the bottom of a long context is recoverable; a refused run isn't.

**Scope guardrails (deferred):**
- Nested DAG nodes (`parallel:`, `switch:` arms) receive `None` and consume their model's native window. DAG-aware sub-allocation is a follow-up.
- `--pipe-def` is the only file-form entry point; the HTTP `/v1/pipelines/run` path (`run_inline_pipeline`) passes `None` per-stage.
- CLI `--stage` form has no budget surface yet.

**Files:** `src/context_budget.rs` (`allocate_stage_budgets`, `budget_usd_to_input_token_cap`, `truncate_to_token_budget`), `src/pipe.rs` (per-stage allocation in `run()`/`invoke_role`/`invoke_role_streaming`, truncation in `run_stage_inner`), `src/config/role.rs` (`pipeline_budget_usd`, `RolePipelineStage::budget_weight`).

## [Epic Details](./phase-11-context-budget.md)
