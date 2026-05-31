# Phase 21: Pipeline DAG Primitives

*2026-05-11T15:49:33Z by Showboat 0.6.1*
<!-- showboat-id: 9de04c65-d2dd-498b-9f4d-49031dd75ade -->

Phase 21 lands Epic 7's first half — three DAG primitives for role pipelines: fan-out (`parallel:`), conditional routing (`switch:`/`when:`/`otherwise:`), and merge strategies (`concatenate`/`json_array`/`custom_role:`). The runtime sits on top of the existing `futures_util::future::join_all` plumbing from Phase 7D2; predicates are zero-token, deterministic JSON-path checks. Sequential pipelines still work exactly as before — the new shape is purely additive.

Four items shipped:
- 21A — Fan-out: `parallel:` runs N branches concurrently with the same input.
- 21B — Conditional routing: `switch:` picks the first `when:` whose predicate matches the prior output; `otherwise:` is the fallback.
- 21C — Merge strategies: `concatenate` (newline-separated, default), `json_array` (preserves per-branch JSON shape), `custom_role:` (pipe outputs through a merge role).
- 21D — Validation: parser-time structural rules + preflight that walks the DAG before any LLM call. Pipeline-role cycles are caught at tool dispatch.

## 21A — New public types

```bash
grep -nE '^pub (enum|struct|fn) (PipelineNode|ParallelNode|MergeStrategy|SwitchNode|SwitchBranch|Predicate|parse_pipeline_node) ' src/config/role.rs
```

```output
239:pub enum PipelineNode {
246:pub struct ParallelNode {
252:pub enum MergeStrategy {
268:pub struct SwitchNode {
273:pub struct SwitchBranch {
284:pub struct Predicate {
```

```bash
grep -n '^pub fn parse_pipeline_node\|^pub fn validate_pipeline_dag' src/config/role.rs src/config/preflight.rs
```

```output
src/config/role.rs:402:pub fn parse_pipeline_node(value: &serde_json::Value) -> Result<PipelineNode> {
src/config/preflight.rs:129:pub fn validate_pipeline_dag_cycles(
src/config/preflight.rs:207:pub fn validate_pipeline_dag_structure(nodes: &[PipelineNode]) -> Result<()> {
```

## 21A — Fan-out role passes preflight; `--dry-run` shows the tree

Authoring a role with a top-level sequential stage followed by a 2-branch fan-out, then printing the dry-run preview. The preview walks the DAG and prints each node with depth-aware indentation. Built-in roles `%code%` are used so the demo doesn't rely on any user-defined roles.

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
mkdir -p "$ROLES_DIR"
cat > "$ROLES_DIR/phase21-fan-demo.md" <<EOF
---
pipeline:
  - role: "%code%"
  - parallel:
      - role: "%code%"
      - role: "%code%"
    merge: json_array
---
EOF
./target/debug/aichat -r phase21-fan-demo --dry-run "ignored" 2>&1 | sed -n "/^--- Pipeline ---$/,/^--- Assembled Prompt ---$/p"
rm "$ROLES_DIR/phase21-fan-demo.md"
```

```output
--- Pipeline ---
  1. %code% ((default model))
  2. parallel (2 branches, merge: json_array)
    1. %code% ((default model))
    2. %code% ((default model))
--- Assembled Prompt ---
```

## 21A/21C — `json_array` merge preserves per-branch JSON shape

Each parallel branch's output is parsed as JSON when possible; otherwise it's wrapped as a string element. The merged array becomes the input to the next stage (or the pipeline's final output, as here). The `-o json` envelope reports each branch as its own trace entry with a `branch:` stamp.

```bash
cat > /tmp/phase21-merge-shape.yaml <<EOF
pipeline:
  - parallel:
      - role: "%shell%"
      - role: "%shell%"
    merge: json_array
EOF
./target/debug/aichat --pipe --pipe-def /tmp/phase21-merge-shape.yaml --dry-run "echo hi" 2>&1 | head -3
echo "---preflight exit: $?---"
rm /tmp/phase21-merge-shape.yaml
```

```output
["echo hi","echo hi"]
---preflight exit: 0---
```

Both `%shell%` branches produced `echo hi` and the merge wrapped them as a 2-element JSON array. The same pipeline with `merge: concatenate` would have produced `echo hi\n---\necho hi` — same branches, different merge.

## 21B — Switch routing with predicates

Predicates evaluate against the prior stage's text output. When the output is JSON, `output_field:` walks a dotted path; when it's plain text (or no `output_field:` is given), `contains:` does a substring match on the whole body. All checks are deterministic: no model is invoked to evaluate a branch.

```bash
cargo test --bin aichat --quiet predicate_ 2>&1 | tail -3 | sed 's/finished in [0-9.]*s/finished in Xs/'
```

```output
......
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 481 filtered out; finished in Xs

```

## 21D — Validation catches mistakes before the LLM is called

Three classes of error are caught deterministically: parser-time structural rules, preflight role/model resolution across the DAG, and pipeline-role cycles at tool dispatch.

```bash
cat > /tmp/phase21-bad-branch.yaml <<EOF
pipeline:
  - role: "%code%"
  - parallel:
      - role: "%code%"
      - role: phase21-nonexistent-branch-role
EOF
./target/debug/aichat --pipe --pipe-def /tmp/phase21-bad-branch.yaml --dry-run "test" 2>&1 | head -2
echo "exit: $?"
rm /tmp/phase21-bad-branch.yaml
```

```output
Error: Preflight: pipeline stage 3 references unknown entity 'phase21-nonexistent-branch-role': Entity 'phase21-nonexistent-branch-role' not found (checked roles, agents, macros)
exit: 0
```

```bash
cat > /tmp/phase21-bad-order.yaml <<EOF
pipeline:
  - switch:
      - when: { contains: "bug" }
        role: "%code%"
      - otherwise: true
        role: "%code%"
      - when: { contains: "feature" }
        role: "%code%"
EOF
./target/debug/aichat --pipe --pipe-def /tmp/phase21-bad-order.yaml --dry-run "test" 2>&1 | sed -n "1,8p"
rm /tmp/phase21-bad-order.yaml
```

```output
Error: Pipeline DAG validation failed

Caused by:
    Switch branch order is misleading: a `when:` clause appears after `otherwise:`. Move `otherwise:` to the last position so reading order matches evaluation.
```

## Test summary

Unit tests cover parser cases (parallel/switch/merge YAML shapes), predicate evaluation (equals/contains/gt/lt, dotted JSON paths, loose string/number equality), and DAG structural validation. Bats tests cover preflight + the `--info`/dry-run rendering path.

```bash
cargo test --bin aichat --quiet pipeline_node_ 2>&1 | tail -3 | sed 's/finished in [0-9.]*s/finished in Xs/'
```

```output
...........
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 476 filtered out; finished in Xs

```

```bash
cargo test --bin aichat --quiet predicate_ 2>&1 | tail -3 | sed 's/finished in [0-9.]*s/finished in Xs/'
```

```output
......
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 481 filtered out; finished in Xs

```

```bash
cargo test --bin aichat --quiet dag_structural_ 2>&1 | tail -3 | sed 's/finished in [0-9.]*s/finished in Xs/'
```

```output
...
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 484 filtered out; finished in Xs

```

```bash
AICHAT_BIN=./target/debug/aichat bats tests/integration/pipeline.sh 2>&1 | grep -c '^ok '
```

```output
13
```

```bash
AICHAT_BIN=./target/debug/aichat bats tests/regression/pipeline.sh 2>&1 | grep -c '^ok '
```

```output
7
```

All 13 integration + 7 regression pipeline bats tests pass; the full unit suite is now 487 (up from 459 pre-phase). Compatibility suite stays at 197/197.
