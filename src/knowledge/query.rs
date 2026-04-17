//! Phase 26A: Tag-filter + BM25 query core.
//!
//! The retrieval pipeline is deterministic end-to-end:
//!
//! ```text
//! tag predicates? ──AND filter──▶ candidate set
//!                                       │
//!                                       ▼
//!                              BM25 over descriptions
//!                                       │
//!                                       ▼
//!                          top-K hits, budget-capped
//! ```
//!
//! Tag predicates narrow the candidate set *before* BM25 ranks — cutting the
//! per-query work proportionally and giving users a precise structural knob
//! that vector similarity cannot express.

use bm25::{Language, SearchEngineBuilder};

use crate::context_budget::DEFAULT_OUTPUT_RESERVE;
use crate::utils::estimate_token_length;

use super::edp::{EntityDescriptionPair, FactId};
use super::store::KnowledgeStore;
use super::tags::Tag;

pub const DEFAULT_TOP_K: usize = 8;

/// One retrieval result: the fact itself plus the BM25 score and the source
/// path from its provenance (hoisted here so consumers don't need to walk
/// into `fact.provenance` for the common case).
#[derive(Debug, Clone)]
pub struct FactHit {
    pub fact: EntityDescriptionPair,
    pub score: f32,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct QueryOptions {
    /// Maximum number of hits to return. `None` → `DEFAULT_TOP_K`.
    pub top_k: Option<usize>,
    /// AND-joined tag predicates. An EDP passes the filter when every
    /// predicate matches one of its tags.
    pub tags: Vec<Tag>,
    /// Token budget for the combined hit set. When set, hits are appended in
    /// rank order until adding another would exceed this budget; remaining
    /// hits are dropped (no mid-fact slicing).
    pub token_budget: Option<usize>,
    /// Phase 27A: when true, deprecated facts are returned alongside live
    /// ones. Default retrieval filters them out so callers consuming the
    /// KB don't see superseded content.
    pub include_deprecated: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            top_k: None,
            tags: Vec::new(),
            token_budget: None,
            include_deprecated: false,
        }
    }
}

/// Filter facts by AND-joined tag predicates. Empty predicate list → all
/// facts pass. Each fact passes when, for every predicate `p`, the fact
/// carries a matching `(namespace, value)` tag.
pub fn filter_by_tags<'a>(
    facts: &'a [EntityDescriptionPair],
    predicates: &[Tag],
) -> Vec<&'a EntityDescriptionPair> {
    if predicates.is_empty() {
        return facts.iter().collect();
    }
    facts
        .iter()
        .filter(|fact| {
            predicates.iter().all(|pred| {
                fact.tags
                    .iter()
                    .any(|t| t.namespace == pred.namespace && t.value == pred.value)
            })
        })
        .collect()
}

/// Rank a candidate set by BM25 over fact descriptions. Returns up to `top_k`
/// `FactHit`s in descending score order. Candidates that BM25 did not score
/// (i.e. no query-term overlap) are excluded — BM25's filter behavior.
pub fn bm25_rank(
    candidates: &[&EntityDescriptionPair],
    query: &str,
    top_k: usize,
) -> Vec<FactHit> {
    if candidates.is_empty() || query.trim().is_empty() || top_k == 0 {
        return Vec::new();
    }

    let docs: Vec<bm25::Document<u32>> = candidates
        .iter()
        .enumerate()
        .map(|(i, f)| bm25::Document::new(i as u32, f.description.as_str()))
        .collect();
    let engine = SearchEngineBuilder::<u32>::with_documents(Language::English, docs)
        .k1(1.5)
        .b(0.75)
        .build();
    let results = engine.search(query.trim(), top_k);

    results
        .into_iter()
        .filter_map(|r| {
            let idx = r.document.id as usize;
            candidates.get(idx).map(|fact| FactHit {
                fact: (*fact).clone(),
                score: r.score,
                source: fact.provenance.path.clone(),
            })
        })
        .collect()
}

/// Apply a token budget to an ordered hit list. Walks top-down; includes
/// each hit whose token estimate fits in the remaining budget, and drops
/// the rest. Cuts at fact boundaries — an EDP is either included whole or
/// skipped (same discipline as Phase 11B file selection).
pub fn apply_budget(hits: Vec<FactHit>, budget: usize) -> Vec<FactHit> {
    let mut out = Vec::with_capacity(hits.len());
    let mut used: usize = 0;
    for hit in hits {
        let tokens = estimate_token_length(&hit.fact.description);
        if used.saturating_add(tokens) <= budget {
            used += tokens;
            out.push(hit);
        }
    }
    out
}

/// Full query pipeline: tag filter → BM25 rank → budget truncation.
/// Phase 27A: deprecated facts are filtered out unless
/// `opts.include_deprecated` is set.
pub fn query(store: &KnowledgeStore, text: &str, opts: &QueryOptions) -> Vec<FactHit> {
    let candidates: Vec<&EntityDescriptionPair> = if opts.include_deprecated {
        store.facts.iter().collect()
    } else {
        store.facts.iter().filter(|f| !f.deprecated).collect()
    };
    let filtered: Vec<&EntityDescriptionPair> = if opts.tags.is_empty() {
        candidates
    } else {
        candidates
            .into_iter()
            .filter(|fact| {
                opts.tags.iter().all(|pred| {
                    fact.tags
                        .iter()
                        .any(|t| t.namespace == pred.namespace && t.value == pred.value)
                })
            })
            .collect()
    };
    let top_k = opts.top_k.unwrap_or(DEFAULT_TOP_K);
    let ranked = bm25_rank(&filtered, text, top_k);
    match opts.token_budget {
        Some(budget) => apply_budget(ranked, budget),
        None => ranked,
    }
}

/// Convenience: compute a reasonable default `token_budget` for the given
/// model. Mirrors Phase 11A's `ContextBudget::remaining` with a safety margin
/// that leaves room for system prompt + user message.
pub fn default_budget_for(max_input_tokens: Option<usize>) -> Option<usize> {
    max_input_tokens.map(|m| {
        m.saturating_sub(DEFAULT_OUTPUT_RESERVE)
            .saturating_sub(crate::context_budget::FILES_SAFETY_MARGIN)
    })
}

/// Format a flat list of fact hits as injected context. Each fact gets a
/// header line with its id + tags so attribution survives the round-trip.
pub fn format_hits_for_injection(hits: &[FactHit]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n## Retrieved knowledge\n");
    for h in hits {
        out.push_str(&format!("\n[[{}]]", h.fact.id));
        if !h.fact.tags.is_empty() {
            let tag_strs: Vec<String> = h.fact.tags.iter().map(|t| t.to_string()).collect();
            out.push_str(&format!(" ({})", tag_strs.join(", ")));
        }
        out.push_str(&format!("\n{}\n", h.fact.description));
    }
    out
}

/// Phase 27D: variant of `format_hits_for_injection` that instructs the
/// LLM to carry `[[fact-id]]` markers through into its response so the
/// driver can expand them into a provenance table post-hoc. No extra LLM
/// call — marker placement is deterministic, expansion is a text walk
/// against the hit list the user saw.
pub fn format_hits_for_attributed_injection(hits: &[FactHit]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\n## Retrieved knowledge (with citations)\n\nEach fact below is prefixed with a `[[fact-id]]` marker. When you use a fact in your response, keep the `[[fact-id]]` marker next to the claim it supports. Unused facts don't need markers.\n",
    );
    for h in hits {
        out.push_str(&format!("\n[[{}]]", h.fact.id));
        if !h.fact.tags.is_empty() {
            let tag_strs: Vec<String> = h.fact.tags.iter().map(|t| t.to_string()).collect();
            out.push_str(&format!(" ({})", tag_strs.join(", ")));
        }
        out.push_str(&format!("\n{}\n", h.fact.description));
    }
    out
}

/// Phase 27D: post-process an LLM response that carries `[[fact-id]]`
/// citation markers. Finds every marker that matches a retrieved fact,
/// keeps the markers in the body (they're useful inline), and appends a
/// deterministic provenance table listing each cited fact's source path
/// and line range. Markers that don't match any retrieved fact are left
/// untouched — the driver doesn't guess.
pub fn annotate_output_with_provenance(output: &str, hits: &[FactHit]) -> String {
    if hits.is_empty() {
        return output.to_string();
    }
    // Build a lookup from id → (source path, line range).
    let mut lookup: indexmap::IndexMap<String, &FactHit> = indexmap::IndexMap::new();
    for h in hits {
        lookup.insert(h.fact.id.to_string(), h);
    }
    // Walk the output, collect in-order unique ids actually mentioned.
    let mut cited_ids: Vec<String> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = output[search_from..].find("[[") {
        let start = search_from + rel + 2;
        if let Some(end_rel) = output[start..].find("]]") {
            let end = start + end_rel;
            let id = output[start..end].trim().to_string();
            if lookup.contains_key(&id) && !cited_ids.contains(&id) {
                cited_ids.push(id);
            }
            search_from = end + 2;
        } else {
            break;
        }
    }
    if cited_ids.is_empty() {
        return output.to_string();
    }
    let mut table = String::from("\n\n---\nSources:\n");
    for id in &cited_ids {
        let hit = lookup[id];
        table.push_str(&format!(
            "- [[{}]] {} lines {}–{}\n",
            id,
            hit.fact.provenance.path,
            hit.fact.provenance.line_range.0,
            hit.fact.provenance.line_range.1,
        ));
    }
    format!("{}{}", output.trim_end(), table)
}

/// Same as `format_hits_for_injection` but emits each hit as a JSON line —
/// consumed by `--knowledge-search -o json` (Phase 26E).
pub fn hits_to_json(hits: &[FactHit]) -> serde_json::Value {
    let items: Vec<serde_json::Value> = hits
        .iter()
        .map(|h| {
            serde_json::json!({
                "id": h.fact.id,
                "entity": h.fact.entity,
                "description": h.fact.description,
                "tags": h.fact.tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
                "score": h.score,
                "source": h.source,
                "line_range": [h.fact.provenance.line_range.0, h.fact.provenance.line_range.1],
            })
        })
        .collect();
    serde_json::Value::Array(items)
}

/// Collect FactIds from a hit slice — used by the graph-walk caller to
/// feed seed ids into expansion (Phase 26B).
pub fn hit_ids(hits: &[FactHit]) -> Vec<FactId> {
    hits.iter().map(|h| h.fact.id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::edp::{EntityDescriptionPair, SourceAnchor};
    use crate::knowledge::store::KnowledgeStore;
    use tempfile::tempdir;

    fn fact(
        entity: &str,
        description: &str,
        tags: Vec<Tag>,
        path: &str,
    ) -> EntityDescriptionPair {
        EntityDescriptionPair::new(
            entity,
            description,
            tags,
            SourceAnchor {
                path: path.into(),
                byte_range: (0, description.len()),
                line_range: (1, 1),
                content_hash: "h".into(),
            },
            vec![],
        )
    }

    fn store_with_facts(facts: Vec<EntityDescriptionPair>) -> KnowledgeStore {
        let dir = tempdir().unwrap().keep();
        let path = dir.join("kb");
        let mut store = KnowledgeStore::create(&path, "test-kb").unwrap();
        for f in facts {
            store.append_fact(f).unwrap();
        }
        store
    }

    // ---- tag filter ----

    #[test]
    fn filter_by_empty_predicates_passes_all() {
        let facts = vec![
            fact("a", "hello", vec![Tag::new("kind", "rule")], "p.md"),
            fact("b", "world", vec![], "p.md"),
        ];
        let out = filter_by_tags(&facts, &[]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn filter_by_tags_requires_all_predicates() {
        let facts = vec![
            fact(
                "a",
                "one",
                vec![Tag::new("kind", "rule"), Tag::new("topic", "retrieval")],
                "p.md",
            ),
            fact(
                "b",
                "two",
                vec![Tag::new("kind", "rule")], // missing topic
                "p.md",
            ),
            fact(
                "c",
                "three",
                vec![Tag::new("topic", "retrieval")], // missing kind
                "p.md",
            ),
        ];
        let predicates = vec![Tag::new("kind", "rule"), Tag::new("topic", "retrieval")];
        let out = filter_by_tags(&facts, &predicates);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entity, "a");
    }

    #[test]
    fn filter_by_tags_rejects_unknown_predicate() {
        let facts = vec![fact(
            "a",
            "x",
            vec![Tag::new("kind", "rule")],
            "p.md",
        )];
        let out = filter_by_tags(&facts, &[Tag::new("kind", "unknown")]);
        assert!(out.is_empty());
    }

    // ---- bm25 rank ----

    #[test]
    fn bm25_rank_orders_by_relevance() {
        let facts = vec![
            fact(
                "unrelated",
                "The cat sat on the mat and enjoyed a nap.",
                vec![],
                "a.md",
            ),
            fact(
                "relevant",
                "Retrieval augmented generation grounds responses in retrieved context.",
                vec![],
                "b.md",
            ),
        ];
        let cands: Vec<&_> = facts.iter().collect();
        let hits = bm25_rank(&cands, "retrieval generation", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].fact.entity, "relevant");
    }

    #[test]
    fn bm25_rank_empty_inputs_return_nothing() {
        let facts = vec![fact("a", "hello", vec![], "p.md")];
        let cands: Vec<&_> = facts.iter().collect();
        assert!(bm25_rank(&[], "q", 5).is_empty());
        assert!(bm25_rank(&cands, "", 5).is_empty());
        assert!(bm25_rank(&cands, "   ", 5).is_empty());
        assert!(bm25_rank(&cands, "q", 0).is_empty());
    }

    #[test]
    fn bm25_rank_respects_top_k() {
        let facts: Vec<_> = (0..10)
            .map(|i| {
                fact(
                    &format!("e{i}"),
                    &format!("retrieval fact number {i} about retrieval"),
                    vec![],
                    "p.md",
                )
            })
            .collect();
        let cands: Vec<&_> = facts.iter().collect();
        let hits = bm25_rank(&cands, "retrieval", 3);
        assert_eq!(hits.len(), 3);
    }

    // ---- budget ----

    #[test]
    fn apply_budget_includes_facts_until_budget_exhausted() {
        let f1 = fact("a", "short fact", vec![], "p.md");
        let f2 = fact(
            "b",
            "a very long description that uses many more tokens to say something",
            vec![],
            "p.md",
        );
        let hits = vec![
            FactHit {
                fact: f1.clone(),
                score: 2.0,
                source: "p.md".into(),
            },
            FactHit {
                fact: f2.clone(),
                score: 1.5,
                source: "p.md".into(),
            },
        ];
        let tokens_f1 = estimate_token_length(&f1.description);
        // Budget fits only the first fact.
        let out = apply_budget(hits.clone(), tokens_f1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].fact.entity, "a");
        // Budget fits both.
        let out = apply_budget(hits, 10_000);
        assert_eq!(out.len(), 2);
    }

    // ---- end-to-end ----

    #[test]
    fn query_end_to_end_filters_then_ranks() {
        let store = store_with_facts(vec![
            fact(
                "Mismatched-topic",
                "Retrieval fact in other topic",
                vec![Tag::new("kind", "rule"), Tag::new("topic", "other")],
                "a.md",
            ),
            fact(
                "Correct-topic-matched",
                "Retrieval augmented generation uses retrieval.",
                vec![Tag::new("kind", "rule"), Tag::new("topic", "retrieval")],
                "b.md",
            ),
            fact(
                "Missing-kind",
                "Retrieval also great fact",
                vec![Tag::new("topic", "retrieval")],
                "c.md",
            ),
        ]);
        let opts = QueryOptions {
            top_k: Some(5),
            tags: vec![Tag::new("kind", "rule"), Tag::new("topic", "retrieval")],
            token_budget: None,
            include_deprecated: false,
        };
        let hits = query(&store, "retrieval", &opts);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fact.entity, "Correct-topic-matched");
    }

    #[test]
    fn query_without_tag_filter_searches_all() {
        let store = store_with_facts(vec![
            fact("a", "alpha beta gamma", vec![], "p.md"),
            fact("b", "delta epsilon zeta", vec![], "p.md"),
        ]);
        let hits = query(&store, "alpha", &QueryOptions::default());
        assert!(!hits.is_empty());
        assert_eq!(hits[0].fact.entity, "a");
    }

    // ---- formatting ----

    #[test]
    fn format_hits_for_injection_embeds_fact_ids() {
        let f = fact(
            "x",
            "desc",
            vec![Tag::new("kind", "rule")],
            "p.md",
        );
        let hits = vec![FactHit {
            fact: f.clone(),
            score: 1.0,
            source: "p.md".into(),
        }];
        let s = format_hits_for_injection(&hits);
        assert!(s.contains("[[fact-"));
        assert!(s.contains("(kind:rule)"));
        assert!(s.contains("desc"));
    }

    #[test]
    fn format_hits_empty_returns_empty_string() {
        assert_eq!(format_hits_for_injection(&[]), "");
    }

    #[test]
    fn hits_to_json_has_expected_fields() {
        let f = fact("x", "desc", vec![Tag::new("kind", "rule")], "p.md");
        let hits = vec![FactHit {
            fact: f.clone(),
            score: 2.5,
            source: "p.md".into(),
        }];
        let v = hits_to_json(&hits);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["entity"], "x");
        assert_eq!(arr[0]["description"], "desc");
        assert_eq!(arr[0]["tags"][0], "kind:rule");
        assert_eq!(arr[0]["score"], 2.5);
        assert_eq!(arr[0]["source"], "p.md");
    }

    #[test]
    fn query_excludes_deprecated_facts_by_default() {
        let mut store = store_with_facts(vec![
            fact("live", "alpha beta gamma", vec![], "p.md"),
            fact("dead", "alpha beta gamma", vec![], "q.md"),
        ]);
        let dead_id = store
            .facts
            .iter()
            .find(|f| f.entity == "dead")
            .map(|f| f.id.clone())
            .unwrap();
        store.deprecate_fact(&dead_id, None).unwrap();

        let hits = query(&store, "alpha", &QueryOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fact.entity, "live");

        let with_deprecated = query(
            &store,
            "alpha",
            &QueryOptions {
                include_deprecated: true,
                ..QueryOptions::default()
            },
        );
        assert_eq!(with_deprecated.len(), 2);
    }

    // ---- Phase 27D: attributed output ----

    #[test]
    fn format_hits_for_attributed_injection_instructs_on_markers() {
        let f = fact("x", "desc", vec![Tag::new("kind", "rule")], "p.md");
        let hits = vec![FactHit {
            fact: f.clone(),
            score: 1.0,
            source: "p.md".into(),
        }];
        let s = format_hits_for_attributed_injection(&hits);
        assert!(s.contains("keep the `[[fact-id]]` marker"));
        assert!(s.contains("[[fact-"));
        assert!(s.contains("desc"));
    }

    #[test]
    fn annotate_output_appends_sources_table_for_cited_ids_only() {
        let f1 = fact("one", "alpha", vec![], "a.md");
        let f2 = fact("two", "beta", vec![], "b.md");
        let id1 = f1.id.clone();
        let id2 = f2.id.clone();
        let hits = vec![
            FactHit {
                fact: f1,
                score: 1.0,
                source: "a.md".into(),
            },
            FactHit {
                fact: f2,
                score: 1.0,
                source: "b.md".into(),
            },
        ];
        // LLM output mentions only the first fact.
        let output = format!("Claim backed by [[{id1}]].");
        let annotated = annotate_output_with_provenance(&output, &hits);
        assert!(annotated.contains("---\nSources:"));
        assert!(annotated.contains(&format!("[[{id1}]]")));
        assert!(
            !annotated.contains(&format!("[[{id2}]] b.md")),
            "uncited fact must not appear in the sources table"
        );
    }

    #[test]
    fn annotate_output_is_noop_when_no_markers_match() {
        let f = fact("x", "desc", vec![], "p.md");
        let hits = vec![FactHit {
            fact: f.clone(),
            score: 1.0,
            source: "p.md".into(),
        }];
        let output = "Claim without citation.";
        let annotated = annotate_output_with_provenance(output, &hits);
        assert_eq!(annotated, output);
    }

    #[test]
    fn annotate_output_deduplicates_repeat_citations() {
        let f = fact("x", "desc", vec![], "p.md");
        let id = f.id.clone();
        let hits = vec![FactHit {
            fact: f,
            score: 1.0,
            source: "p.md".into(),
        }];
        let output = format!("First [[{id}]] then again [[{id}]] done.");
        let annotated = annotate_output_with_provenance(&output, &hits);
        // Only one sources line per unique id.
        let count = annotated.matches(&format!("[[{id}]] p.md")).count();
        assert_eq!(count, 1);
    }

    // ---- end Phase 27D ----

    #[test]
    fn default_budget_for_subtracts_reserve_and_safety() {
        assert_eq!(default_budget_for(None), None);
        let b = default_budget_for(Some(100_000)).unwrap();
        assert_eq!(b, 100_000 - DEFAULT_OUTPUT_RESERVE - crate::context_budget::FILES_SAFETY_MARGIN);
    }
}
