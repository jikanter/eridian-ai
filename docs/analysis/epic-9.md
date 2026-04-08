# Epic 9: RAG Evolution — Structured Retrieval & Composability

**Created:** 2026-03-16
**Updated:** 2026-04-07 (renumbered from Epic 4; phases 15-17 → 25-27)
**Status:** Planning
**Depends on:** Phase 8D (headless RAG), Phase 11C (budget-aware RAG)

---

## Motivation

AIChat's built-in RAG system (`src/rag/mod.rs`, 1029 lines) is already well-engineered: hybrid search (HNSW + BM25), reciprocal rank fusion, language-aware document splitting (18 languages), optional reranking, and batch embedding with retry. The user describes it as "incredibly easy to interact with."

The system has three categories of limitations:

1. **Structural blindness.** Chunks are flat and unrelated. A chunk that matches a query lacks the surrounding context (the function it belongs to, the section heading above it, the adjacent explanation). The splitter *detects* structural boundaries but discards the relationships.

2. **Composability gaps.** RAG cannot be used in CLI mode, as a pipeline stage, or as a tool the LLM invokes on-demand. It only works in REPL and agents. A role cannot declare its own RAG.

3. **Operational friction.** Index rebuild is perceived as expensive (even though hash-based deduplication exists). No staleness detection. No way to add a single document without opening an editor. No search-only mode. No chunk preview before committing to embedding costs.

This epic addresses all three categories while respecting the cost-conscious constraint: every improvement either reduces token spend or costs zero tokens.

---

## Feature 1: Sibling Chunk Expansion (Context Window)

### Problem

The worst failure mode of flat vector+BM25 retrieval: a chunk matches semantically but is contextually orphaned. Example: a code chunk containing `self.data.build_hnsw()` matches "HNSW rebuild" but without the surrounding `sync_documents` function, the LLM cannot reason about when or why it happens.

### Solution

During indexing, record `prev_sibling` and `next_sibling` for each chunk. During retrieval, expand top-k results by including adjacent chunks from the same file. Deduplicate, re-rank, truncate to budget.

### Implementation

**Index-time** — add to `RagDocument` or a parallel structure in `RagData`:
```rust
pub struct ChunkRelations {
    pub prev_sibling: Option<DocumentId>,
    pub next_sibling: Option<DocumentId>,
}
```

In `sync_documents()` (after `split_documents` at line 470-478 of `rag/mod.rs`), chunks are already produced in document order. Recording sibling links is bookkeeping:
```rust
for i in 0..chunks.len() {
    relations[i].prev_sibling = if i > 0 { Some(chunk_ids[i-1]) } else { None };
    relations[i].next_sibling = if i + 1 < chunks.len() { Some(chunk_ids[i+1]) } else { None };
}
```

**Search-time** — after `hybird_search` returns top-k results (line 545-580), expand:
```rust
let mut expanded = seed_ids.clone();
for id in &seed_ids {
    if let Some(prev) = relations[id].prev_sibling { expanded.push(prev); }
    if let Some(next) = relations[id].next_sibling { expanded.push(next); }
}
expanded.dedup();
// Re-rank expanded set against original query, truncate to budget
```

**Token impact:** If top_k=5, expansion produces at most 15 chunks (5 seeds × 3-chunk windows). After dedup (adjacent seeds share siblings), typically 8-10 unique chunks. At ~250 tokens/chunk, that is ~2-2.5K tokens. Budget-aware retrieval (Phase 11C) caps this.

### Files to Modify

| File | Change |
|---|---|
| `src/rag/mod.rs` | Add `ChunkRelations` struct; populate during `sync_documents`; expand during search |

### Effort

Small. ~50 lines. Zero new dependencies. Zero LLM cost during indexing.

---

## Feature 2: Metadata-Enriched Chunks

### Problem

`RagDocument.metadata` (`IndexMap<String, String>`) exists but only stores `__extension__`. Chunks carry no structural context: no heading, no function name, no line numbers. The splitter detects structural boundaries but discards them.

### Solution

Populate metadata during splitting. For each chunk, record the structural context that the splitter already knows.

### Implementation

**Markdown files** — track heading hierarchy during split:
```rust
metadata.insert("heading".into(), current_heading.clone());
metadata.insert("section".into(), parent_heading.clone());
```

The Markdown separator list (`\n## `, `\n### `, etc.) already triggers splits at headings. A small state machine tracking the most recent heading at each level adds ~20 lines to the splitter.

**Code files** — extract enclosing symbol name:
```rust
// When the Rust splitter splits on "\nfn ", the text immediately after is the function name
if let Some(fn_name) = extract_function_name(chunk_text, language) {
    metadata.insert("symbol".into(), fn_name);
}
```

For the 18 supported languages, simple regex extraction of the first function/class definition in each chunk. Not AST-level accuracy, but sufficient for metadata.

**All files** — line range and chunk index:
```rust
metadata.insert("line_start".into(), line_start.to_string());
metadata.insert("line_end".into(), line_end.to_string());
metadata.insert("chunk_index".into(), chunk_index.to_string());
```

The splitter already tracks byte offsets for overlap. Converting to line numbers is cheap (count newlines).

**Source attribution in assembled context** — prefix each chunk:
```
[src/rag/mod.rs, fn hybird_search, lines 524-590]
chunk text here...

[docs/ROADMAP.md, section "Phase 11"]
another chunk text here...
```

This costs ~5 tokens per chunk but massively improves the LLM's ability to cite sources.

### Files to Modify

| File | Change |
|---|---|
| `src/rag/splitter/mod.rs` | Extract heading, symbol, line range during splitting |
| `src/rag/mod.rs` | Include metadata prefix in assembled context string |

### Effort

Medium. ~80 lines in splitter, ~15 lines in context assembly. Zero new dependencies.

---

## Feature 3: RAG on Roles (Declarative Binding)

### Problem

Only agents can own a RAG. Roles — which have schema validation, pipelines, lifecycle hooks, inheritance, and MCP binding — cannot. Users who want RAG + these features must build agents, even though agents lack them (per the feature matrix in the roadmap).

### Solution

Add `rag:` field to role frontmatter. When a role declares `rag: my-docs`, the runtime loads the RAG and injects context automatically.

### Implementation

**Role frontmatter** (`src/config/role.rs`):
```yaml
---
model: claude:claude-sonnet-4-6
rag: codebase-docs
output_schema: {...}
---
Review code against project conventions.
__INPUT__
```

Add `rag: Option<String>` to the `Role` struct (alongside `mcp_servers`, `use_tools`, etc.).

**Context injection** (`src/config/input.rs:use_embeddings()`):
```rust
// Current: checks config.rag only
// New: check role.rag() first, then config.rag
let rag_name = input.role().rag().or_else(|| config.rag.as_ref().map(|r| r.name()));
```

**Pipeline integration** (`src/pipe.rs:run_stage_inner()`):
Add `input.use_embeddings(abort_signal).await?` before the LLM call (currently missing — RAG never runs in pipeline stages). Each stage's role determines whether RAG is used, which RAG, and with what configuration.

### Files to Modify

| File | Change |
|---|---|
| `src/config/role.rs` | Add `rag: Option<String>` to Role struct and frontmatter parsing |
| `src/config/input.rs` | Check role.rag() before config.rag in `use_embeddings()` |
| `src/pipe.rs` | Add `use_embeddings()` call in `run_stage_inner()` |
| `src/config/mod.rs` | Load and cache RAG instances referenced by roles |

### Effort

Small-medium. ~60 lines. This is the single most impactful composability change — it unlocks RAG in pipelines, CLI roles, and schema-validated workflows.

---

## Feature 4: CLI RAG and Search-Only Mode

### Problem

RAG is REPL/agent-only. No CLI mode. No way to retrieve without generating. No search-only mode.

### Solution

`--rag name` flag for CLI mode. `--rag-search` flag for retrieval-only (no LLM call).

### Implementation

**CLI RAG** — already partially wired (`cli.rs:121-122`, `main.rs:261-263`). The `--rag` flag exists. The gap: `use_rag()` in `config/mod.rs:1587` bails when the RAG doesn't exist. Fix: allow pre-existing RAGs in CLI mode; fail with helpful error if not found.

```bash
# Standard query with RAG context
aichat --rag my-docs -r analyst "What is the auth flow?"

# RAG + file input (additive context)
aichat --rag my-docs -f schema.sql "How does users table relate to auth?"

# RAG + pipeline
aichat --rag my-docs --stage extract --stage summarize "auth flow"

# RAG + batch (each record gets its own RAG search)
cat queries.txt | aichat --rag my-docs -r analyst --each
```

**Search-only mode** — bypass LLM entirely:
```bash
# Returns raw chunks, no LLM call
aichat --rag my-docs --rag-search "auth flow"

# JSON output for machine consumption
aichat --rag my-docs --rag-search -o json "auth flow"
```

JSON output:
```json
[
  {"source": "docs/auth.md", "chunk_id": "3-2", "score": 0.87, "content": "The auth flow begins with..."},
  {"source": "docs/oauth.md", "chunk_id": "1-0", "score": 0.72, "content": "OAuth2 token exchange..."}
]
```

**REPL search-only** — new `.rag search` command:
```
> .rag search "auth flow"
[0.87] docs/auth.md (3-2): "The auth flow begins with..."
[0.72] docs/oauth.md (1-0): "OAuth2 token exchange..."

> .rag ask       # Send last search results to LLM
```

### Files to Modify

| File | Change |
|---|---|
| `src/cli.rs` | Add `--rag-search` flag |
| `src/main.rs` | Search-only code path (call `Rag::search()` directly, format, print, exit) |
| `src/config/mod.rs` | Fix `use_rag()` to work in CLI mode for pre-existing RAGs |
| `src/repl/mod.rs` | Add `.rag search` and `.rag ask` commands |

### Effort

Medium. ~120 lines total across files.

---

## Feature 5: Incremental Indexing

### Problem

Perceived as "full rebuild required for any change." In reality, `sync_documents()` (line 328) already does hash-based deduplication — unchanged files skip re-embedding. But HNSW and BM25 indices rebuild from scratch every time (lines 517-519).

### Solution

Incremental HNSW insertion on append. Full rebuild only on deletion. BM25 always rebuilds (CPU-only, fast).

### Implementation

The `hnsw_rs` crate supports `parallel_insert` on an existing index. The current code at line 793 creates a new HNSW every time.

```rust
// In sync_documents, after data.add() and data.del():
let had_deletions = !to_delete_file_ids.is_empty();
let had_additions = !new_embeddings.is_empty();

// BM25 always rebuilds (cheap, CPU-only, no incremental API)
if had_deletions || had_additions {
    self.bm25 = self.data.build_bm25();
}

// HNSW: incremental insert on append, full rebuild on delete
if had_deletions {
    self.hnsw = self.data.build_hnsw();  // must rebuild (no delete API)
} else if had_additions {
    // Insert only new vectors into existing HNSW
    let new_points: Vec<_> = new_document_ids.iter()
        .zip(new_embeddings.iter())
        .map(|(id, vec)| (vec.as_slice(), id.0))
        .collect();
    self.hnsw.parallel_insert(&new_points);
}
```

**Additional UX improvements:**
- Rename `.rebuild rag` to `.rag sync` (or alias both)
- Add `.rag add <path>` — append to `document_paths`, sync only new files
- Add `.rag rm <path>` — remove from `document_paths`, sync with deletions
- Add staleness detection on load: compare file mtimes, warn if stale

### Files to Modify

| File | Change |
|---|---|
| `src/rag/mod.rs` | Conditional HNSW rebuild; incremental insert path |
| `src/config/mod.rs` | New `.rag add`, `.rag rm`, `.rag sync` commands |
| `src/repl/mod.rs` | Wire new REPL commands |

### Effort

Medium. ~100 lines for the incremental logic, ~60 lines for new commands.

---

## Feature 6: Binary Vector Storage

### Problem

Vectors are stored as Base64-encoded floats in YAML (`src/rag/serde_vectors.rs`). For a 50K-chunk RAG with 768-dim embeddings, the `rag.yaml` file is ~150MB of Base64. Loading requires full deserialization on every startup.

### Solution

Split storage: `rag.yaml` for metadata (small), `rag.bin` for vectors (binary, memory-mappable).

### Implementation

**Save path:**
```rust
impl Rag {
    pub fn save(&self) -> Result<bool> {
        // Save metadata YAML (without vectors — #[serde(skip)])
        let yaml = serde_yaml::to_string(&self.data)?;
        fs::write(&self.path, yaml)?;

        // Save vectors as binary sidecar
        let bin_path = Path::new(&self.path).with_extension("bin");
        let mut file = BufWriter::new(fs::File::create(&bin_path)?);
        for (id, vec) in &self.data.vectors {
            file.write_all(&id.0.to_le_bytes())?;
            file.write_all(&(vec.len() as u32).to_le_bytes())?;
            let bytes: &[u8] = bytemuck::cast_slice(vec.as_slice());
            file.write_all(bytes)?;
        }
        Ok(true)
    }
}
```

**Load path:** On load, check for `.bin` sidecar. If present, read binary. If absent, fall back to Base64-in-YAML (backward compatible). On save, always write new format.

**Dependency:** `bytemuck` for safe `&[f32]` → `&[u8]` conversion (zero-copy, no unsafe needed). Or use `memmap2` for memory-mapped access (lazy loading, OS-managed caching). Both are tiny crates.

### Files to Modify

| File | Change |
|---|---|
| `src/rag/mod.rs` | Binary save/load path in `Rag::save()` and `Rag::load()` |
| `src/rag/serde_vectors.rs` | Mark as legacy fallback; new binary path bypasses serde |
| `Cargo.toml` | Add `bytemuck` (or `memmap2`) |

### Effort

Small-medium. ~80 lines. Backward compatible.

### Performance Impact

| Corpus size | YAML load time | Binary load time | File size |
|---|---|---|---|
| 10K chunks, 768-dim | ~2s | ~50ms | 25MB → 30MB bin |
| 50K chunks, 768-dim | ~15s | ~200ms | 150MB → 150MB bin |
| 100K chunks, 1536-dim | ~60s | ~500ms | 800MB → 600MB bin |

---

## Feature 7: Multi-RAG Search

### Problem

Only one RAG can be active at a time. Users with separate indices (API docs, codebase, design specs) cannot query across them.

### Solution

Allow roles and CLI to reference multiple RAGs. Search each independently, merge via RRF.

### Implementation

**Role frontmatter** — `rag:` accepts string or list:
```yaml
rag: api-docs                    # single RAG (backward compatible)
rag: [api-docs, codebase, specs] # multi-RAG
rag:                             # weighted multi-RAG
  - name: api-docs
    weight: 1.5
  - name: forum-posts
    weight: 0.8
```

Use serde `untagged` enum (pattern already used in `VariableDefault`).

**CLI** — multiple `--rag` flags:
```bash
aichat --rag api-docs --rag codebase "How does auth use the OAuth endpoint?"
```

**Search** — federated with RRF merge:
```rust
let mut all_ranked_lists = Vec::new();
let mut all_weights = Vec::new();
for rag_binding in &rag_bindings {
    let (ids, _) = rag_binding.rag.search(query, top_k, ...).await?;
    all_ranked_lists.push(ids);
    all_weights.push(rag_binding.weight);
}
let merged = reciprocal_rank_fusion(all_ranked_lists, all_weights, top_k);
```

The existing `reciprocal_rank_fusion()` (line 1005-1028 of `rag/mod.rs`) already accepts `Vec<Vec<DocumentId>>` and `Vec<f32>` weights. It works unchanged.

**Cross-model safety:** Different RAGs may use different embedding models. RRF operates on rank positions, not raw scores, so cross-model results merge correctly.

### Files to Modify

| File | Change |
|---|---|
| `src/config/role.rs` | `rag:` field accepts String or Vec<RagBinding> |
| `src/config/input.rs` | `use_embeddings()` iterates multiple RAGs, merges via RRF |
| `src/config/mod.rs` | Load multiple RAGs from role config |
| `src/cli.rs` | Accept multiple `--rag` flags |

### Effort

Medium. ~100 lines. Leverages existing RRF infrastructure.

---

## Feature 8: Chunk-Adjacency Graph

### Problem

Beyond immediate siblings (Feature 1), chunks may reference other chunks via links, imports, or cross-references. These relationships are invisible to flat retrieval.

### Solution

Build a lightweight graph during indexing from deterministically extractable references: markdown links, import statements, file cross-references. Use graph expansion to augment retrieval candidates.

### Implementation

**Index-time extraction** — in `sync_documents()`, after splitting:
```rust
// Extract references from chunk text
let references = extract_references(&chunk.page_content, &file.path);
// Types: markdown links [text](target), import statements, URL references
```

Extraction is regex-based, per-language. No AST parser, no LLM call.

**Storage** — add to `RagData`:
```rust
pub graph_edges: Vec<(DocumentId, DocumentId, f32)>,  // (from, to, weight)
```

Serialized in `rag.yaml` alongside files. For a 10K-chunk corpus with ~3 edges/chunk, this is ~30K tuples, ~1MB serialized.

**Search-time expansion** — after hybrid search returns seeds:
```rust
let expanded = graph_expand(&seed_ids, &graph_edges, max_expansion: top_k);
// Merge expanded candidates with seeds
// Re-rank via RRF or reranker
// Truncate to budget
```

Graph expansion is 1-hop only (configurable). For each seed, fetch its graph neighbors. The expanded set is capped at 2× top_k to bound token cost.

### Files to Modify

| File | Change |
|---|---|
| `src/rag/mod.rs` | Reference extraction during indexing; graph expansion during search |
| `Cargo.toml` | Add `petgraph` (optional — could use simple adjacency list instead) |

### Effort

Medium-large. ~200 lines for extraction + graph + expansion. `petgraph` is optional — a `Vec<(DocumentId, DocumentId)>` suffices for 1-hop expansion.

---

## Feature 9: RAG as LLM Tool (Agent-Directed Search)

### Problem

When RAG is active in an agent, context is injected on *every turn* — even turns where the LLM doesn't need it. This wastes embedding API calls and context window space in multi-turn sessions.

### Solution

For agents, expose RAG as a callable tool instead of automatic injection. The LLM decides when to search.

### Implementation

In `select_functions()` (`src/config/mod.rs:1780+`), when the active entity has a RAG, inject a synthetic tool:
```rust
FunctionDeclaration {
    name: "search_knowledge".to_string(),
    description: format!("Search the '{}' knowledge base for relevant information", rag.name()),
    parameters: json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Search query"}
        },
        "required": ["query"]
    }),
}
```

In `eval_tool_calls()` (`src/function.rs`), add a dispatch case:
```rust
if call.name == "search_knowledge" {
    let query = call.arguments["query"].as_str().unwrap_or("");
    let (chunks, _) = rag.search(query, top_k, reranker, abort_signal).await?;
    return Ok(json!({"results": chunks}));
}
```

**Backward compatibility:** Add a `rag_mode:` field to role/agent config:
```yaml
rag: my-docs
rag_mode: tool      # "inject" (current default) or "tool" (agent-directed)
```

Default is `inject` for backward compatibility. `tool` mode suppresses automatic injection and exposes the synthetic tool instead.

### Files to Modify

| File | Change |
|---|---|
| `src/config/mod.rs` | Inject synthetic RAG tool in `select_functions()` when `rag_mode: tool` |
| `src/function.rs` | Dispatch `search_knowledge` tool calls to `Rag::search()` |
| `src/config/role.rs` | Add `rag_mode: Option<String>` to role frontmatter |

### Effort

Medium. ~80 lines. Follows the existing `tool_search` pattern exactly.

---

## Feature 10: Trace Integration and Search Transparency

### Problem

Users can't see what RAG context was injected. `.sources rag` shows file paths but not content. No integration with `--trace`.

### Solution

Extend `.sources rag` with content previews. Integrate RAG search events into the Phase 8F trace system.

### Implementation

**`.sources rag` content preview:**
```
> .sources rag
docs/auth.md (3-2) [0.87]
  "The auth flow begins with an OAuth2 authorization code grant..."

docs/oauth.md (1-0) [0.72]
  "OAuth2 token exchange requires a valid client_id..."
```

Store content snippets (first 200 chars) alongside source IDs in `last_sources`.

**`--trace` integration:**
```
[rag] my-docs: 4 chunks retrieved (hybrid search, 1 embedding call)
  0.87 docs/auth.md (3-2) [vector+keyword]
  0.72 docs/oauth.md (1-0) [vector]
  0.65 docs/sessions.md (0-3) [keyword]
  0.51 docs/tokens.md (2-1) [sibling expansion]
[1] → claude-sonnet  2847tok in  423tok out  2.1s
total: 1 turn  2847tok in  423tok out  $0.012  2.1s
```

The RAG trace event fires in `use_embeddings()` before the LLM call.

### Files to Modify

| File | Change |
|---|---|
| `src/rag/mod.rs` | Store content snippets in `last_sources`; return search metadata |
| `src/config/input.rs` | Emit trace event in `use_embeddings()` |
| `src/utils/trace.rs` | New `emit_rag_search()` method (after Phase 8F lands) |

### Effort

Small. ~50 lines. Depends on Phase 8F for full trace integration.

---

## Cross-Feature Dependency Graph

```
F1 (sibling expansion) ───────────────── Independent
F2 (metadata enrichment) ─────────────── Independent
F3 (role rag: field) ─────────────────── Independent
F4 (CLI RAG + search-only) ───────────── Independent
F5 (incremental indexing) ────────────── Independent
F6 (binary storage) ──────────────────── Independent
F7 (multi-RAG) ──── depends on F3 ────── Soft dep
F8 (chunk graph) ── benefits from F1 ─── Soft dep
F9 (RAG as tool) ── benefits from F3 ─── Soft dep
F10 (trace) ──── depends on Phase 8F ─── External dep
```

**Maximum parallelism: 6 independent work streams** (F1, F2, F3, F4, F5, F6).

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Knowledge graph with entity extraction | Requires LLM calls per chunk during indexing. Violates cost-conscious constraint. |
| AST-based code dependency graph | Requires `tree-sitter` + language grammars. Significant binary bloat. Use MCP tool for code intelligence instead. |
| Semantic chunking (LLM-based boundaries) | Costs tokens per chunk. Language-aware `RecursiveCharacterTextSplitter` is 95% as good at zero cost. |
| Query expansion / HyDE | Costs an LLM call before retrieval. If needed, build as a pipeline stage, not core RAG. |
| Multi-modal RAG (image/audio) | Different embedding models, storage, retrieval. Different product. |
| Real-time file watching (fsnotify) | CLI tools are invocation-based, not daemons. Use `cron` or shell loops. |
| Distributed/sharded vector storage | If you need this, use Qdrant/Milvus. CLI tool is wrong layer. |
| Custom tokenizer (tiktoken) | Phase 11 already rejected this. Heuristic token counting suffices. |
| RAG evaluation framework | Development tool, not runtime feature. Use external benchmarks. |
| External backend integration (ChromaDB, Qdrant) | Low priority. Built-in HNSW covers CLI-scale workloads. The storage trait (if it emerges from F5/F6 refactoring) can enable this later. |

---

## Relationship to Existing Roadmap

| Epic 9 Feature | Existing Phase | Relationship |
|---|---|---|
| F1 (sibling expansion) | None | **New** |
| F2 (metadata enrichment) | None | **New** |
| F3 (role `rag:` field) | Phase 6C (unified resource binding) | **Extension** — same pattern as `mcp_servers:` per-role binding |
| F4 (CLI RAG) | Phase 8D (headless RAG) | **Extension** — 8D unblocks non-interactive RAG; F4 adds CLI flags |
| F5 (incremental indexing) | None | **New** |
| F6 (binary storage) | None | **New** |
| F7 (multi-RAG) | None | **New** |
| F8 (chunk graph) | None | **New** |
| F9 (RAG as tool) | Phase 1C (deferred tool loading) | **Extension** — same synthetic-tool pattern as `tool_search` |
| F10 (trace) | Phase 8F (interaction trace) | **Extension** — adds RAG event to existing trace system |
