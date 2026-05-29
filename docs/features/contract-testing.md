# Contract Testing (`--check`)

`aichat --check` validates a role or pipeline **definition** without running it. It is deterministic and zero-token — no model is ever called — so it belongs in pre-commit hooks, CI, and authoring loops where you want to catch a broken pipeline before spending a single token on it.

A pipeline chains roles. Each role declares an `input_schema` (what it accepts) and an `output_schema` (what it produces). Stage N's output becomes stage N+1's input, so the two schemas have to line up. `--check` proves they do — or shows you exactly where they don't.

See also: [Phase 15 design](../roadmap/phase-15-overview.md), [architecture.md](../architecture/architecture.md), [Typed Input](./typed-input.md), [demo](../demos/phase-15-contract-testing.md).

## What it checks

| Check | Source | Detail |
|---|---|---|
| Stage existence | 15A | Every stage role/agent/remote referenced by the pipeline resolves. |
| Model & tool capability | 15A | A stage that needs tool calling isn't pinned to a non-function-calling model. |
| DAG structure | 21D | `parallel:`/`switch:` nodes are well-formed; no `when:` after `otherwise:`. |
| Cycles | 21D | A pipeline role can't transitively reference itself. |
| Schema containment | **15B** | Output of stage N satisfies the input of stage N+1. |
| Schema validity | 15C | A role's own `input_schema`/`output_schema` are valid JSON Schema. |

The first four already run implicitly before every pipeline execution. `--check` runs them standalone and adds the cross-stage containment check (15B) and a standalone validity check on declared schemas.

## Usage

```bash
# A role that defines a pipeline:
aichat --check -r review-pipeline

# A single role (capability + schema validity + port summary):
aichat --check -r summarize

# An ad-hoc pipeline, no role file needed:
aichat --check --pipe --stage extract --stage review --stage format

# A pipeline definition file:
aichat --check --pipe --pipe-def review.yaml

# Machine-readable, for CI gating:
aichat --check -r review-pipeline -o json
```

Exit codes: **0** valid · **3** invalid · **2** nothing to check.

## Schema containment (15B)

Two adjacent stages are **compatible** when every document valid under the upstream `output_schema` is also valid under the downstream `input_schema` — i.e. the output schema is a *subset of* (is *contained by*) the input schema. This is decided deterministically from the declared schemas; the model is never consulted.

Example — an incompatible boundary:

```
Pipeline: review-pipe (3 stages)
  1. extract                  in: any                    out: json{text, metadata}
  2. review                   in: json{content, language} out: text
  3. format                   in: json{issues}           out: text

FAIL: stage 1 (extract) → stage 2 (review)
  Missing: content, language
  Extra:   text, metadata
  Suggestion: add a transform stage, or align the schemas so the
              upstream output satisfies the downstream input.

check failed: 1 incompatible boundary
```

What the check reports per boundary:

- **Missing** — a field the downstream stage *requires* that the upstream stage does not *guarantee*. The upstream guarantees a field only if it is in the upstream's own `required` list; an optional field may be omitted by a conforming document, which breaks containment. → **FAIL**.
- **Type mismatch** — a field declared by both sides with conflicting `type`s (`integer` is accepted where a `number` is expected, but not vice-versa). → **FAIL**.
- **Forbidden** — when the downstream sets `additionalProperties: false`, any field the upstream can emit that the downstream doesn't declare. → **FAIL**.
- **Extra** — fields the upstream emits that the downstream doesn't declare. Informational only (JSON Schema allows additional properties by default); listed for context.

Two non-fatal verdicts:

- **WARN** — the upstream stage declares no `output_schema`, so it emits free text. The text *might* be JSON that conforms, but nothing guarantees it. Surfaced, but does not fail the check.
- **note (Unknown)** — a schema uses `anyOf`/`oneOf`/`allOf`/`$ref`/`not`. Exact containment is undecidable for these in the general case, so the check declines to guess rather than risk a false failure. Runtime validation still applies.

## Limitations (by design)

- **Sequential pipelines only.** Cross-stage containment runs on a purely sequential stage list. For `parallel:`/`switch:` DAGs the check validates structure and stage existence and prints a `non-sequential` note; adjacent-stage shape validation across branches is [Phase 33D](../roadmap/phase-33-overview.md).
- **Top-level shapes.** Containment compares top-level object properties, scalar `type`s, and arrays. It is intentionally conservative: it only reports a `FAIL` for a *provable* violation, returning `Unknown` for shapes it cannot reason about statically.
- **Roles only.** Agent and remote stages can't be introspected statically; those boundaries are reported as `SKIP`.

## JSON output

```bash
aichat --check -r review-pipe -o json
```

```json
{
  "valid": false,
  "target": "review-pipe",
  "kind": "pipeline",
  "stages": [
    { "position": 1, "role": "extract", "input": "any", "output": "json{text, metadata}" }
  ],
  "boundaries": [
    {
      "from": "extract", "to": "review", "status": "fail",
      "missing": ["content", "language"], "extra": ["text", "metadata"],
      "forbidden": [], "type_mismatches": [], "notes": []
    }
  ],
  "non_sequential": false,
  "errors": []
}
```

`status` per boundary is one of `ok` / `fail` / `warn` / `unknown` / `skipped`.

## Implementation

- `src/config/preflight.rs::schema_containment` — the pure containment core.
- `src/config/preflight.rs::validate_pipeline_schema_containment` — walks adjacent boundaries, resolving each stage's role.
- `src/pipe.rs::run_check` — the `--check` entry point and report rendering.
