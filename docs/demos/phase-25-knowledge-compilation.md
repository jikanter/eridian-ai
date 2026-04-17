# Phase 25: Knowledge Compilation

*2026-04-17T03:35:48Z by Showboat 0.6.1*
<!-- showboat-id: 1007e874-8a7b-4a7f-b4f4-ec78e6fcc3f7 -->

Phase 25 ships the Epic-9 foundation: atomic Entity-Description Pairs (EDPs), on-disk store, AEVS restore-check, LLM-driven compiler, and a CLI surface. Retrieval at query time stays cheap and deterministic (next: Phase 26). Research basis: FADER (atomic facts + BM25), AEVS (extract-then-restore grounding), Karpathy's compiled-KB pattern.

## Module layout

```bash
ls -1 src/knowledge/
```

```output
cli.rs
compile.rs
edp.rs
mod.rs
restore.rs
store.rs
tags.rs
```

## 25A — EDP + Tag schema

```bash
grep -nE "^pub (struct|enum) |^pub fn" src/knowledge/edp.rs src/knowledge/tags.rs | head -20
```

```output
src/knowledge/edp.rs:22:pub struct FactId(String);
src/knowledge/edp.rs:54:pub struct SourceAnchor {
src/knowledge/edp.rs:72:pub enum EdgeKind {
src/knowledge/edp.rs:82:pub struct EdgeRef {
src/knowledge/edp.rs:90:pub struct EntityDescriptionPair {
src/knowledge/tags.rs:18:pub struct Tag {
src/knowledge/tags.rs:72:pub struct TagSchema {
```

## 25C — AEVS restore-check ladder

```bash
grep -nE "^pub fn|^pub enum|^pub struct" src/knowledge/restore.rs
```

```output
32:pub enum RestoreStrategy {
40:pub struct RestoreOutcome {
50:pub fn restore_check(description: &str, source: &str) -> Option<RestoreOutcome> {
107:pub fn check_fact(edp: &EntityDescriptionPair, source: &str) -> Result<RestoreOutcome> {
```

Ladder: **Exact → WhitespaceTolerant → SchemaNormalized → TokenOverlap (≥70%)**. The Levenshtein step from the original design was replaced with whitespace + schema normalization; same coverage at linear cost.

## 25D — On-disk storage layout

```bash
grep -nE "pub const.*FILE|pub fn (create|load|save|append_fact|append_edge|remove_facts_by_source|tag_index|outbound_edges|set_schema)" src/knowledge/store.rs | head -15
```

```output
33:pub const MANIFEST_FILE: &str = "manifest.yaml";
34:pub const FACTS_FILE: &str = "facts.jsonl";
35:pub const EDGES_FILE: &str = "edges.jsonl";
36:pub const SCHEMA_FILE: &str = "knowledge.yaml";
123:    pub fn create(dir: impl Into<PathBuf>, name: impl Into<String>) -> Result<Self> {
145:    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
174:    pub fn save(&self) -> Result<()> {
213:    pub fn append_fact(&mut self, fact: EntityDescriptionPair) -> Result<()> {
233:    pub fn append_edge(&mut self, edge: EdgeEntry) {
243:    pub fn remove_facts_by_source(&mut self, source_path: &str) -> usize {
268:    pub fn tag_index(&self) -> IndexMap<String, Vec<FactId>> {
280:    pub fn outbound_edges(&self, fact_id: &FactId) -> Vec<&EdgeEntry> {
285:    pub fn set_schema(&mut self, schema: TagSchema) {
```

## 25B — Compiler pipeline

```bash
grep -nE "^pub fn |^pub struct |^pub const" src/knowledge/compile.rs | head -15
```

```output
46:pub const COMPILER_VERSION: &str = "v1";
48:pub struct CompileOptions {
74:pub struct EdpCandidate {
84:pub struct CompileReport {
121:pub fn commit_candidates(
186:pub fn line_range_to_bytes(source: &str, line_range: (usize, usize)) -> (usize, usize) {
221:pub fn dedupe_candidates(candidates: Vec<EdpCandidate>) -> Vec<EdpCandidate> {
```

Per-file pipeline: **manifest hash-check → StageCache hit → N-sample LLM extraction with `response_format: json_schema` → dedup by (entity, description) → commit_candidates (restore-check + append)**. Scope-deferred from the original plan: FADER question-speculation step, edge extraction, parallel compilation. Those land with Phase 26/27.

## 25E — CLI surface

```bash
./target/debug/aichat --help 2>&1 | grep -A1 "knowledge-"
```

```output
      --knowledge-compile <KB_NAME>
          Phase 25B: compile source files into a knowledge base (requires -f)
--
      --knowledge-list
          Phase 25E: list all compiled knowledge bases
--
      --knowledge-stat <KB_NAME>
          Phase 25E: show stats (fact count, tag distribution, per-source coverage) for a KB
--
      --knowledge-show <KB:ID>
          Phase 25E: show a single fact; format is `KB_NAME:FACT_ID` (e.g. `docs:fact-abc123`)
```

## Unit tests across all five items (deterministic: piped through sort)

```bash
cargo test --bin aichat -- knowledge:: 2>&1 | grep "^test result: ok\." | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 75 passed; 0 failed; 0 ignored; 0 measured; 265 filtered out; finished in Xs
```

Breakdown by submodule: 24 edp+tags, 15 store, 16 restore, 16 compile, 4 cli = **75 knowledge-module tests**.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 340 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
