# Phase 15: Contract Testing : Overview - Epic 4

**Status (2026-05-29):** **Done.** Stage existence + capability checks (15A) run implicitly before every pipeline and are now also exposed standalone; cross-stage JSON Schema containment (15B) is implemented in `src/config/preflight.rs::schema_containment()`; and the `--check` flag (15C) validates a role or pipeline definition without executing it. User docs: [`docs/features/contract-testing.md`](../features/contract-testing.md). Demo: [`docs/demos/phase-15-contract-testing.md`](../demos/phase-15-contract-testing.md).

| Item | Description | Status |
|---|---|---|
| 15A | Pipeline stage existence + model/tool capability checks at authoring time | **Done** — `validate_pipeline_stages()` runs implicitly before execution and standalone via `--check` |
| 15B | Cross-stage schema containment validation (output N satisfies input N+1) | **Done** — `schema_containment()` + `validate_pipeline_schema_containment()` in `src/config/preflight.rs` |
| 15C | `--check` flag for validating role/pipeline definitions without execution | **Done** — `src/pipe.rs::run_check`; exits 0 valid / 3 invalid / 2 usage, `-o json` supported |

> **What shipped vs. the original design.** The 15A design sketched a `showboat validate-pipeline` subcommand; this was unified into the single `--check` flag (one tool per job — `--check` covers roles, ad-hoc `--pipe` chains, and pipeline-def files). Containment is checked on **sequential** pipelines; `parallel:`/`switch:` DAGs get structure + existence checks plus a `non-sequential` note. Extending containment across DAG branches is [Phase 33D](phase-33-overview.md). The check is deliberately conservative: it returns `Unknown` (no failure) for `anyOf`/`oneOf`/`allOf`/`$ref`/`not` rather than risk a false positive.

**Shipped surface — `--check`:**

```bash
$ aichat --check -r extract-review-format

Pipeline: extract-review-format (3 stages)
  1. extract                  in: any                    out: json{text, metadata}
  2. review                   in: json{content, language} out: text
  3. format                   in: json{issues}           out: text

FAIL: stage 1 (extract) → stage 2 (review)
  Missing: content, language
  Extra:   text, metadata
  Suggestion: add a transform stage, or align the schemas so the
              upstream output satisfies the downstream input.

check failed: 1 incompatible boundary      # exit 3
```

JSON Schema containment check: a document conforming to stage N's `output_schema` must also pass stage N+1's `input_schema` validation (output schema ⊆ input schema). Deterministic — no LLM needed, zero runtime cost — and prevents an entire class of pipeline failures before any token is spent. See [`docs/features/contract-testing.md`](../features/contract-testing.md) for the full report semantics (Missing / Type mismatch / Forbidden / Extra, and the WARN / Unknown / SKIP non-failure verdicts).

**Files:**
- `src/config/preflight.rs` — `schema_containment()` (pure containment core, unit-tested) + `validate_pipeline_schema_containment()` (resolves adjacent stage roles).
- `src/pipe.rs` — `run_check()` and the human/JSON report rendering; `--check` flag in `src/cli.rs`, dispatch in `src/main.rs`.
- Tests: `src/config/preflight.rs` (14 containment unit tests); `tests/integration/check.sh` (11 cases).
