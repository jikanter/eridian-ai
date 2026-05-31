# Phase 21: Pipeline DAG Primitives : Overview - Epic 7

| Item | Description | Status |
|---|---|---|
| 21A | Fan-out (run multiple stages in parallel on same input) | **Done** |
| 21B | Conditional routing (`when:` predicate routes to stage A or B) | **Done** |
| 21C | Merge (combine parallel outputs into single input for next stage) | **Done** |
| 21D | Pipeline DAG validation (cycle detection, unreachable nodes, type compatibility) | **Done** |

**21A/21B/21C Design — DAG Pipeline Syntax:**

Stays declarative YAML. Additive to existing sequential model — a `pipeline:` without `parallel:` or `when:` works exactly as today.

```yaml
# Sequential (unchanged)
pipeline:
  - role: extract
  - role: summarize

# Fan-out + merge
pipeline:
  - role: extract
  - parallel:
      - role: security-review
      - role: style-review
      - role: performance-review
    merge: concatenate          # concatenate | json_array | custom_role
  - role: synthesize

# Conditional routing
pipeline:
  - role: classify
  - switch:
      - when: { output_field: "category", equals: "bug" }
        role: bug-triage
      - when: { output_field: "category", equals: "feature" }
        role: feature-review
      - otherwise:
        role: general-review
  - role: format
```

**Implementation:**

`PipelineStage` becomes an enum:

```rust
enum PipelineNode {
    Stage(PipelineStage),                   // existing sequential stage
    Parallel {
        branches: Vec<PipelineNode>,
        merge: MergeStrategy,
    },
    Switch {
        conditions: Vec<ConditionalBranch>,
        otherwise: Option<Box<PipelineNode>>,
    },
}

enum MergeStrategy {
    Concatenate,                            // join outputs with newlines
    JsonArray,                              // wrap in JSON array
    CustomRole(String),                     // merge via a role
}

struct ConditionalBranch {
    when: Predicate,                        // JSONPath predicate on prior output
    node: PipelineNode,
}
```

**Fan-out runtime:** Uses existing `futures_util::future::join_all` from Phase 7D2. Each parallel branch gets a clone of the input. Branches execute concurrently.

**Conditional runtime:** Evaluate `when:` predicates against the previous stage's JSON output. Predicates are deterministic — `output_field`, `equals`, `contains`, `gt`, `lt` — no LLM call.

**Merge strategies:**
- `concatenate`: join outputs with `\n---\n` separator
- `json_array`: `[output1, output2, output3]`
- `custom_role`: pipe concatenated outputs through a merge role

**Files:** `src/pipe.rs` (refactor `PipelineStage` to `PipelineNode` enum, add parallel/conditional execution), `src/config/role.rs` (parse DAG YAML syntax).

---

## Shipped (2026-05-11)

**Types:** `PipelineNode { Stage | Parallel | Switch }`, `MergeStrategy { Concatenate | JsonArray | CustomRole(String) }`, `Predicate { output_field?, equals?, contains?, gt?, lt? }`, `SwitchBranch { predicate: Option<Predicate>, node }`. All in `src/config/role.rs`.

**YAML shape that landed:** matches the design with one tweak — `otherwise:` is a boolean marker (`otherwise: true`) and the branch body is a sibling at the same indent level. The `- otherwise:\n  role: x` shape from the design draft would parse as `{otherwise: null, role: x}` in YAML; using an explicit `otherwise: true` is unambiguous and matches the `- when: {...}\n  role: x` pattern used by `when:` branches.

```yaml
pipeline:
  - role: extract
  - parallel:
      - role: security-review
      - role: style-review
    merge: concatenate           # default; json_array or `custom_role: <name>`
  - switch:
      - when: { output_field: "category", equals: "bug" }
        role: bug-triage
      - when: { contains: "urgent" }
        role: hot-path
      - otherwise: true
        role: general-review
  - role: format
```

**Runtime:**
- Sequential stages and `--stage` / `--pipe-def` flag behaviors are unchanged.
- `--pipe-def` YAML now accepts either `stages:` (legacy) **or** `pipeline:` (DAG). Mixing both fails preflight.
- Fan-out: each branch sees a clone of the prior stage's output; branches run concurrently via `futures_util::future::join_all`.
- Switch: every `when:` is evaluated against the prior output; the first match wins. If none match, the `otherwise:` branch runs. With no `otherwise:` and no match, the pipeline errors.
- JSON trace envelope now stamps `branch: N` on stages produced inside a fan-out so consumers can group them.

**Validation (21D):**
- Parser-time: empty `parallel:` / `switch:`, missing branch body, unknown merge strategy, double `otherwise:`, `when:` after `otherwise:` (misleading order).
- Preflight: every leaf role across the DAG (including custom-role mergers) must exist; every stage's model must resolve.
- Tool dispatch: pipeline-role cycles (direct or transitive self-reference) are detected before any LLM call. Cross-role cycles walk the role graph; visited roles short-circuit on a chain repeat.

**Tests added:** 23 unit tests in `src/config/role.rs` (parsing + predicate evaluation), 3 in `src/config/preflight.rs` (structural ordering + recursion), 9 bats tests in `tests/integration/pipeline.sh`, 3 in `tests/regression/pipeline.sh`. Suite is now 482 unit / 197 compatibility / 13 integration pipeline / 7 regression pipeline.

**Out of scope for Phase 21 (Phase 22):** DAG trace visualization (tree-shaped `--trace` output), per-branch cost tracking, budget-aware fan-out (split a parent budget across branches), DAG-aware stage caching (skip branches whose inputs are unchanged).
