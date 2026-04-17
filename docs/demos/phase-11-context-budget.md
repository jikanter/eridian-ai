# Phase 11: Context Budget + BM25 File Ranking

*2026-04-17T03:03:30Z by Showboat 0.6.1*
<!-- showboat-id: 7524d040-85ba-4209-ac17-826b44a9aa51 -->

Phase 11 ships two pieces together in `src/context_budget.rs`:

- **11A (ContextBudget)** — tracks `max_input_tokens - output_reserve - fixed_allocations`, saturating so nothing underflows. Consumed by 11B and, later, by Phase 26A.
- **11B (BM25 file ranking)** — when `-f` loads multiple files alongside a query, rank files by BM25 relevance, then greedily pack highest-ranked files into the remaining budget. Skipped files are logged to stderr. Cuts at file boundaries — no mid-file slicing.

**11C (budget-aware RAG) was superseded.** Phase 25 deprecates `src/rag/`; Phase 26A consumes this budget directly. See `docs/roadmap/phase-11-context-budget.md`.

## Module API

```bash
grep -nE "^pub (fn|struct|const) " src/context_budget.rs
```

```output
23:pub const DEFAULT_OUTPUT_RESERVE: usize = 4096;
28:pub const FILES_SAFETY_MARGIN: usize = 2048;
30:pub struct ContextBudget {
53:pub struct RankedContent {
61:pub struct SelectionOutcome {
75:pub fn rank_files(files: Vec<(String, String)>, query: &str) -> Vec<RankedContent> {
134:pub fn select_within_budget(ranked: Vec<RankedContent>, budget: usize) -> SelectionOutcome {
151:pub fn format_selection_summary(outcome: &SelectionOutcome) -> Option<String> {
```

## Unit tests — budget math, BM25 ordering, greedy selection

```bash
cargo test --bin aichat -- context_budget::tests 2>&1 | grep -E "^test context_budget|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test context_budget::tests::default_output_reserve_is_sensible_for_modern_models ... ok
test context_budget::tests::rank_files_empty_query_passthrough ... ok
test context_budget::tests::rank_files_orders_by_bm25_score ... ok
test context_budget::tests::rank_files_single_file_passthrough ... ok
test context_budget::tests::remaining_saturates_to_zero_when_overspent ... ok
test context_budget::tests::remaining_subtracts_reserve_and_fixed ... ok
test context_budget::tests::select_cuts_at_file_boundaries ... ok
test context_budget::tests::select_fits_everything_when_budget_allows ... ok
test context_budget::tests::select_greedy_prefers_higher_ranked_over_packing_efficiency ... ok
test context_budget::tests::select_skips_files_that_dont_fit ... ok
test context_budget::tests::summary_lists_included_and_skipped_files ... ok
test context_budget::tests::summary_returns_none_when_nothing_skipped ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 253 filtered out; finished in Xs
```

## Integration — `Input::from_files`

```bash
grep -n "Phase 11\|maybe_select_by_budget\|ContextBudget\|rank_files\|select_within_budget" src/config/input.rs
```

```output
103:        // Phase 11A/B: when multiple files were loaded and the user supplied a
108:        let documents = maybe_select_by_budget(documents, raw_text, &role);
486:/// Phase 11A/B integration glue. Given the raw `-f` document list, decide
492:fn maybe_select_by_budget(
498:        format_selection_summary, rank_files, select_within_budget, ContextBudget,
510:    let budget = ContextBudget::new(max_input, DEFAULT_OUTPUT_RESERVE);
534:    let ranked = rank_files(files, raw_text);
535:    let outcome = select_within_budget(ranked, files_budget);
```

Kicks in only when **all** of these hold: (a) `-f` loaded multiple files, (b) a non-empty query is present, (c) the role's model advertises `max_input_tokens`, (d) the concatenated token estimate exceeds the files budget (`max_input - 4096 output reserve - 2048 safety margin`). Otherwise the existing concatenate-all path is preserved — no behavior change for the small-input common case.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 265 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
