# Phase 26: Knowledge Query & Composability

*2026-04-17T03:57:43Z by Showboat 0.6.1*
<!-- showboat-id: 0c60d594-b335-4c33-b1fb-03ef81094bcb -->

Phase 26 wires the compiled knowledge store from Phase 25 into roles, pipelines, CLI, and the tool-calling surface. Retrieval is deterministic: **tag filter → BM25 → 1-hop graph walk → RRF fuse across bindings → token budget**. No embeddings anywhere.

## Module surface

```bash
ls -1 src/knowledge/ | grep -E "^(cli|compile|edp|graph|mod|query|restore|retrieve|store|tags)\.rs$"
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
grep -E "^pub (fn (filter_by_tags|bm25_rank|apply_budget|query|default_budget_for|format_hits_for_injection|hits_to_json|hit_ids)|struct (FactHit|QueryOptions))" src/knowledge/query.rs
```

```output
pub struct FactHit {
pub struct QueryOptions {
pub fn filter_by_tags<'a>(
pub fn bm25_rank(
pub fn apply_budget(hits: Vec<FactHit>, budget: usize) -> Vec<FactHit> {
pub fn query(store: &KnowledgeStore, text: &str, opts: &QueryOptions) -> Vec<FactHit> {
pub fn default_budget_for(max_input_tokens: Option<usize>) -> Option<usize> {
pub fn format_hits_for_injection(hits: &[FactHit]) -> String {
pub fn hits_to_json(hits: &[FactHit]) -> serde_json::Value {
pub fn hit_ids(hits: &[FactHit]) -> Vec<FactId> {
```

## 26B — Graph walk + RRF

```bash
grep -E "^pub (fn|const)" src/knowledge/graph.rs
```

```output
pub const EXPANSION_CAP_MULTIPLE: usize = 2;
pub const RRF_K: f64 = 60.0;
pub fn one_hop_neighbors(store: &KnowledgeStore, seeds: &[FactId]) -> Vec<FactId> {
pub fn reciprocal_rank_fusion(
pub fn expand_and_fuse(
```

## 26D/F — Multi-binding retrieval orchestrator

```bash
grep -E "^pub (struct RetrievalOptions|fn retrieve_from_bindings\b)" src/knowledge/retrieve.rs
```

```output
pub struct RetrievalOptions {
pub fn retrieve_from_bindings(
```

## 26E — CLI surface (all six flags)

```bash
./target/debug/aichat --help 2>&1 | grep -E "^\s+--knowledge(\s|-(compile|list|stat|show|search))" -A0
```

```output
      --knowledge <KB_NAME>
      --knowledge-search <QUERY>
      --knowledge-compile <KB_NAME>
      --knowledge-list
      --knowledge-stat <KB_NAME>
      --knowledge-show <KB:ID>
```

## 26E — Tool-mode dispatch

```bash
grep "SEARCH_KNOWLEDGE_NAME\|search_knowledge\|eval_search_knowledge" src/function.rs src/config/mod.rs | head -10
```

```output
src/function.rs:pub const SEARCH_KNOWLEDGE_NAME: &str = "search_knowledge";
src/function.rs:    /// Phase 26E: synthetic `search_knowledge` tool. Injected when the active
src/function.rs:    pub fn search_knowledge() -> Self {
src/function.rs:            name: SEARCH_KNOWLEDGE_NAME.to_string(),
src/function.rs:        // Phase 26E: Handle search_knowledge synthetic tool.
src/function.rs:        if self.name == SEARCH_KNOWLEDGE_NAME {
src/function.rs:            return self.eval_search_knowledge(config);
src/function.rs:    /// Phase 26E: handle `search_knowledge` synthetic tool calls. Resolves
src/function.rs:    fn eval_search_knowledge(&self, config: &GlobalConfig) -> Result<Value> {
src/function.rs:                "note": "search_knowledge called with empty query",
```

## Unit tests across 26A–26F

```bash
cargo test --bin aichat -- knowledge:: 2>&1 | grep "^test result: ok\." | tail -1 | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

Role-level tests (Phase 26C) live under `config::role::tests::test_knowledge_*` — 9 tests covering bare string / list / object frontmatter shapes and round-trip export.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```
