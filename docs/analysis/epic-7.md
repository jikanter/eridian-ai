# Epic 7: DAG Execution

**Created:** 2026-04-07
**Status:** Planning
**Depends on:** Epic 5 (server pipeline engine), Epic 4 Phase 15 (contract testing)
**Phases:** 21-22
**Source:** Theme 4 — AI Architect

---

## Motivation

Sequential pipelines are the bicycle. DAGs with conditional routing are the car. The runtime foundation (concurrent tool execution via `join_all` from Phase 7D2) already exists. Three new primitives within the existing pipeline model: fan-out, conditional, merge.

---

## Phases

### Phase 21: Pipeline DAG Primitives

| Item | Description |
|---|---|
| 21A | Fan-out (run multiple stages in parallel on same input) |
| 21B | Conditional routing (`when:` predicate routes to stage A or B) |
| 21C | Merge (combine parallel outputs into single input for next stage) |
| 21D | Pipeline DAG validation (cycle detection, unreachable nodes, type compatibility) |

### Phase 22: DAG Observability & Budget

| Item | Description |
|---|---|
| 22A | DAG trace visualization (tree structure in `--trace` output) |
| 22B | Per-branch cost tracking in parallel execution |
| 22C | Budget-aware fan-out (split pipeline budget across parallel branches) |
| 22D | DAG stage caching (cache branches independently, skip unchanged) |

---

## Key Designs

**DAG Pipeline Syntax** — additive to existing sequential model:

```yaml
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

**Implementation:** `PipelineStage` becomes `PipelineNode` enum (Stage, Parallel, Switch). Fan-out uses existing `futures_util::future::join_all`. Conditionals are deterministic predicates on JSON output — no LLM call.

**Merge strategies:** `concatenate` (newline join), `json_array` (wrap outputs), `custom_role` (pipe through merge role).

Files: `src/pipe.rs` (refactor PipelineStage → PipelineNode), `src/config/role.rs` (parse DAG YAML).

Full designs with trace examples: [ROADMAP.md, Epic 7 section](../ROADMAP.md#epic-7-dag-execution-new)

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Visual DAG editor | Violates "no desktop UI". YAML is the authoring tool. |
| Dynamic DAG modification at runtime | Deterministic structure. LLM doesn't alter the DAG. |
| Arbitrary graph topologies (cycles) | DAGs only. Cycles are validation errors, not features. |
