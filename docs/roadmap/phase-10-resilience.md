# Phase 10: Runtime Intelligence — Resilience

**Status:** Done
**Epic:** 2 — Runtime Intelligence
**Design:** [epic-2.md](../analysis/epic-2.md)

---

| Item | Status | Notes |
|---|---|---|
| 10A. API-level retry with exponential backoff | Done | New `src/client/retry.rs`. Retries on HTTP 429/500/502/503 with exponential backoff (default: 3 retries, 1s initial, 30s max). Parses `Retry-After` header on 429. Fails immediately on 401/403/404. Global `retry:` config section. |
| 10B. Pipeline stage output cache | Done | New `src/cache.rs`. Content-addressable cache keyed on `sha256(role + model + input)`. File-backed in `<config_dir>/.cache/stages/`. Configurable TTL (default: 1hr). `--no-cache` flag to bypass. Checked before LLM call in `pipe.rs:run_stage_inner()`. |
| 10C. Pipeline stage retry | Done | On stage failure, retry up to N times (default: 1) before propagating. New `stage_retries:` role frontmatter field. `is_retryable_stage_error()` returns true for API (5/6), schema (8), and model (7) errors; false for config (3), auth (4), abort (9). |
| 10D. Pipeline model fallback | Done | New `fallback_models:` role frontmatter field (list of model IDs). After exhausting retries with primary model, try each fallback in order. Wraps the retry loop: `fallback(retry(cache(run_stage_inner)))`. |

**Parallelization:** 10A is fully independent of 10B/C/D. Within the pipeline features: 10B (cache), 10C (retry), 10D (fallback) modify different layers of `pipe.rs:run_stage()` and can be implemented by separate agents, then composed. Nesting order: `10D(10C(10B(run_stage_inner)))`.

**Dependency:** 10A should land before 10C — stage retry benefits from API retry being in place (otherwise a stage retry that hits a rate limit still fails).

**Key files:** new `src/client/retry.rs` (10A), new `src/cache.rs` (10B), `src/pipe.rs` (10B/C/D), `src/config/role.rs` (10C/D), `src/cli.rs` (10B `--no-cache` flag), `Cargo.toml` (`sha2` crate for 10B).
