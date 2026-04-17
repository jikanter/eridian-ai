//! Phase 26B: 1-hop graph-walk expansion.
//!
//! After Phase 26A returns a ranked list of seed facts, this module expands
//! the seed set by one hop along the authoritative `edges.jsonl` graph and
//! re-ranks the combined set via reciprocal rank fusion (RRF). The expansion
//! is capped at `2 × seed_count` to bound context cost — the same bound
//! GraphRAG production deployments use.
//!
//! Edges are deterministic (markdown links, shared file, shared canonical
//! entity) so expansion is fully explainable: every expanded fact was reached
//! via a concrete named edge from a ranked seed. Contrast with vector
//! neighborhood search, where the neighbors are opaque.

use indexmap::IndexMap;

use super::edp::FactId;
use super::query::FactHit;
use super::store::KnowledgeStore;

/// Cap on expansion size as a multiple of the seed count.
pub const EXPANSION_CAP_MULTIPLE: usize = 2;

/// RRF rank smoothing constant. 60 is the value from the original
/// Reciprocal Rank Fusion paper; we keep it for cross-KB/graph alignment
/// (Phase 26F reuses this same constant when fusing multiple KB results).
pub const RRF_K: f64 = 60.0;

/// Collect the set of 1-hop neighbors for the given seed ids. Deduplicates
/// by `FactId` and excludes the seeds themselves. Order follows the store's
/// edge list (insertion order preserved).
pub fn one_hop_neighbors(store: &KnowledgeStore, seeds: &[FactId]) -> Vec<FactId> {
    let seed_set: std::collections::HashSet<FactId> = seeds.iter().cloned().collect();
    let mut out: IndexMap<FactId, ()> = IndexMap::new();
    for seed in seeds {
        for edge in store.outbound_edges(seed) {
            if !seed_set.contains(&edge.to) && !out.contains_key(&edge.to) {
                out.insert(edge.to.clone(), ());
            }
        }
    }
    out.into_keys().collect()
}

/// Reciprocal Rank Fusion: fuse multiple ranked lists of fact ids into a
/// single merged ordering. Each rank position contributes `1 / (k + rank)`
/// to the fact's fused score; facts present in multiple lists accumulate.
///
/// This operates on **rank positions only**, not raw scores — so it's safe
/// to fuse lists produced by different scoring systems (BM25 vs graph).
/// Phase 26F reuses this same function to fuse multiple KBs.
pub fn reciprocal_rank_fusion(
    ranked_lists: &[&[FactId]],
    weights: &[f64],
    top_k: usize,
) -> Vec<(FactId, f64)> {
    let mut accum: IndexMap<FactId, f64> = IndexMap::new();
    for (list_idx, list) in ranked_lists.iter().enumerate() {
        let w = weights.get(list_idx).copied().unwrap_or(1.0);
        for (rank, id) in list.iter().enumerate() {
            let contribution = w / (RRF_K + (rank as f64) + 1.0);
            *accum.entry(id.clone()).or_insert(0.0) += contribution;
        }
    }
    let mut entries: Vec<(FactId, f64)> = accum.into_iter().collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    entries.truncate(top_k);
    entries
}

/// Primary retrieval expansion: given a ranked seed list from Phase 26A,
/// walk 1 hop on the explicit edge graph, merge seeds + expanded neighbors
/// via RRF, and return the fused hit list truncated to `top_k`.
///
/// The expansion set is capped at `EXPANSION_CAP_MULTIPLE * seeds.len()`
/// neighbors *before* fusion — unbounded expansion on a dense KB could
/// otherwise blow the token budget.
pub fn expand_and_fuse(
    store: &KnowledgeStore,
    seeds: Vec<FactHit>,
    top_k: usize,
) -> Vec<FactHit> {
    if seeds.is_empty() || top_k == 0 {
        return seeds.into_iter().take(top_k).collect();
    }

    let seed_ids: Vec<FactId> = seeds.iter().map(|h| h.fact.id.clone()).collect();
    let mut expanded_ids = one_hop_neighbors(store, &seed_ids);

    // Bound expansion so a densely-edged seed doesn't dominate the result.
    let cap = EXPANSION_CAP_MULTIPLE.saturating_mul(seed_ids.len());
    if expanded_ids.len() > cap {
        expanded_ids.truncate(cap);
    }

    // RRF: seeds weighted slightly higher than the graph-derived expansion,
    // since BM25 already established the seeds' relevance to the query.
    let seed_ref: &[FactId] = &seed_ids;
    let exp_ref: &[FactId] = &expanded_ids;
    let fused = reciprocal_rank_fusion(&[seed_ref, exp_ref], &[1.5, 1.0], top_k);

    // Reify fused ids into `FactHit`s. Seeds keep their original BM25 scores;
    // expanded-only facts get the fused rank-based score (on a different
    // scale, but the ordering is what matters downstream).
    let seed_by_id: std::collections::HashMap<FactId, FactHit> = seeds
        .into_iter()
        .map(|h| (h.fact.id.clone(), h))
        .collect();

    let mut out = Vec::with_capacity(fused.len());
    for (id, fused_score) in fused {
        if let Some(hit) = seed_by_id.get(&id) {
            out.push(hit.clone());
        } else if let Some(fact) = store.facts.iter().find(|f| f.id == id) {
            out.push(FactHit {
                fact: fact.clone(),
                score: fused_score as f32,
                source: fact.provenance.path.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::edp::{EdgeKind, EntityDescriptionPair, SourceAnchor};
    use crate::knowledge::query::FactHit;
    use crate::knowledge::store::{EdgeEntry, KnowledgeStore};
    use tempfile::tempdir;

    fn fact(entity: &str) -> EntityDescriptionPair {
        EntityDescriptionPair::new(
            entity,
            format!("{entity} description body"),
            vec![],
            SourceAnchor {
                path: "src.md".into(),
                byte_range: (0, 10),
                line_range: (1, 1),
                content_hash: "h".into(),
            },
            vec![],
        )
    }

    fn hit(fact: EntityDescriptionPair, score: f32) -> FactHit {
        let source = fact.provenance.path.clone();
        FactHit { fact, score, source }
    }

    fn store_with(facts: Vec<EntityDescriptionPair>, edges: Vec<EdgeEntry>) -> KnowledgeStore {
        let dir = tempdir().unwrap().keep();
        let path = dir.join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        for f in facts {
            store.append_fact(f).unwrap();
        }
        for e in edges {
            store.append_edge(e);
        }
        store
    }

    // ---- one_hop_neighbors ----

    #[test]
    fn one_hop_excludes_seeds_from_result() {
        let a = fact("a");
        let b = fact("b");
        let store = store_with(
            vec![a.clone(), b.clone()],
            vec![EdgeEntry {
                from: a.id.clone(),
                to: b.id.clone(),
                kind: EdgeKind::SharedFile,
            }],
        );
        let neighbors = one_hop_neighbors(&store, &[a.id.clone(), b.id.clone()]);
        assert!(neighbors.is_empty(), "b is a seed; must not appear as a neighbor of a");
    }

    #[test]
    fn one_hop_dedups_neighbors_reached_from_multiple_seeds() {
        let a = fact("a");
        let b = fact("b");
        let c = fact("c");
        // Both a and b link to c.
        let edges = vec![
            EdgeEntry {
                from: a.id.clone(),
                to: c.id.clone(),
                kind: EdgeKind::MarkdownLink,
            },
            EdgeEntry {
                from: b.id.clone(),
                to: c.id.clone(),
                kind: EdgeKind::MarkdownLink,
            },
        ];
        let store = store_with(vec![a.clone(), b.clone(), c.clone()], edges);
        let neighbors = one_hop_neighbors(&store, &[a.id.clone(), b.id.clone()]);
        assert_eq!(neighbors, vec![c.id]);
    }

    #[test]
    fn one_hop_returns_empty_when_no_edges() {
        let a = fact("a");
        let store = store_with(vec![a.clone()], vec![]);
        assert!(one_hop_neighbors(&store, &[a.id]).is_empty());
    }

    // ---- RRF ----

    #[test]
    fn rrf_fuses_overlapping_lists_higher() {
        let a = FactId::from_raw("fact-aaaaaaaaaaaaaaaa");
        let b = FactId::from_raw("fact-bbbbbbbbbbbbbbbb");
        let c = FactId::from_raw("fact-cccccccccccccccc");
        // a appears in both lists; b only in list 1; c only in list 2.
        let list1: &[FactId] = &[a.clone(), b.clone()];
        let list2: &[FactId] = &[a.clone(), c.clone()];
        let fused = reciprocal_rank_fusion(&[list1, list2], &[1.0, 1.0], 10);
        assert!(fused[0].0 == a, "a should win via double contribution");
    }

    #[test]
    fn rrf_weights_bias_toward_higher_weighted_list() {
        let a = FactId::from_raw("fact-aaaaaaaaaaaaaaaa");
        let b = FactId::from_raw("fact-bbbbbbbbbbbbbbbb");
        let list1: &[FactId] = &[a.clone()];
        let list2: &[FactId] = &[b.clone()];
        // list 2 is weighted 10x.
        let fused = reciprocal_rank_fusion(&[list1, list2], &[1.0, 10.0], 10);
        assert_eq!(fused[0].0, b);
    }

    #[test]
    fn rrf_truncates_to_top_k() {
        let ids: Vec<FactId> = (0..10)
            .map(|i| FactId::from_raw(format!("fact-{i:016x}")))
            .collect();
        let slice: &[FactId] = &ids;
        let fused = reciprocal_rank_fusion(&[slice], &[1.0], 3);
        assert_eq!(fused.len(), 3);
    }

    // ---- expand_and_fuse ----

    #[test]
    fn expand_returns_seeds_when_no_edges() {
        let a = fact("a");
        let store = store_with(vec![a.clone()], vec![]);
        let out = expand_and_fuse(&store, vec![hit(a.clone(), 3.0)], 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].fact.id, a.id);
    }

    #[test]
    fn expand_includes_1hop_neighbors_in_fused_result() {
        let a = fact("a");
        let b = fact("b");
        let edges = vec![EdgeEntry {
            from: a.id.clone(),
            to: b.id.clone(),
            kind: EdgeKind::MarkdownLink,
        }];
        let store = store_with(vec![a.clone(), b.clone()], edges);
        let out = expand_and_fuse(&store, vec![hit(a.clone(), 3.0)], 10);
        let ids: Vec<&FactId> = out.iter().map(|h| &h.fact.id).collect();
        assert!(ids.contains(&&a.id));
        assert!(ids.contains(&&b.id));
    }

    #[test]
    fn expand_caps_expansion_at_2x_seed_count() {
        // 1 seed, 5 neighbors — cap = 2 → only 2 neighbors survive.
        let seed = fact("seed");
        let mut neighbors = Vec::new();
        let mut edges = Vec::new();
        for i in 0..5 {
            let n = fact(&format!("n{i}"));
            edges.push(EdgeEntry {
                from: seed.id.clone(),
                to: n.id.clone(),
                kind: EdgeKind::SharedFile,
            });
            neighbors.push(n);
        }
        let mut all_facts = vec![seed.clone()];
        all_facts.extend(neighbors.clone());
        let store = store_with(all_facts, edges);

        let out = expand_and_fuse(&store, vec![hit(seed.clone(), 5.0)], 100);
        // 1 seed + at most 2 expanded → ≤ 3 total.
        assert!(out.len() <= 3, "got {}", out.len());
        assert!(out.iter().any(|h| h.fact.id == seed.id));
    }

    #[test]
    fn expand_truncates_to_top_k() {
        let seed = fact("seed");
        let n1 = fact("n1");
        let n2 = fact("n2");
        let edges = vec![
            EdgeEntry {
                from: seed.id.clone(),
                to: n1.id.clone(),
                kind: EdgeKind::SharedFile,
            },
            EdgeEntry {
                from: seed.id.clone(),
                to: n2.id.clone(),
                kind: EdgeKind::SharedFile,
            },
        ];
        let store = store_with(vec![seed.clone(), n1.clone(), n2.clone()], edges);
        let out = expand_and_fuse(&store, vec![hit(seed.clone(), 5.0)], 2);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn expand_empty_seeds_returns_empty() {
        let a = fact("a");
        let store = store_with(vec![a], vec![]);
        let out = expand_and_fuse(&store, vec![], 10);
        assert!(out.is_empty());
    }

    #[test]
    fn expand_preserves_seed_hit_fields() {
        let a = fact("a");
        let store = store_with(vec![a.clone()], vec![]);
        let seed_hit = FactHit {
            fact: a.clone(),
            score: 7.42,
            source: "custom-source".into(),
        };
        let out = expand_and_fuse(&store, vec![seed_hit], 10);
        assert_eq!(out.len(), 1);
        assert!((out[0].score - 7.42).abs() < 0.001);
        assert_eq!(out[0].source, "custom-source");
    }
}
