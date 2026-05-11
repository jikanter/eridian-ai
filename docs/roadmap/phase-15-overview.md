# Phase 15: Contract Testing : Overview - Epic 4

**Status (2026-05-11):** **Partially scaffolded.** Basic preflight stage validation (existence + capability compatibility) ships in `src/config/preflight.rs::validate_pipeline_stages()` and runs implicitly before pipeline execution. The full contract-testing surface — JSON Schema containment between adjacent stages and a standalone `--check` flag — is **not implemented**.

| Item | Description | Status |
|---|---|---|
| 15A | Pipeline schema compatibility check at authoring time (`showboat validate-pipeline`) | **Partial** — stage existence + capability checks live in `src/config/preflight.rs`; runs implicitly at execution time, not standalone |
| 15B | Cross-stage schema containment validation (output N satisfies input N+1) | Planned — no JSON Schema subset checking exists yet |
| 15C | `--check` flag for validating role/pipeline definitions without execution | Planned — no standalone validation flag exists |

**15A Design — Authoring-Time Validation:**

```bash
$ showboat validate-pipeline extract-review-format

Pipeline: extract-review-format (3 stages)
  Stage 1: extract
    output_schema: { text: string, metadata: object }
  Stage 2: review
    input_schema:  { content: string, language: string }     # MISMATCH
    output_schema: { issues: array, severity: string }
  Stage 3: format
    input_schema:  { issues: array }                         # OK (subset)

FAIL: Stage 1 output -> Stage 2 input
  Missing: content, language
  Extra: text, metadata
  Suggestion: Add a transform role or update schemas for compatibility.
```

JSON Schema containment check: verify that a document conforming to output_schema would pass input_schema validation. This is deterministic — no LLM needed. Zero runtime cost, prevents an entire class of pipeline failures.

**Files:** `src/config/preflight.rs` (new: pipeline schema validation), integration with `showboat` command.
