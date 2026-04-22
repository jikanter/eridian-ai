# Knowledge (Native RAG)

The Knowledge feature (Native RAG) allows `aichat` to store and retrieve information from your documents efficiently. It uses a "Knowledge Compilation" approach where documents are processed into atomic Entity-Description Pairs (EDPs) and stored in a structured format.

## Storage Location

Knowledge bases (KBs) are stored in:
`<aichat-config-dir>/kb/<kb-name>/`

## On-Disk Format

A compiled KB consists of several files:

- **`manifest.yaml`**: Metadata about the KB, including the name, version, and content hashes of all source files.
- **`facts.jsonl`**: The authoritative list of extracted facts (EDPs), one per line in JSONL format.
- **`edges.jsonl`**: The authoritative graph of relationships between facts.
- **`revisions.jsonl`**: An append-only log of all changes (appends, patches, deprecations) for auditing and reflection.
- **`knowledge.yaml`** (Optional): A tag schema used to validate tags during compilation or manual fact entry.

## Fact Format (EDP)

Each fact in `facts.jsonl` follows the Entity-Description Pair model:

```json
{
  "id": "fact-1234567890abcdef",
  "entity": "Trait",
  "description": "A way to define shared behavior in Rust.",
  "tags": ["rust:core", "concept:behavior"],
  "provenance": {
    "path": "/path/to/rust-book.md",
    "byte_range": [100, 250],
    "line_range": [10, 15],
    "content_hash": "source-file-hash"
  }
}
```

## Tag Schema (`knowledge.yaml`)

The optional `knowledge.yaml` file defines allowed namespaces and values for tags.

```yaml
namespaces:
  rust: [core, async, macro]
  concept: [behavior, data, memory]
```

When present, the system validates all facts against this schema.

## Retrieval Strategy

Retrieval in the Native RAG system is a multi-stage process:
1. **Tag Filter**: Narrow down facts by specific tags.
2. **BM25 Search**: Traditional keyword-based retrieval.
3. **Graph Walk**: Follow edges in `edges.jsonl` to find related context.
4. **AEVS Restore-Check**: Verify that the extracted description still accurately reflects the source content.

## Legacy RAG Format

Before the Knowledge feature, `aichat` used a simpler vector-based RAG system. These are stored as YAML files in `<aichat-config-dir>/rag/`. This format is deprecated but may still be present in older installations.

## Commands

- **Compile a KB**: `aichat --knowledge-compile <name> -f <files...>`
- **List KBs**: `aichat --knowledge-list`
- **Show a Fact**: `aichat --knowledge-show <name>:<fact-id>`
- **Query in REPL**: `.rag <kb-name>` (Legacy command redirected to Knowledge in newer versions)

See also: [Macros](./macros.md), [Agents](./agents.md)
