# Success Metrics

Per-epic targets the roadmap is committing to. Linked from [`../ROADMAP.md`](../ROADMAP.md).

## Shipped & in-flight

| Metric | Current State | Target | Epic |
|---|---|---|---|
| Schema failure rate with `output_schema` | <1% (Phase 9C, **achieved**) | <5% (9A/B), <1% (9C) | 2 |
| Pipeline re-run cost after stage failure | Stage cost only (Phase 10B cache, **achieved**) | Stage cost only | 2 |
| Time to understand a role before using it | `--dry-run` shows everything in 0 tokens (**achieved**) | 0-token introspection | 3 |
| Time to create a role variant | 5 sec (`--fork-role`, **achieved**) | 5 sec | 3 |
| Compose roles across machines | First-class (`remote:host/role`, Phase 20, **achieved**) | — | 6 |
| Pipeline topology | Per-stage config isolation (Phase 36, **achieved**) | DAG + budget + isolation | 7 |
| Context utilization for `-f dir/` | Budget propagation (Phase 11D, **achieved**) | Cache-aware budgeting (37–41) | 2 |
| Role quality tracking | Per-role metrics + cost ledger (Phase 23, **achieved**) | Regression tests + distillation (Phase 24) | 8 |
| Repeated-call cost (cache hits) | No response cache | L1/L3 transparent caching (Phase 37, **in flight**) | 2 |
| Batch cost savings with mixed complexity | Deterministic routing via `switch:`/`when:` (**achieved**) | 40–60% | 7 |

## Next-year targets (Epics 15–17 + finishing committed work)

| Metric | Current State | Target | Epic / Phase |
|---|---|---|---|
| Trace coverage of a turn | Ad-hoc `--trace`/`AICHAT_TRACE` (8F/8G) | Every lifecycle event in SPEC-001 schema, blob-backed, redacted, non-blocking | 15 / 42 |
| Control-flow test determinism | Live-provider integration tests | Retry/fallback/timeout exercised offline via wiremock + `tokio::time::pause` | 15 / 43 |
| Training-pair yield (uncontaminated) | None | SFT/DPO pairs extracted from traces with **0 replayed responses** leaking in | 15 / 44 |
| Eval-replay determinism | None | A committed cassette replays **byte-identically offline**, token-free; drift fails CI | 16 / 46 |
| Astrophage cache-hit savings | In-aichat `StageCache` only | Wire-level hits over `base_url` with `cache_hit:true` + correlated `cache.lookup` | 16 / 45 |
| Tool-replay key stability | Open (`SPEC-astrophage §9.2`) | `(tool_name, args_hash)` proven a stable lookup key; deterministic tool+wire eval end-to-end | 16 / 46C |
| Agent memory federation | Per-agent local file (planned 29B) | Agent facts queryable over knowledge-MCP from another machine, AEVS-gated, turn-attributed | 10 / 49 |
| Knowledge portability | Compiled KB is a local directory | KB as a portable committed artifact, queried remotely, drift-detected | 17 / 50 |
| Local-model parameter reach | Fixed OpenAI-compatible body | Vendor knobs (`num_ctx`, guided decoding) pass through via `extensions:` merge | 17 / 51 |
