# Success Metrics

Per-epic targets the roadmap is committing to. Linked from [`ROADMAP.md`](../ROADMAP.md).

| Metric | Current State | Target | Epic |
|---|---|---|---|
| Schema failure rate with `output_schema` | Unknown | <5% (Phase 9A/B), <1% (Phase 9C) | 2 |
| Pipeline re-run cost after stage failure | 100% (full re-run) | Stage cost only (Phase 10B cache) | 2 |
| Time to understand a role before using it | Read YAML file | `--dry-run` shows everything in 0 tokens | 3 |
| Time to create a role variant | 5 min (copy + edit) | 5 sec (`--fork-role`) | 3 |
| Can compose roles across machines | First-class (`remote:host/role`, Phase 20) | — (achieved) | 6 |
| Pipeline topology | Fan-out, conditional, merge (Phase 21) | DAG observability + budget (Phase 22) | 7 |
| Role quality tracking | None | Per-role metrics + regression tests | 8 |
| AIChat features accessible via HTTP | 7 (chat, embed, rerank, role invoke, pipeline, batch, role list) | 8+ (add cost estimation) | 5 |
| Context utilization for `-f dir/` | BM25-ranked, budget-optimized (Phase 11A/B) | Pipeline-level budget propagation (Phase 11D) | 2 |
| Pre-flight error prevention | Capability/schema checks (Phase 9D, 14) | All capability mismatches caught | 2/4 |
| Batch cost savings with mixed complexity | 0% (static model) | 40-60% (deterministic routing via `switch:`/`when:`) | 7 |
