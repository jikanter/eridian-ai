# Phase 11: Runtime Intelligence — Context Budget

**Status:** 11A/11B Done; 11C Superseded; 11D Done (2026-05-28)
**Epic:** 2 — Runtime Intelligence
**Design:** [epic-2.md](../analysis/epic-2.md)

---

| Item | Status | Notes |
|---|---|---|
| 11A. Context budget allocator core | Done | New `src/context_budget.rs`. Calculates: `remaining = max_input_tokens - output_reserve - fixed_allocations`. `ContextBudget::new` + `.remaining()` (saturating). Defaults: 4096 output reserve, 2048 safety margin for fixed allocations. |
| 11B. BM25-ranked file inclusion | Done | Same module. `rank_files` + `select_within_budget` + `format_selection_summary`. Wired into `Input::from_files` — kicks in only when `-f` loads multiple files, a query is present, and the total would exceed the files budget. Skipped files logged to stderr; cuts at file boundaries (never slices mid-file). |
| 11C. Budget-aware RAG | Superseded | **Not shipping.** The legacy `src/rag/` module is slated for deprecation when Phase 25A (Knowledge Compilation) lands, so widening `Rag::search()` would be throwaway work. The budget plumbing from 11A is instead consumed directly by [Phase 26A](./phase-26-knowledge-query.md) (tag-filter + BM25 query core), which is budget-aware from day one. |
| 11D. Pipeline budget propagation | Done | `pipeline_budget_usd:` on a role pipeline (or `budget_usd:` in a `--pipe-def` file) plus optional `budget_weight:` per stage (default 1.0). `run()`, `invoke_role`, and `invoke_role_streaming` allocate via `context_budget::allocate_stage_budgets` and stamp the share onto each `PipelineStage`. `run_stage_inner` converts the dollar share into an input-token cap via `budget_usd_to_input_token_cap` and tail-truncates post-knowledge input via `truncate_to_token_budget` with a stderr warning. Nested DAG nodes, `--stage` CLI form, and the HTTP `/v1/pipelines/run` endpoint propagate `None` for now. Files: `src/context_budget.rs`, `src/pipe.rs`, `src/config/role.rs`. |

**Parallelization:** 11A and 11B shipped together as one module (`src/context_budget.rs`); 11B consumes the `ContextBudget` helper from 11A.

**Config:**
```yaml
context_budget:
  output_reserve: 4096     # tokens reserved for output
  file_strategy: bm25      # bm25 | truncate | all (default: bm25 when >1 file)
  warn_on_truncation: true  # emit warning to stderr when content is truncated
```

**Key files:** new `src/context_budget.rs` (11A), `src/config/input.rs` (11A integration), `src/config/mod.rs` (11C, config parsing), `src/client/model.rs` (11A soft-fail guard), `src/rag/mod.rs` (11C).

**What to kill:**
| Proposal | Reason |
|---|---|
| Token-exact counting (tiktoken) | `tiktoken-rs` only covers OpenAI tokenizers. Heuristic (~1.3 tokens/word) is sufficient for budget allocation — order-of-magnitude correctness, not precision. |
| LiteLLM integration as dependency | Python runtime conflicts with single-binary. AIChat already targets LiteLLM proxy via `openai-compatible` with zero code changes. |
| Automatic model selection (`model: auto`) | Requires quality benchmarks per model per task. Manual selection + fallback chains (10D) is more reliable. |
| Prompt caching API integration | Provider-specific (Anthropic cache_control, OpenAI implicit). Separate epic for provider-specific optimizations. |
