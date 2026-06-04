# Phase 35: Knowledge-MCP Protocol — Deep Design

Detail companion to [`phase-35-overview.md`](phase-35-overview.md). The overview lists what 35A–35D do; this doc captures the full op-by-op protocol mapping, the error envelope, the transport choice rationale, and the composability story.

## Why this is the right shape

The convergence-doc third move is "position knowledge/ as a shippable component." Theme 1 of the divergence playbook (`[dom-ai-00020]`) gives that move a concrete protocol target: Anthropic's `memory_20250818`. The mapping is direct because aichat's typed-store primitives were designed for exactly the operations Anthropic's memory tool wants — append, patch, soft-delete, with a per-mutation audit row.

The phrasing matters: this is *not* a protocol-translation exercise that needs an impedance-matching layer. There is one op, there is one method behind it. If the protocol shim grows beyond ~200 lines of dispatch logic, the design has drifted off the direct-mapping tenet and needs to be reconsidered.

## Full subcommand surface

Per the overview's 35A section, with full flag documentation:

```
USAGE:
    aichat knowledge-mcp serve <kb-name> [OPTIONS]

ARGS:
    <kb-name>       Name of an existing KB in ~/.config/aichat/knowledge/

OPTIONS:
    --transport <stdio|sse>     Transport for MCP messages [default: stdio]
    --bind <addr:port>          Required if --transport sse; e.g. 127.0.0.1:8765
    --readonly                  Reject every write op (create, str_replace, delete,
                                insert, rename); allow view only
    --instruction-shim          Print the recommended system-prompt shim to stdout
                                and exit 0; does not start the server
    --audit-tag <tag>           String included in every RevisionEntry's reason field
                                (useful for distinguishing MCP-driven mutations from
                                CLI-driven ones in the audit log)
    --max-view-bytes <N>        Cap on bytes returned per view op [default: 65536]
    -h, --help                  Print help
```

Exit codes follow [`src/utils/exit_code.rs`](../../src/utils/exit_code.rs):

| Exit | Category | Cause |
|---|---|---|
| 0 | Success | Server exited cleanly on stdio EOF; or `--instruction-shim` printed |
| 1 | General | Unhandled error inside the dispatch loop |
| 8 | Schema validation | KB manifest is malformed; KB not found |
| 9 | Permission | `--readonly` write op rejected by the server |
| 10 | Transport | SSE bind failed (port in use, address malformed) |

## Op-by-op mapping (full)

### `view`

Anthropic input:

```json
{ "method": "tools/call", "params": { "name": "memory_20250818", "arguments": { "op": "view", "path": "/memories" } } }
```

Maps to:

```rust
let facts = store.live_facts();                   // KnowledgeStore::live_facts()
let descriptions = query::query(&store, "")?;     // empty query = return all
// Format each fact as: "<entity>: <description>"
// Truncate at --max-view-bytes
```

Response:

```json
{
  "content": [
    { "type": "text", "text": "src/foo.rs: defines the Foo trait used by..." },
    { "type": "text", "text": "src/bar.rs: implements Foo for Bar; see..." }
  ]
}
```

Empty store returns an empty content array, not an error. This is the case Anthropic's system-prompt shim (35C) explicitly handles — the LLM is expected to interpret an empty view as "no prior knowledge."

### `create`

Anthropic input:

```json
{ "op": "create", "path": "/memories/foo.md", "content": "the rationale for Foo is..." }
```

Maps to:

```rust
let edp = EntityDescriptionPair::new(
    path_to_entity(&args.path),                 // "/memories/foo.md" → "foo"
    args.content,
    vec![],                                      // no tags from this surface
);

// 35D: AEVS gate
if !store.aevs_can_commit(&edp)? {
    return Err(McpError::aevs_restore_failed(...));
}

store.append_fact_with_reason(edp, Some(format!("mcp:{} ({})", client_name, audit_tag)))?;
```

The reason field carries the originating client name (extracted from the MCP `initialize` handshake) and the optional `--audit-tag` value. The `RevisionEntry` audit row is what makes this op asymmetric with Anthropic's freeform filesystem-backed memory: every mutation is replayable.

### `str_replace`

Anthropic input:

```json
{ "op": "str_replace", "path": "/memories/foo.md", "old_str": "rationale", "new_str": "rationale (updated)" }
```

Maps to:

```rust
let fact_id = path_to_fact_id(&args.path, &store)?;
let mut current = store.get_fact(&fact_id)?;
let new_description = current.description.replace(&args.old_str, &args.new_str);

if new_description == current.description {
    return Err(McpError::no_match(...));        // old_str not found
}

// 35D: AEVS gate on the patched form
let patched = current.with_description(new_description);
if !store.aevs_can_commit(&patched)? {
    return Err(McpError::aevs_restore_failed(...));
}

store.patch_fact(&fact_id, FactPatch::description(new_description))?;
```

### `delete`

Anthropic input:

```json
{ "op": "delete", "path": "/memories/foo.md" }
```

Maps to:

```rust
let fact_id = path_to_fact_id(&args.path, &store)?;
store.deprecate_fact(&fact_id, Some(format!("mcp delete via {}", client_name)))?;
```

This is *soft* delete. The fact disappears from `live_facts()` and from `view` output, but the `RevisionEntry` row persists with `op: deprecate`. A future `--knowledge-restore <fact-id>` would surface the deprecated fact (Phase 27 feature). No AEVS gate is needed — deprecating an existing fact is always safe.

### `insert`

Anthropic input:

```json
{ "op": "insert", "path": "/memories/list.md", "line": 3, "content": "new item at line 3" }
```

Maps to the same code path as `create` but with `position: 3` recorded in the fact's frontmatter:

```rust
let mut edp = EntityDescriptionPair::new(
    path_to_entity(&args.path),
    args.content,
    vec![Tag::new("mcp_position", &args.line.to_string())],
);
// AEVS gate as in `create`
store.append_fact_with_reason(edp, ...)?;
```

The server emits a one-time stderr warning per process the first time `insert` is called: `warn: insert position is advisory; KnowledgeStore returns facts in append-time order`. Documented as a known divergence in the user-facing `docs/features/knowledge-mcp.md` doc that ships with this phase.

### `rename`

Anthropic input:

```json
{ "op": "rename", "path": "/memories/old.md", "new_path": "/memories/new.md" }
```

Maps to a `patch_fact` on the `entity` field:

```rust
let fact_id = path_to_fact_id(&args.path, &store)?;
store.patch_fact(&fact_id, FactPatch::entity(path_to_entity(&args.new_path)))?;
```

No AEVS gate — renaming an existing fact's entity does not introduce a new fact and cannot violate restore invariants.

## Error envelope

Every error response follows JSON-RPC 2.0 with an `error.code` from this taxonomy:

| `error.code` | Meaning | HTTP-equivalent |
|---|---|---|
| `kb_not_found` | KB does not exist or manifest is unreadable | 404 |
| `fact_not_found` | The path resolves to no live fact (after path→FactId lookup) | 404 |
| `no_match` | `str_replace` `old_str` not found in description | 422 |
| `aevs_restore_failed` | 35D gate fired; `rung` and `blocking_existing_id` included | 409 |
| `readonly_violation` | `--readonly` server received a write op | 403 |
| `view_truncated` | View output exceeded `--max-view-bytes`; `bytes_omitted` included | 200 (warning, not error) |
| `internal` | Unhandled store-side error; `cause` field carries the underlying message | 500 |

The full schema is in `src/knowledge/mcp.rs::McpError` (single source of truth; the integration test asserts against this enum).

## Transport choice rationale

The overview lists stdio default with SSE behind `--transport sse`. The full reasoning:

**stdio is the default because:**

- Every Anthropic-API reference client speaks stdio MCP first.
- stdio has no port-binding configuration to get wrong.
- stdio inherits the parent process's lifecycle naturally — when the parent (the LLM client) exits, the server exits.
- Output redirection / logging is straightforward (stderr is free for diagnostics).

**SSE is supported (not just stdio-only) because:**

- The existing [`src/mcp.rs:AichatMcpServer`](../../src/mcp.rs) already speaks SSE; reusing the transport layer is cheap.
- Long-running multi-client scenarios (one KB served to many concurrent LLM sessions) need a network-reachable server.
- The MCP spec is transport-agnostic; reflecting that in the implementation costs little.

The SSE path reuses the streamable-HTTP adapter at [`src/mcp_client/streamable_http.rs`](../../src/mcp_client/streamable_http.rs) inverted — what that file does for *consuming* an SSE-MCP server, this phase does for *serving* one.

## Composability story

"Any Anthropic-API integrator already using the memory tool can point it at aichat's typed-store discipline without changing client code." That's the headline. The fuller picture:

1. A developer writes a Claude-API app that uses the memory tool. They configure `memory_20250818` against an in-memory filesystem stub or `~/memories/` on their laptop.
2. They install aichat. They have one or more aichat KBs already (Phase 25 compiled them from source files).
3. They change the memory-tool configuration in their Claude-API app to point at `aichat knowledge-mcp serve project-facts` (over stdio).
4. The app now reads/writes against aichat's typed store, with full audit trail, FactId dedup, and AEVS restore gating — *without changing a single line of their Claude-API code*.

The substitution is transparent to the model — `memory_20250818` ops look identical from the LLM's perspective. The substitution is *not* transparent to the developer in two specific ways, which the user-facing doc must surface clearly:

- **Some `create` ops will return `aevs_restore_failed`** where they would have silently succeeded against a filesystem backend. The LLM must be prompted to handle this (the 35C shim does so).
- **`view` returns descriptions, not raw markdown bodies.** A filesystem-backed memory tool returns whatever was written; the typed store returns the *description* field, which is what `query.rs` uses to feed downstream retrieval. The bodies (raw frontmatter, additional sections) are not exposed via `view`. Documented as a divergence.

These trade-offs are *the point*. The developer chose typed-store discipline by pointing at this server; if they wanted filesystem semantics they would have kept the filesystem backend.

## Path → FactId resolution

Anthropic's protocol identifies memory entries by path (`/memories/foo.md`). The typed store identifies them by `FactId` (content-hash-derived). The shim maintains the mapping in-memory per server lifetime:

```rust
struct PathMap {
    by_path: HashMap<String, FactId>,
    by_id: HashMap<FactId, String>,
}
```

Populated lazily — when a `create` arrives at path `/memories/foo.md`, the resulting `FactId` is recorded. On startup, `view` is the only op that does not need a prior path; it returns descriptions tagged with their synthetic paths (`/memories/<entity>.md` where `entity` is the fact's entity field). After the first `view` the path map is fully populated.

This design has a *known limitation*: if the LLM writes `create /memories/foo.md "X"` then later `view` returns a path constructed from the fact's entity (which is `foo`, yielding `/memories/foo.md`), the round-trip works. But if the entity in the store is e.g. `foo.rs` (compiled from a Rust source file), `view` returns `/memories/foo.rs.md` and the LLM may not realise the original creation path mattered. Documented; future work could canonicalise the path-to-entity mapping.

## Testing plan

### Bats integration tests (`tests/integration/knowledge-mcp.sh`)

Full assertions per the overview's Testing section. The bats helpers should:

- Start the server as a backgrounded subprocess connected via stdio fifos.
- Use `jq` to assemble JSON-RPC requests and parse responses.
- Run against a deterministic fixture KB at `tests/fixtures/knowledge/mcp-test-kb/`.
- Tear down the server with SIGTERM in `teardown_file`.

A representative happy-path test:

```bash
@test "knowledge-mcp: create then view round-trips" {
  start_server "$FIXTURE_KB"

  send_op create '{"path":"/memories/foo.md","content":"the rationale for Foo is..."}'
  send_op view '{"path":"/memories"}'

  assert_view_contains "foo: the rationale for Foo is..."
  assert_revisions_jsonl_has_entry "op:create entity:foo reason:mcp:bats-test"
}
```

### Rust unit tests

- `src/knowledge/mcp.rs::tests::view_returns_all_live_facts`
- `src/knowledge/mcp.rs::tests::create_appends_revision_entry_with_reason`
- `src/knowledge/mcp.rs::tests::str_replace_no_match_returns_no_match_error`
- `src/knowledge/mcp.rs::tests::delete_is_soft_revision_persists`
- `src/knowledge/mcp.rs::tests::insert_emits_position_warning_once_per_process`
- `src/knowledge/mcp.rs::tests::rename_updates_entity_field`
- `src/knowledge/mcp.rs::tests::aevs_failure_returns_aevs_restore_failed`
- `src/knowledge/mcp.rs::tests::readonly_rejects_create`
- `src/knowledge/mcp.rs::tests::path_map_populates_lazily_on_view`

## Cited source ranges

- [`src/mcp.rs:31`](../../src/mcp.rs) — `AichatMcpServer`, the existing MCP server (disambiguated against `KnowledgeMcpServer` introduced here).
- [`src/knowledge/store.rs:117`](../../src/knowledge/store.rs) — `RevisionEntry` struct (the audit primitive every write op writes through).
- [`src/knowledge/store.rs:263-339`](../../src/knowledge/store.rs) — `append_fact_with_reason`, `patch_fact`, `deprecate_fact` (the three primitives the protocol maps onto).
- [`src/knowledge/restore.rs`](../../src/knowledge/restore.rs) — AEVS restore ladder (35D gate).
- [`src/knowledge/query.rs`](../../src/knowledge/query.rs) — `query("")` is the `view` backend.
- [`src/knowledge/cli.rs`](../../src/knowledge/cli.rs) — argc-derive site for the new subcommand.
- [`src/mcp_client/streamable_http.rs`](../../src/mcp_client/streamable_http.rs) — SSE transport adapter (consumed inverted by 35A `--transport sse`).
- [`src/utils/exit_code.rs`](../../src/utils/exit_code.rs) — exit-code taxonomy referenced by the server.

## References

- Theme 1 (`[dom-ai-00020]`), Theme 8 (`[dom-ai-00023]`, "MCP exposure of aichat-unique surfaces"), Posture C of the divergence playbook
- [Anthropic memory-tool protocol](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool) — the spec this phase targets
- [Phase 35 overview](phase-35-overview.md) — status table and sub-item summary
- [Phase 34 overview](archive/phase-34-overview.md) — freeform side of the dual-store
- [Phase 27 knowledge evolution](archive/phase-27-knowledge-evolution.md) — the existing Reflect/Curate surface that complements this MCP server
