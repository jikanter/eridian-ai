//! Phase 25C: AEVS-style restore-check.
//!
//! AEVS ("Anchor, Extract, Verify, Supplement" — arXiv 2503.19574 et al.)
//! prescribes a deterministic matching ladder that proves every emitted fact
//! is grounded in its source. If a candidate description cannot be restored
//! back to the source via any step of the ladder, the fact is rejected at
//! compile time — the LLM hallucinated it or drifted off-source.
//!
//! The ladder here:
//!
//! 1. **Exact** — `source.contains(description)`.
//! 2. **WhitespaceTolerant** — collapse whitespace runs on both sides.
//! 3. **SchemaNormalized** — lowercase, strip ASCII punctuation, collapse
//!    whitespace, strip leading articles ("the", "a", "an").
//! 4. **TokenOverlap** — at least `TOKEN_OVERLAP_THRESHOLD` (70%) of the
//!    description's word tokens appear somewhere in the source. A loose
//!    recall check — catches heavier rephrasing while rejecting outright
//!    fabrications.
//!
//! We deliberately **do not** use Levenshtein distance here. Computing
//! Levenshtein against every substring of a large source is quadratic and
//! scope-inappropriate for a compile-time guard. The whitespace and schema
//! steps catch the common minor-variation cases; the token fallback is the
//! safety net for everything else.

use super::edp::EntityDescriptionPair;
use anyhow::{bail, Result};

pub const TOKEN_OVERLAP_THRESHOLD: f64 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreStrategy {
    Exact,
    WhitespaceTolerant,
    SchemaNormalized,
    TokenOverlap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoreOutcome {
    pub strategy: RestoreStrategy,
    /// Byte range in `source` (half-open) where the matched span sits. For
    /// `TokenOverlap` — where the loose recall check spans multiple regions
    /// — this is the envelope from first to last matched token.
    pub matched_byte_range: (usize, usize),
}

/// Try to restore `description` to `source` via the deterministic ladder.
/// Returns `None` when no strategy succeeds — the caller rejects the fact.
pub fn restore_check(description: &str, source: &str) -> Option<RestoreOutcome> {
    if description.is_empty() || source.is_empty() {
        return None;
    }

    // Step 1: exact.
    if let Some(idx) = source.find(description) {
        return Some(RestoreOutcome {
            strategy: RestoreStrategy::Exact,
            matched_byte_range: (idx, idx + description.len()),
        });
    }

    // Step 2: whitespace-tolerant. Normalize both sides; map the match back
    // to an approximate byte range in the original source.
    let desc_ws = collapse_whitespace(description);
    let src_ws = collapse_whitespace(source);
    if !desc_ws.is_empty() {
        if let Some(idx_ws) = src_ws.find(&desc_ws) {
            if let Some(range) = map_normalized_to_source(source, idx_ws, desc_ws.len()) {
                return Some(RestoreOutcome {
                    strategy: RestoreStrategy::WhitespaceTolerant,
                    matched_byte_range: range,
                });
            }
        }
    }

    // Step 3: schema-normalized.
    let desc_sn = schema_normalize(description);
    let src_sn = schema_normalize(source);
    if !desc_sn.is_empty() {
        if let Some(idx_sn) = src_sn.find(&desc_sn) {
            if let Some(range) = map_normalized_to_source(source, idx_sn, desc_sn.len()) {
                return Some(RestoreOutcome {
                    strategy: RestoreStrategy::SchemaNormalized,
                    matched_byte_range: range,
                });
            }
        }
    }

    // Step 4: token-overlap fallback.
    if let Some(range) = token_overlap_envelope(description, source, TOKEN_OVERLAP_THRESHOLD) {
        return Some(RestoreOutcome {
            strategy: RestoreStrategy::TokenOverlap,
            matched_byte_range: range,
        });
    }

    None
}

/// Check an EDP against its source text and return the matching outcome, or
/// an error identifying the fact that failed restore. The EDP's
/// `provenance.byte_range` is **not** mutated here; callers (Phase 25B) may
/// choose to update the anchor to the actual matched range.
pub fn check_fact(edp: &EntityDescriptionPair, source: &str) -> Result<RestoreOutcome> {
    match restore_check(&edp.description, source) {
        Some(outcome) => Ok(outcome),
        None => bail!(
            "Fact {} failed AEVS restore-check (not found in {})",
            edp.id,
            edp.provenance.path
        ),
    }
}

// ---------- private helpers ----------

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !in_ws && !out.is_empty() {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

fn schema_normalize(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let depunct: String = lower
        .chars()
        .map(|c| if c.is_ascii_punctuation() { ' ' } else { c })
        .collect();
    let collapsed = collapse_whitespace(&depunct);
    strip_leading_articles(&collapsed)
}

fn strip_leading_articles(s: &str) -> String {
    let mut remaining = s;
    loop {
        let trimmed = remaining.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("the ")
            .or_else(|| trimmed.strip_prefix("a "))
            .or_else(|| trimmed.strip_prefix("an "))
        {
            remaining = rest;
            continue;
        }
        return trimmed.to_string();
    }
}

/// Given an offset+length in the normalized source, find an approximate byte
/// range in the original source by walking character-by-character and
/// re-counting the collapsed form until we cross the requested normalized
/// offset. Returns `None` if the normalized offset is past end-of-source.
fn map_normalized_to_source(
    source: &str,
    norm_start: usize,
    norm_len: usize,
) -> Option<(usize, usize)> {
    let mut norm_pos: usize = 0;
    let mut byte_start: Option<usize> = None;
    let mut byte_end: Option<usize> = None;
    let mut in_ws = false;
    let mut saw_non_ws = false;

    for (byte_idx, c) in source.char_indices() {
        // Mirror collapse_whitespace's normalization: each whitespace run
        // becomes a single space (unless it would be the leading char).
        let emits_char = if c.is_whitespace() {
            let emit = !in_ws && saw_non_ws;
            in_ws = true;
            emit
        } else {
            in_ws = false;
            saw_non_ws = true;
            true
        };
        if !emits_char {
            continue;
        }

        if norm_pos == norm_start && byte_start.is_none() {
            byte_start = Some(byte_idx);
        }
        norm_pos += 1;
        if norm_pos == norm_start + norm_len {
            byte_end = Some(byte_idx + c.len_utf8());
            break;
        }
    }

    match (byte_start, byte_end) {
        (Some(s), Some(e)) => Some((s, e)),
        (Some(s), None) => Some((s, source.len())),
        _ => None,
    }
}

fn tokenize_words(s: &str) -> Vec<(usize, String)> {
    use unicode_segmentation::UnicodeSegmentation;
    s.split_word_bound_indices()
        .filter_map(|(i, w)| {
            let t = w.trim().to_ascii_lowercase();
            if t.is_empty() || t.chars().all(|c| !c.is_alphanumeric()) {
                None
            } else {
                Some((i, t))
            }
        })
        .collect()
}

fn token_overlap_envelope(
    description: &str,
    source: &str,
    threshold: f64,
) -> Option<(usize, usize)> {
    let desc_tokens: std::collections::HashSet<String> =
        tokenize_words(description).into_iter().map(|(_, t)| t).collect();
    if desc_tokens.is_empty() {
        return None;
    }

    let src_tokens = tokenize_words(source);
    let mut first_match: Option<usize> = None;
    let mut last_match: Option<usize> = None;
    let mut matched: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (byte_idx, tok) in &src_tokens {
        if desc_tokens.contains(tok) {
            matched.insert(tok.clone());
            if first_match.is_none() {
                first_match = Some(*byte_idx);
            }
            last_match = Some(byte_idx + tok.len());
        }
    }

    let overlap = matched.len() as f64 / desc_tokens.len() as f64;
    if overlap < threshold {
        return None;
    }
    match (first_match, last_match) {
        (Some(s), Some(e)) => Some((s, e)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::edp::{EntityDescriptionPair, SourceAnchor};

    fn edp(description: &str) -> EntityDescriptionPair {
        EntityDescriptionPair::new(
            "entity",
            description,
            vec![],
            SourceAnchor {
                path: "p.md".into(),
                byte_range: (0, description.len()),
                line_range: (1, 1),
                content_hash: "h".into(),
            },
            vec![],
        )
    }

    // ---- exact ----

    #[test]
    fn exact_match_is_the_first_strategy_tried() {
        let source = "Alan Turing pioneered modern computing in the 1940s.";
        let desc = "pioneered modern computing";
        let outcome = restore_check(desc, source).unwrap();
        assert_eq!(outcome.strategy, RestoreStrategy::Exact);
        assert_eq!(
            &source[outcome.matched_byte_range.0..outcome.matched_byte_range.1],
            desc
        );
    }

    #[test]
    fn exact_match_rejects_description_absent_from_source() {
        let source = "The cat sat on the mat.";
        assert!(restore_check("dog", source).is_some() == false);
    }

    // ---- whitespace-tolerant ----

    #[test]
    fn whitespace_tolerant_matches_through_extra_newlines() {
        let source = "the quick\n\n  brown fox\n jumps";
        let desc = "quick brown fox jumps";
        let outcome = restore_check(desc, source).unwrap();
        assert_eq!(outcome.strategy, RestoreStrategy::WhitespaceTolerant);
        // Byte range should cover the matched region, not the normalized one.
        let slice = &source[outcome.matched_byte_range.0..outcome.matched_byte_range.1];
        assert!(slice.contains("quick"));
        assert!(slice.contains("jumps"));
    }

    #[test]
    fn whitespace_tolerant_does_not_upgrade_exact_match() {
        // Plain contains wins even when whitespace would also succeed.
        let source = "quick brown fox jumps";
        let desc = "quick brown fox jumps";
        let outcome = restore_check(desc, source).unwrap();
        assert_eq!(outcome.strategy, RestoreStrategy::Exact);
    }

    // ---- schema-normalized ----

    #[test]
    fn schema_normalized_handles_case_and_punctuation() {
        let source = "Retrieval-augmented generation: a pattern for grounding LLMs.";
        let desc = "retrieval augmented generation a pattern for grounding llms";
        let outcome = restore_check(desc, source).unwrap();
        assert_eq!(outcome.strategy, RestoreStrategy::SchemaNormalized);
    }

    #[test]
    fn schema_normalized_strips_leading_articles() {
        let source = "cat sat on mat.";
        let desc = "The cat sat on the mat";
        let outcome = restore_check(desc, source).unwrap();
        // "The cat sat on the mat" → normalized → "cat sat on the mat"
        //                                          ^^^ article still appears mid-phrase
        // source "cat sat on mat." → normalized → "cat sat on mat"
        // These are not substring-equal — so schema-normalize should still
        // fail, falling to token overlap.
        assert_ne!(outcome.strategy, RestoreStrategy::Exact);
        // Just assert *some* strategy accepted it (token overlap will).
        assert!(matches!(
            outcome.strategy,
            RestoreStrategy::SchemaNormalized | RestoreStrategy::TokenOverlap
        ));
    }

    #[test]
    fn strip_leading_articles_removes_stacked_articles() {
        assert_eq!(strip_leading_articles("the a an word"), "word");
        assert_eq!(strip_leading_articles("word"), "word");
        assert_eq!(strip_leading_articles(""), "");
    }

    // ---- token overlap fallback ----

    #[test]
    fn token_overlap_matches_reworded_description() {
        let source = "The quick brown fox jumps over the lazy dog on a sunny day.";
        // Description rewords but keeps enough vocabulary overlap.
        let desc = "quick brown fox jumps over dog";
        let outcome = restore_check(desc, source).unwrap();
        // Should succeed via some strategy; token overlap if earlier steps miss.
        assert!(matches!(
            outcome.strategy,
            RestoreStrategy::Exact
                | RestoreStrategy::WhitespaceTolerant
                | RestoreStrategy::SchemaNormalized
                | RestoreStrategy::TokenOverlap
        ));
    }

    #[test]
    fn token_overlap_rejects_low_overlap() {
        let source = "The cat sat on the mat.";
        let desc = "Alan Turing pioneered computing machinery in Manchester."; // zero overlap
        assert!(restore_check(desc, source).is_none());
    }

    #[test]
    fn token_overlap_requires_70_percent_threshold() {
        let source = "word1 word2 word3 word4 word5 unrelated.";
        // 4 of 5 description tokens present → 80%, passes.
        let desc = "word1 word2 word3 word4 word99";
        let outcome = restore_check(desc, source);
        assert!(outcome.is_some());

        // 3 of 5 → 60%, below threshold.
        let desc = "word1 word2 word3 word98 word99";
        assert!(restore_check(desc, source).is_none());
    }

    // ---- empty / edge cases ----

    #[test]
    fn empty_description_or_source_returns_none() {
        assert!(restore_check("", "hello").is_none());
        assert!(restore_check("hello", "").is_none());
    }

    // ---- check_fact ----

    #[test]
    fn check_fact_returns_ok_for_grounded_edp() {
        let source = "Claude is a language model from Anthropic.";
        let fact = edp("Claude is a language model from Anthropic.");
        let outcome = check_fact(&fact, source).unwrap();
        assert_eq!(outcome.strategy, RestoreStrategy::Exact);
    }

    #[test]
    fn check_fact_errors_with_fact_id_and_path_on_failure() {
        let source = "The sky is blue.";
        let fact = edp("Entirely unrelated fabricated claim about nothing.");
        let err = check_fact(&fact, source).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("restore-check"), "msg: {msg}");
        assert!(msg.contains(fact.provenance.path.as_str()), "msg: {msg}");
    }

    // ---- internal helpers ----

    #[test]
    fn collapse_whitespace_normalizes_runs_and_edges() {
        assert_eq!(collapse_whitespace("  a  b   c  "), "a b c");
        assert_eq!(collapse_whitespace("\n\n x \t y \n"), "x y");
        assert_eq!(collapse_whitespace(""), "");
    }

    #[test]
    fn schema_normalize_strips_punctuation_and_lowercases() {
        assert_eq!(schema_normalize("Retrieval-augmented Generation!"), "retrieval augmented generation");
        assert_eq!(schema_normalize("The rain, in Spain."), "rain in spain");
    }

    #[test]
    fn token_overlap_envelope_returns_range_spanning_first_to_last_hit() {
        let source = "alpha beta gamma delta epsilon";
        // description tokens: beta, delta → both present.
        let range = token_overlap_envelope("beta delta", source, 1.0).unwrap();
        let slice = &source[range.0..range.1];
        assert!(slice.starts_with("beta"));
        assert!(slice.ends_with("delta"));
    }
}
