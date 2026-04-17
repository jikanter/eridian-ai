//! Phase 26D+26F: Orchestrator that turns a role's `KnowledgeBinding` list
//! into a ranked set of `FactHit`s ready for injection.
//!
//! Pipeline per binding:
//!
//! 1. Load the KB from disk (`<config>/kb/<name>/`).
//! 2. Phase 26A: tag-filter → BM25 → top-K seeds.
//! 3. Phase 26B: expand seeds along the 1-hop graph, RRF-fuse.
//!
//! Across bindings (Phase 26F):
//!
//! 4. Reciprocal Rank Fusion of each binding's result list, weighted by the
//!    binding's `weight` field. Reuses the `reciprocal_rank_fusion` helper
//!    from `graph.rs` — rank-based fusion is safe across KBs regardless of
//!    how each one scored its own results.
//! 5. Reify the fused id list into `FactHit`s (picking the highest-score
//!    representative when the same fact appears in multiple KB results).
//! 6. Optional Phase 11A-style token budget truncation.

use anyhow::Result;

use crate::config::KnowledgeBinding;
use crate::utils::trace::{KnowledgeQueryEvent, TraceEmitter};

use super::cli::kb_dir;
use super::graph::{expand_and_fuse, reciprocal_rank_fusion};
use super::query::{apply_budget, query, FactHit, QueryOptions, DEFAULT_TOP_K};
use super::store::KnowledgeStore;
use super::tags::Tag;

#[derive(Debug, Clone, Default)]
pub struct RetrievalOptions {
    /// Upper bound on the final fused hit count. `None` → `DEFAULT_TOP_K`.
    pub top_k: Option<usize>,
    /// Combined token budget across all bindings. `None` → unbounded.
    pub token_budget: Option<usize>,
    /// When true, run graph-walk expansion (Phase 26B) on each binding's
    /// seed set before fusion. When false, only the seed set survives.
    pub graph_expand: bool,
    /// Phase 27A: surface deprecated facts alongside live ones. Off by
    /// default — retrieval pretends deprecated facts don't exist.
    pub include_deprecated: bool,
}

impl RetrievalOptions {
    pub fn new_for_injection(token_budget: Option<usize>) -> Self {
        Self {
            top_k: None,
            token_budget,
            graph_expand: true,
            include_deprecated: false,
        }
    }
}

/// Retrieve facts across one or more knowledge bindings. Returns an empty
/// vec when `bindings` is empty. Errors propagate from store load or
/// filesystem issues; per-binding retrieval errors surface as `Err`.
pub fn retrieve_from_bindings(
    bindings: &[KnowledgeBinding],
    query_text: &str,
    opts: &RetrievalOptions,
) -> Result<Vec<FactHit>> {
    retrieve_from_bindings_traced(bindings, query_text, opts, None).map(|(hits, _)| hits)
}

/// Phase 27C: variant that threads a `TraceEmitter` and returns the
/// per-binding events alongside the hits. Callers that want both sides
/// (main.rs for `--trace`, REPL for `.sources knowledge`) invoke this.
pub fn retrieve_from_bindings_traced(
    bindings: &[KnowledgeBinding],
    query_text: &str,
    opts: &RetrievalOptions,
    trace_emitter: Option<&TraceEmitter>,
) -> Result<(Vec<FactHit>, Vec<KnowledgeQueryEvent>)> {
    if bindings.is_empty() || query_text.trim().is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let top_k = opts.top_k.unwrap_or(DEFAULT_TOP_K);

    // Retrieve per-binding. Track each binding's ranked id list (for RRF)
    // and a shared map from id → best FactHit (for reification after fusion).
    let mut per_binding_ranked: Vec<Vec<super::edp::FactId>> = Vec::with_capacity(bindings.len());
    let mut weights: Vec<f64> = Vec::with_capacity(bindings.len());
    let mut best_hit: std::collections::HashMap<super::edp::FactId, FactHit> =
        std::collections::HashMap::new();
    let mut events: Vec<KnowledgeQueryEvent> = Vec::with_capacity(bindings.len());

    for binding in bindings {
        let dir = kb_dir(&binding.name);
        let store = KnowledgeStore::load(&dir)
            .map_err(|e| anyhow::anyhow!("Failed to load KB '{}': {e:#}", binding.name))?;

        let tags: Vec<Tag> = binding
            .tags
            .iter()
            .filter_map(|s| Tag::parse(s).ok())
            .collect();
        let tag_filter_strings: Vec<String> =
            tags.iter().map(|t| t.to_string()).collect();

        // Count live (or with-deprecated) candidates up front for the trace.
        let candidate_count = if opts.include_deprecated {
            store.facts.len()
        } else {
            store.facts.iter().filter(|f| !f.deprecated).count()
        };

        let seed_hits = query(
            &store,
            query_text,
            &QueryOptions {
                top_k: Some(top_k),
                tags,
                token_budget: None,
                include_deprecated: opts.include_deprecated,
            },
        );
        let seed_ids: Vec<String> =
            seed_hits.iter().map(|h| h.fact.id.to_string()).collect();

        let hits = if opts.graph_expand {
            expand_and_fuse(&store, seed_hits.clone(), top_k)
        } else {
            seed_hits.clone()
        };
        let expanded_ids: Vec<String> = hits
            .iter()
            .map(|h| h.fact.id.to_string())
            .filter(|id| !seed_ids.contains(id))
            .collect();

        let ranked_ids: Vec<_> = hits.iter().map(|h| h.fact.id.clone()).collect();
        per_binding_ranked.push(ranked_ids);
        weights.push(binding.weight as f64);

        let final_ids: Vec<String> = hits.iter().map(|h| h.fact.id.to_string()).collect();
        let final_scores: Vec<f32> = hits.iter().map(|h| h.score).collect();

        let event = KnowledgeQueryEvent {
            kb: binding.name.clone(),
            query: query_text.to_string(),
            tag_filter: tag_filter_strings,
            candidate_count,
            seed_ids,
            expanded_ids,
            final_ids,
            final_scores,
        };
        if let Some(emitter) = trace_emitter {
            emitter.emit_knowledge_query(&event);
        }
        events.push(event);

        for hit in hits {
            best_hit
                .entry(hit.fact.id.clone())
                .and_modify(|existing| {
                    if hit.score > existing.score {
                        *existing = hit.clone();
                    }
                })
                .or_insert(hit);
        }
    }

    // Single-binding fast path: no fusion needed, but still dedupe via the
    // hit map and preserve the single binding's order.
    let ranked_slice: Vec<&[super::edp::FactId]> =
        per_binding_ranked.iter().map(|v| v.as_slice()).collect();
    let fused = reciprocal_rank_fusion(&ranked_slice, &weights, top_k);

    let mut hits: Vec<FactHit> = fused
        .into_iter()
        .filter_map(|(id, _score)| best_hit.get(&id).cloned())
        .collect();

    if let Some(budget) = opts.token_budget {
        hits = apply_budget(hits, budget);
    }
    Ok((hits, events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::cli::kb_root;
    use crate::knowledge::edp::{EntityDescriptionPair, SourceAnchor};
    use crate::knowledge::tags::Tag as KbTag;

    fn fact(entity: &str, description: &str, path: &str) -> EntityDescriptionPair {
        EntityDescriptionPair::new(
            entity,
            description,
            vec![KbTag::new("kind", "fact")],
            SourceAnchor {
                path: path.into(),
                byte_range: (0, description.len()),
                line_range: (1, 1),
                content_hash: "h".into(),
            },
            vec![],
        )
    }

    /// Create a KB under the live `kb_root()` so `retrieve_from_bindings`
    /// (which looks up via `kb_dir(name)`) can find it. The test must be
    /// serial-safe; we use unique names per test.
    fn seed_kb(name: &str, facts: Vec<EntityDescriptionPair>) -> std::path::PathBuf {
        let dir = kb_root().join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).ok();
        }
        let mut store = KnowledgeStore::create(&dir, name).unwrap();
        for f in facts {
            store.append_fact(f).unwrap();
        }
        store.save().unwrap();
        dir
    }

    #[test]
    fn retrieve_empty_bindings_returns_empty() {
        let hits = retrieve_from_bindings(&[], "query", &RetrievalOptions::default()).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn retrieve_empty_query_returns_empty() {
        let b = KnowledgeBinding::simple("doesnt-matter");
        let hits =
            retrieve_from_bindings(&[b], "   ", &RetrievalOptions::default()).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn retrieve_errors_on_missing_kb() {
        let b = KnowledgeBinding::simple("nonexistent-kb-zzz-99999");
        let err =
            retrieve_from_bindings(&[b], "query", &RetrievalOptions::default()).unwrap_err();
        assert!(err.to_string().contains("Failed to load KB"));
    }

    #[test]
    fn retrieve_single_binding_returns_ranked_hits() {
        let name = "test-retrieve-single";
        let dir = seed_kb(
            name,
            vec![
                fact("unrelated", "The cat sat on the mat peacefully.", "a.md"),
                fact(
                    "relevant",
                    "Retrieval augmented generation grounds output.",
                    "b.md",
                ),
            ],
        );

        let hits = retrieve_from_bindings(
            &[KnowledgeBinding::simple(name)],
            "retrieval generation",
            &RetrievalOptions::default(),
        )
        .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].fact.entity, "relevant");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn retrieve_multi_binding_fuses_across_kbs() {
        let kb_a = "test-retrieve-fuse-a";
        let kb_b = "test-retrieve-fuse-b";
        let dir_a = seed_kb(
            kb_a,
            vec![fact(
                "common",
                "retrieval augmented generation mentioned here",
                "a.md",
            )],
        );
        let dir_b = seed_kb(
            kb_b,
            vec![fact(
                "common",
                "retrieval augmented generation mentioned here",
                "a.md",
            )],
        );

        let hits = retrieve_from_bindings(
            &[
                KnowledgeBinding::simple(kb_a),
                KnowledgeBinding::simple(kb_b),
            ],
            "retrieval",
            &RetrievalOptions::default(),
        )
        .unwrap();
        // Identical fact in both KBs → same FactId → single fused entry.
        assert_eq!(hits.len(), 1);

        std::fs::remove_dir_all(dir_a).ok();
        std::fs::remove_dir_all(dir_b).ok();
    }

    #[test]
    fn retrieve_traced_returns_event_per_binding() {
        let name = "test-retrieve-traced-single";
        let dir = seed_kb(
            name,
            vec![fact(
                "r",
                "retrieval augmented generation grounds output",
                "a.md",
            )],
        );
        let (hits, events) = retrieve_from_bindings_traced(
            &[KnowledgeBinding::simple(name)],
            "retrieval",
            &RetrievalOptions::default(),
            None,
        )
        .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kb, name);
        assert!(!events[0].final_ids.is_empty());
        assert_eq!(events[0].final_scores.len(), events[0].final_ids.len());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn retrieve_token_budget_truncates_at_boundary() {
        let name = "test-retrieve-budget";
        let dir = seed_kb(
            name,
            vec![
                fact(
                    "short",
                    "retrieval short fact",
                    "a.md",
                ),
                fact(
                    "long",
                    "retrieval much longer fact that takes many more tokens because of its length",
                    "a.md",
                ),
            ],
        );
        // Very low budget — at most the shorter fact fits.
        let hits = retrieve_from_bindings(
            &[KnowledgeBinding::simple(name)],
            "retrieval",
            &RetrievalOptions {
                top_k: None,
                token_budget: Some(10),
                graph_expand: false,
                include_deprecated: false,
            },
        )
        .unwrap();
        assert!(hits.len() <= 1);
        std::fs::remove_dir_all(dir).ok();
    }
}
