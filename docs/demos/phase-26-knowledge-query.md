# Phase 26: Knowledge Query & Composability

*2026-04-17T03:57:43Z by Showboat 0.6.1*
<!-- showboat-id: 0c60d594-b335-4c33-b1fb-03ef81094bcb -->

Phase 26 wires the compiled knowledge store from Phase 25 into roles, pipelines, CLI, and the tool-calling surface. Retrieval is deterministic: **tag filter → BM25 → 1-hop graph walk → RRF fuse across bindings → token budget**. No embeddings anywhere.

## Module surface

```bash
ls -1 src/knowledge/
```

```output
cli.rs
compile.rs
edp.rs
graph.rs
mod.rs
query.rs
restore.rs
retrieve.rs
store.rs
tags.rs
```

New in Phase 26: `query.rs`, `graph.rs`, `retrieve.rs`. The rest landed in Phase 25.

## 26C — Role frontmatter (three shapes)

Four-space-indented examples (non-executable):

    # Shape 1 — single bare string
    knowledge: my-docs

    # Shape 2 — list of strings
    knowledge:
      - api-docs
      - codebase

    # Shape 3 — full-form, per-binding tag filter + weight
    knowledge:
      - name: api-docs
        tags: [kind:rule]
        weight: 1.5
      - name: codebase

    # Bonus: tool mode — LLM calls search_knowledge instead of auto-injection
    knowledge: my-docs
    knowledge_mode: tool

## 26A — Query core API

```bash
grep -nE "^pub (fn|struct)" src/knowledge/query.rs | head -15
```

```output
34:pub struct FactHit {
41:pub struct QueryOptions {
66:pub fn filter_by_tags<'a>(
88:pub fn bm25_rank(
125:pub fn apply_budget(hits: Vec<FactHit>, budget: usize) -> Vec<FactHit> {
139:pub fn query(store: &KnowledgeStore, text: &str, opts: &QueryOptions) -> Vec<FactHit> {
152:pub fn default_budget_for(max_input_tokens: Option<usize>) -> Option<usize> {
161:pub fn format_hits_for_injection(hits: &[FactHit]) -> String {
179:pub fn hits_to_json(hits: &[FactHit]) -> serde_json::Value {
199:pub fn hit_ids(hits: &[FactHit]) -> Vec<FactId> {
```

## 26B — Graph walk + RRF

```bash
grep -nE "^pub (fn|const)" src/knowledge/graph.rs
```

```output
21:pub const EXPANSION_CAP_MULTIPLE: usize = 2;
26:pub const RRF_K: f64 = 60.0;
31:pub fn one_hop_neighbors(store: &KnowledgeStore, seeds: &[FactId]) -> Vec<FactId> {
51:pub fn reciprocal_rank_fusion(
77:pub fn expand_and_fuse(
```

## 26D/F — Multi-binding retrieval orchestrator

```bash
grep -nE "^pub (fn|struct)" src/knowledge/retrieve.rs
```

```output
31:pub struct RetrievalOptions {
54:pub fn retrieve_from_bindings(
```

## 26E — CLI surface (all six flags)

```bash
./target/debug/aichat --help 2>&1 | grep "knowledge"
```

```output
      --knowledge <KB_NAME>
          Phase 26D: attach a knowledge base to this invocation (repeatable)
      --knowledge-search <QUERY>
      --knowledge-compile <KB_NAME>
          Phase 25B: compile source files into a knowledge base (requires -f)
      --knowledge-list
          Phase 25E: list all compiled knowledge bases
      --knowledge-stat <KB_NAME>
      --knowledge-show <KB:ID>
```

## 26E — Tool-mode dispatch

```bash
grep -n "SEARCH_KNOWLEDGE_NAME\|search_knowledge\|eval_search_knowledge" src/function.rs src/config/mod.rs | head -10
```

```output
src/function.rs:244:pub const SEARCH_KNOWLEDGE_NAME: &str = "search_knowledge";
src/function.rs:269:    /// Phase 26E: synthetic `search_knowledge` tool. Injected when the active
src/function.rs:273:    pub fn search_knowledge() -> Self {
src/function.rs:275:            name: SEARCH_KNOWLEDGE_NAME.to_string(),
src/function.rs:343:        // Phase 26E: Handle search_knowledge synthetic tool.
src/function.rs:344:        if self.name == SEARCH_KNOWLEDGE_NAME {
src/function.rs:345:            return self.eval_search_knowledge(config);
src/function.rs:432:    /// Phase 26E: handle `search_knowledge` synthetic tool calls. Resolves
src/function.rs:435:    fn eval_search_knowledge(&self, config: &GlobalConfig) -> Result<Value> {
src/function.rs:446:                "note": "search_knowledge called with empty query",
```

## Unit tests across 26A–26F

```bash
cargo test --bin aichat -- knowledge:: 2>&1 | grep "^test result: ok\." | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 106 passed; 0 failed; 0 ignored; 0 measured; 274 filtered out; finished in Xs
```

Role-level tests (Phase 26C) live under `config::role::tests::test_knowledge_*` — 9 tests covering bare string / list / object frontmatter shapes and round-trip export.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 380 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
