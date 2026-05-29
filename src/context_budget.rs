//! Phase 11: Context budget allocation and BM25-ranked file selection.
//!
//! Two closely-related responsibilities live here:
//!
//! - **11A: `ContextBudget`** — a tiny pure helper that tracks a model's input
//!   window and the output reservation, and computes the tokens remaining
//!   for caller-managed content after fixed allocations (system prompt,
//!   user query, schema suffix).
//!
//! - **11B: `rank_files` + `select_within_budget`** — when `-f` names multiple
//!   files alongside a query, rank files by BM25 relevance against the query
//!   and greedily pack the highest-scoring subset into the budget. Files that
//!   don't fit are logged to stderr, not included in the prompt.
//!
//! Intentionally *not* here: knowledge-store or RAG retrieval. Phase 11C
//! (budget-aware RAG) is superseded by Phase 26A, which will consume the
//! same budget plumbing against the compiled knowledge store.

use crate::utils::estimate_token_length;

/// Default reservation for the model's output tokens when the role doesn't
/// specify one. Chosen as a reasonable upper bound for typical LLM replies.
pub const DEFAULT_OUTPUT_RESERVE: usize = 4096;

/// Safety margin subtracted from the files budget to absorb unknown fixed
/// allocations (system prompt, user query prefix, schema suffix, retry
/// feedback, tool schemas) that we don't want to measure precisely here.
pub const FILES_SAFETY_MARGIN: usize = 2048;

pub struct ContextBudget {
    pub total_budget: usize,
    pub output_reserve: usize,
}

impl ContextBudget {
    pub fn new(total_budget: usize, output_reserve: usize) -> Self {
        Self {
            total_budget,
            output_reserve,
        }
    }

    /// Tokens remaining after the output reserve and the caller-provided fixed
    /// allocations are subtracted. Saturating — never underflows.
    pub fn remaining(&self, fixed_allocations: usize) -> usize {
        self.total_budget
            .saturating_sub(self.output_reserve)
            .saturating_sub(fixed_allocations)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedContent {
    pub path: String,
    pub content: String,
    pub relevance_score: f64,
    pub token_estimate: usize,
}

#[derive(Debug, Default)]
pub struct SelectionOutcome {
    pub selected: Vec<RankedContent>,
    pub skipped: Vec<RankedContent>,
}

impl SelectionOutcome {
    pub fn total_selected_tokens(&self) -> usize {
        self.selected.iter().map(|r| r.token_estimate).sum()
    }
}

/// Rank `files` by BM25 relevance against `query`. Files are returned sorted
/// descending by score. Zero-or-one file or an empty query short-circuits to
/// input order with zero scores (no ranking work, no selection churn).
pub fn rank_files(files: Vec<(String, String)>, query: &str) -> Vec<RankedContent> {
    let trimmed_query = query.trim();
    if files.len() <= 1 || trimmed_query.is_empty() {
        return files
            .into_iter()
            .map(|(path, content)| {
                let token_estimate = estimate_token_length(&content);
                RankedContent {
                    path,
                    content,
                    relevance_score: 0.0,
                    token_estimate,
                }
            })
            .collect();
    }

    use bm25::{Language, SearchEngineBuilder};
    let docs: Vec<bm25::Document<u32>> = files
        .iter()
        .enumerate()
        .map(|(i, (_, content))| bm25::Document::new(i as u32, content.as_str()))
        .collect();
    let engine = SearchEngineBuilder::<u32>::with_documents(Language::English, docs)
        .k1(1.5)
        .b(0.75)
        .build();
    let results = engine.search(trimmed_query, files.len());

    let mut scores: std::collections::HashMap<u32, f64> = std::collections::HashMap::new();
    for r in results {
        scores.insert(r.document.id, r.score as f64);
    }

    let mut ranked: Vec<RankedContent> = files
        .into_iter()
        .enumerate()
        .map(|(i, (path, content))| {
            let token_estimate = estimate_token_length(&content);
            RankedContent {
                path,
                content,
                relevance_score: scores.get(&(i as u32)).copied().unwrap_or(0.0),
                token_estimate,
            }
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

/// Greedy best-fit selection: walk ranked files top-down and include each one
/// whose tokens fit in the remaining budget. Skipped files are recorded for
/// stderr reporting. Files are not partially included — we cut at file
/// boundaries so the LLM never sees a truncated-mid-function mess.
pub fn select_within_budget(ranked: Vec<RankedContent>, budget: usize) -> SelectionOutcome {
    let mut selected = Vec::new();
    let mut skipped = Vec::new();
    let mut used: usize = 0;
    for item in ranked {
        if used.saturating_add(item.token_estimate) <= budget {
            used += item.token_estimate;
            selected.push(item);
        } else {
            skipped.push(item);
        }
    }
    SelectionOutcome { selected, skipped }
}

/// Phase 11D: allocate a pipeline's total dollar budget across N stages by
/// `budget_weight`. Stages without an explicit weight default to 1.0. A zero
/// or negative weight is treated as the default — a stage that runs always
/// gets at least its share, never silently zero. With an empty `weights`
/// slice or a zero `total_usd`, returns an all-zeros vector of matching
/// length (the no-op case).
pub fn allocate_stage_budgets(weights: &[Option<f64>], total_usd: f64) -> Vec<f64> {
    let effective: Vec<f64> = weights
        .iter()
        .map(|w| match w {
            Some(v) if *v > 0.0 => *v,
            _ => 1.0,
        })
        .collect();
    let total_weight: f64 = effective.iter().sum();
    if total_weight <= 0.0 || total_usd <= 0.0 {
        return vec![0.0; weights.len()];
    }
    effective
        .iter()
        .map(|w| (w / total_weight) * total_usd)
        .collect()
}

/// Phase 11D: turn a per-stage dollar budget into a maximum input-token cap.
/// Subtracts the cost of the reserved output tokens first; whatever dollars
/// remain are converted to input tokens at the model's input price.
///
/// Prices are in `$/Mtok` (matching the `model.input_price` / `output_price`
/// units already used by `compute_cost`). A zero `input_price_per_million`
/// means the input is free; return `usize::MAX` so the caller treats the
/// budget as "no cap" rather than dividing by zero.
pub fn budget_usd_to_input_token_cap(
    budget_usd: f64,
    input_price_per_million: f64,
    output_reserve_tokens: usize,
    output_price_per_million: f64,
) -> usize {
    if input_price_per_million <= 0.0 {
        return usize::MAX;
    }
    let output_cost = (output_reserve_tokens as f64) * output_price_per_million / 1_000_000.0;
    let input_dollars = (budget_usd - output_cost).max(0.0);
    let tokens = input_dollars * 1_000_000.0 / input_price_per_million;
    if !tokens.is_finite() || tokens <= 0.0 {
        return 0;
    }
    tokens.floor() as usize
}

/// Phase 11D: shrink `text` until `estimate_token_length` reports a value at
/// or below `token_cap`. Truncation is from the tail by character to keep the
/// head of the input (typically the user's instruction / latest message)
/// intact. Returns the trimmed text and a `was_truncated` signal so the
/// caller can emit a warning rather than silently dropping content.
pub fn truncate_to_token_budget(text: &str, token_cap: usize) -> (String, bool) {
    if estimate_token_length(text) <= token_cap {
        return (text.to_string(), false);
    }
    if token_cap == 0 {
        return (String::new(), true);
    }
    // Binary-search over the char-prefix length until the estimate fits.
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        let candidate: String = chars[..mid].iter().collect();
        if estimate_token_length(&candidate) <= token_cap {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let trimmed: String = chars[..lo].iter().collect();
    (trimmed, true)
}

/// Format a short multi-line summary of a selection outcome for stderr.
/// Returns `None` when nothing was skipped — no noise when the budget is fine.
pub fn format_selection_summary(outcome: &SelectionOutcome) -> Option<String> {
    if outcome.skipped.is_empty() {
        return None;
    }
    let total_files = outcome.selected.len() + outcome.skipped.len();
    let mut out = format!(
        "Context budget: included {}/{} files ({} tokens); skipped {} by BM25 rank",
        outcome.selected.len(),
        total_files,
        outcome.total_selected_tokens(),
        outcome.skipped.len()
    );
    for r in &outcome.selected {
        out.push_str(&format!(
            "\n  include {:>7.2}  {:>6}t  {}",
            r.relevance_score, r.token_estimate, r.path
        ));
    }
    for r in &outcome.skipped {
        out.push_str(&format!(
            "\n  skip    {:>7.2}  {:>6}t  {}",
            r.relevance_score, r.token_estimate, r.path
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Phase 11A: ContextBudget ----

    #[test]
    fn remaining_subtracts_reserve_and_fixed() {
        let b = ContextBudget::new(100_000, 4096);
        assert_eq!(b.remaining(2000), 100_000 - 4096 - 2000);
    }

    #[test]
    fn remaining_saturates_to_zero_when_overspent() {
        let b = ContextBudget::new(1000, 4096);
        assert_eq!(b.remaining(500), 0, "reserve alone exceeds total → 0");

        let b = ContextBudget::new(5000, 4096);
        assert_eq!(b.remaining(10_000), 0, "fixed alone exceeds remainder → 0");
    }

    #[test]
    fn default_output_reserve_is_sensible_for_modern_models() {
        assert_eq!(DEFAULT_OUTPUT_RESERVE, 4096);
    }

    // ---- Phase 11B: rank_files ----

    #[test]
    fn rank_files_single_file_passthrough() {
        let files = vec![("a.md".into(), "hello world".into())];
        let ranked = rank_files(files, "anything");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].path, "a.md");
        assert_eq!(ranked[0].relevance_score, 0.0);
        assert!(ranked[0].token_estimate > 0);
    }

    #[test]
    fn rank_files_empty_query_passthrough() {
        let files = vec![
            ("a.md".into(), "first".into()),
            ("b.md".into(), "second".into()),
        ];
        let ranked = rank_files(files, "   ");
        assert_eq!(ranked.len(), 2);
        // order preserved when query is empty
        assert_eq!(ranked[0].path, "a.md");
        assert_eq!(ranked[1].path, "b.md");
    }

    #[test]
    fn rank_files_orders_by_bm25_score() {
        let files = vec![
            (
                "unrelated.md".into(),
                "The quick brown fox jumps over the lazy dog. \
                 Completely unrelated content about animals and pets."
                    .into(),
            ),
            (
                "relevant.md".into(),
                "Retrieval augmented generation patterns. \
                 Chunking and embedding strategies for retrieval pipelines. \
                 Retrieval quality and retrieval latency."
                    .into(),
            ),
        ];
        let ranked = rank_files(files, "retrieval strategies");
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked[0].path, "relevant.md",
            "BM25 should rank the retrieval file above the unrelated one"
        );
        assert!(ranked[0].relevance_score > ranked[1].relevance_score);
    }

    // ---- Phase 11B: select_within_budget ----

    #[test]
    fn select_fits_everything_when_budget_allows() {
        let ranked = vec![
            RankedContent {
                path: "a".into(),
                content: "x".into(),
                relevance_score: 1.0,
                token_estimate: 100,
            },
            RankedContent {
                path: "b".into(),
                content: "y".into(),
                relevance_score: 0.5,
                token_estimate: 200,
            },
        ];
        let outcome = select_within_budget(ranked, 500);
        assert_eq!(outcome.selected.len(), 2);
        assert!(outcome.skipped.is_empty());
        assert_eq!(outcome.total_selected_tokens(), 300);
    }

    #[test]
    fn select_skips_files_that_dont_fit() {
        let ranked = vec![
            RankedContent {
                path: "hi".into(),
                content: "x".into(),
                relevance_score: 1.0,
                token_estimate: 600,
            },
            RankedContent {
                path: "lo".into(),
                content: "y".into(),
                relevance_score: 0.5,
                token_estimate: 200,
            },
        ];
        let outcome = select_within_budget(ranked, 500);
        assert_eq!(outcome.selected.len(), 1, "only the small file fits");
        assert_eq!(outcome.selected[0].path, "lo");
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].path, "hi");
    }

    #[test]
    fn select_cuts_at_file_boundaries() {
        // The greedy packer never slices a file open — either the whole thing
        // is in or the whole thing is skipped. This matters for downstream
        // token budgets: partial files produce malformed code/markdown.
        let ranked = vec![
            RankedContent {
                path: "big".into(),
                content: "x".into(),
                relevance_score: 1.0,
                token_estimate: 400,
            },
            RankedContent {
                path: "tiny".into(),
                content: "y".into(),
                relevance_score: 0.1,
                token_estimate: 50,
            },
        ];
        // Budget=401: big fits, tiny doesn't even though it's small and budget
        // would *technically* allow cutting big to 350 + keeping tiny.
        let outcome = select_within_budget(ranked, 401);
        assert_eq!(outcome.selected.len(), 1);
        assert_eq!(outcome.selected[0].path, "big");
        assert_eq!(outcome.skipped.len(), 1);
    }

    #[test]
    fn select_greedy_prefers_higher_ranked_over_packing_efficiency() {
        // When input is sorted desc by score, we pack in order — never skip
        // a high-rank file to squeeze two lower-rank files into the budget.
        // This is a feature: relevance beats density.
        let ranked = vec![
            RankedContent {
                path: "top".into(),
                content: "x".into(),
                relevance_score: 10.0,
                token_estimate: 400,
            },
            RankedContent {
                path: "mid".into(),
                content: "y".into(),
                relevance_score: 5.0,
                token_estimate: 300,
            },
            RankedContent {
                path: "low".into(),
                content: "z".into(),
                relevance_score: 1.0,
                token_estimate: 100,
            },
        ];
        // Budget=500: top fits (400). mid doesn't (400+300>500). low does fit
        // at 400+100=500 so it's selected after mid is skipped.
        let outcome = select_within_budget(ranked, 500);
        assert_eq!(outcome.selected.len(), 2);
        assert_eq!(outcome.selected[0].path, "top");
        assert_eq!(outcome.selected[1].path, "low");
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].path, "mid");
    }

    // ---- Phase 11D: allocate_stage_budgets ----

    #[test]
    fn allocate_equal_share_when_no_weights() {
        let weights = vec![None, None, None, None];
        let shares = allocate_stage_budgets(&weights, 0.40);
        assert_eq!(shares.len(), 4);
        for s in &shares {
            assert!((s - 0.10).abs() < 1e-9, "each stage gets 1/4: {s}");
        }
    }

    #[test]
    fn allocate_proportional_to_weights() {
        // 1 + 2 + 1 = 4 total weight; shares: 1/4, 2/4, 1/4
        let weights = vec![None, Some(2.0), Some(1.0)];
        let shares = allocate_stage_budgets(&weights, 1.0);
        assert!((shares[0] - 0.25).abs() < 1e-9);
        assert!((shares[1] - 0.50).abs() < 1e-9);
        assert!((shares[2] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn allocate_empty_returns_empty() {
        let shares = allocate_stage_budgets(&[], 5.00);
        assert!(shares.is_empty());
    }

    #[test]
    fn allocate_zero_total_returns_all_zeros() {
        let weights = vec![Some(1.0), Some(2.0)];
        let shares = allocate_stage_budgets(&weights, 0.0);
        assert_eq!(shares, vec![0.0, 0.0]);
    }

    #[test]
    fn allocate_treats_zero_or_negative_weight_as_default() {
        // A user-set weight of 0 or negative is nonsensical for a stage that
        // is actually going to run; fall back to the implicit 1.0 default
        // rather than silently zeroing the stage's budget.
        let weights = vec![Some(0.0), Some(-1.0), Some(2.0)];
        let shares = allocate_stage_budgets(&weights, 4.0);
        // Effective weights: 1.0, 1.0, 2.0 → total 4.0
        assert!((shares[0] - 1.0).abs() < 1e-9);
        assert!((shares[1] - 1.0).abs() < 1e-9);
        assert!((shares[2] - 2.0).abs() < 1e-9);
    }

    // ---- Phase 11D: budget_usd_to_input_token_cap ----

    #[test]
    fn budget_to_tokens_subtracts_output_reservation_cost_first() {
        // $0.01 budget; input $1/Mtok, output $2/Mtok; reserve 1000 output tokens.
        // Output reservation cost = 1000 * 2 / 1_000_000 = $0.002
        // Remaining for input = $0.008
        // Input tokens = 0.008 * 1_000_000 / 1 = 8000
        let cap = budget_usd_to_input_token_cap(0.01, 1.0, 1000, 2.0);
        assert_eq!(cap, 8000);
    }

    #[test]
    fn budget_to_tokens_saturates_to_zero_when_reserve_exceeds_budget() {
        // Output reservation alone costs more than the budget → zero input tokens.
        // Reserve 1000 * $2/Mtok = $0.002; budget $0.001 → 0.
        let cap = budget_usd_to_input_token_cap(0.001, 1.0, 1000, 2.0);
        assert_eq!(cap, 0);
    }

    #[test]
    fn budget_to_tokens_returns_usize_max_when_input_price_is_zero() {
        // A free input model has no token cap — return usize::MAX as the
        // "unconstrained" sentinel so the caller treats the budget as a no-op.
        let cap = budget_usd_to_input_token_cap(0.05, 0.0, 1000, 2.0);
        assert_eq!(cap, usize::MAX);
    }

    // ---- Phase 11D: truncate_to_token_budget ----

    #[test]
    fn truncate_short_text_is_passthrough() {
        let text = "hello world";
        let (out, was_truncated) = truncate_to_token_budget(text, 100);
        assert_eq!(out, text);
        assert!(!was_truncated);
    }

    #[test]
    fn truncate_long_text_shrinks_to_fit_and_signals() {
        // Build a long enough text that the estimate clearly exceeds the cap.
        let text = "word ".repeat(1000); // ~1300 tokens by estimate
        let (out, was_truncated) = truncate_to_token_budget(&text, 100);
        assert!(was_truncated, "long text under tiny cap should truncate");
        assert!(out.len() < text.len());
        assert!(estimate_token_length(&out) <= 100);
    }

    #[test]
    fn truncate_zero_budget_yields_empty_and_signals() {
        let (out, was_truncated) = truncate_to_token_budget("any text at all", 0);
        assert_eq!(out, "");
        assert!(was_truncated);
    }

    // ---- format_selection_summary ----

    #[test]
    fn summary_returns_none_when_nothing_skipped() {
        let outcome = SelectionOutcome {
            selected: vec![RankedContent {
                path: "a".into(),
                content: "x".into(),
                relevance_score: 1.0,
                token_estimate: 10,
            }],
            skipped: vec![],
        };
        assert!(format_selection_summary(&outcome).is_none());
    }

    #[test]
    fn summary_lists_included_and_skipped_files() {
        let outcome = SelectionOutcome {
            selected: vec![RankedContent {
                path: "a.md".into(),
                content: "x".into(),
                relevance_score: 3.2,
                token_estimate: 100,
            }],
            skipped: vec![RankedContent {
                path: "b.md".into(),
                content: "y".into(),
                relevance_score: 0.4,
                token_estimate: 500,
            }],
        };
        let s = format_selection_summary(&outcome).expect("summary present");
        assert!(s.contains("1/2 files"));
        assert!(s.contains("include"));
        assert!(s.contains("a.md"));
        assert!(s.contains("skip"));
        assert!(s.contains("b.md"));
    }
}
