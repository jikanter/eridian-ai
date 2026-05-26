# Phase 35: Knowledge-MCP Protocol : Overview - Epic 14

**Status (2026-05-25):** **Planned — design draft.** No items below are implemented. This phase ships an `aichat knowledge-mcp serve <kb>` subcommand that translates Anthropic's `memory_20250818` operation set onto the existing typed `KnowledgeStore` API. Implements Theme 1 of [`260524_anthropic_memory_divergence.md`](https://github.com/jikanter/aichat-private/) (Posture C "compose"). Sibling to [Phase 34](phase-34-overview.md) (Auto-Memory) under Epic 14.

| Item | Description | Status |
|---|---|---|
| 35A | `aichat knowledge-mcp serve <kb>` subcommand surface — argcfile entry + CLI flag wiring in [`src/knowledge/cli.rs`](../../src/knowledge/cli.rs) | Planned (design draft) |
| 35B | Protocol shim — map every `memory_20250818` op (`view`, `create`, `str_replace`, `delete`, `insert`, `rename`) onto a [`KnowledgeStore`](../../src/knowledge/store.rs) method | Planned (design draft) |
| 35C | System-prompt mandate — emit the "view memory first" instruction shim so Anthropic-API clients using the memory tool behave correctly against an aichat backend | Planned (design draft) |
| 35D | AEVS gating on writes — every `create` / `str_replace` / `rename` passes through the existing AEVS restore-check before being committed to the store | Planned (design draft) |

## Background

Anthropic's `memory_20250818` tool is a *protocol*, not a service: Claude emits `view` / `create` / `str_replace` / `insert` / `delete` / `rename` ops against a virtual `/memories` directory, and the host application executes them against whatever backend it chooses. aichat ships none of this surface. There is no `/memories` namespace, no protocol shim, no system-prompt mandate to view memory first, and no client-side adapter that would let an LLM running through aichat use Anthropic's memory tool against aichat's own storage. The closest existing analog is [`src/mcp.rs:31`](../../src/mcp.rs) (`AichatMcpServer`), which exposes aichat's roles and pipelines as MCP tools — but the exposure surface is currently behaviour-shaped, not memory-shaped. See divergence Theme 1 (`[dom-ai-00020]`) for the full framing.

The ideation move is small and high-leverage: ship `aichat knowledge-mcp serve <kb>` as a *separate* MCP server (called `KnowledgeMcpServer`, distinct from the existing `AichatMcpServer`) that translates Anthropic's op set onto the existing [`src/knowledge/store.rs`](../../src/knowledge/store.rs) API. The mapping is direct because the typed-store primitives — `append_fact_with_reason`, `patch_fact`, `deprecate_fact` with `RevisionEntry` audit ([`src/knowledge/store.rs:263-339`](../../src/knowledge/store.rs)) — are exactly what `memory_20250818` ops need underneath.

**The benefit is composition.** Any Anthropic-API integrator already using the memory tool can point it at aichat's typed-store discipline — `FactId` dedup, AEVS restore-check, append-only audit — without changing client code. This is the convergence-doc third-move recommendation given a concrete protocol specification: "position knowledge/ as a shippable component."

## Disambiguation: two MCP servers

This phase introduces a **second** MCP server distinct from the existing one. To avoid future confusion:

| Server | Source | Exposes | Audience |
|---|---|---|---|
| `AichatMcpServer` (existing) | [`src/mcp.rs:31`](../../src/mcp.rs) | aichat roles, pipelines, agents as MCP tools | LLM clients that want to *call into aichat behaviour* |
| `KnowledgeMcpServer` (this phase) | new `src/knowledge/mcp.rs` | A single `KnowledgeStore` as a `memory_20250818`-shaped virtual `/memories` directory | LLM clients that already speak the memory-tool protocol and want a typed/audited backend |

The two share no code beyond MCP transport plumbing. They serve different protocols (aichat-native MCP tool vocabulary vs Anthropic's `memory_20250818` op set) and different audiences. Confusing them would lead to layering violations — `AichatMcpServer` proxies role *behaviour*; `KnowledgeMcpServer` is a *typed-store backend*.

## Design tenets

1. **The mapping is direct.** Every `memory_20250818` op has exactly one `KnowledgeStore` method behind it. No translation logic, no impedance-matching layer. If the mapping is not direct (or surfaces an ambiguity), the design is wrong.
2. **Audit is non-negotiable on writes.** Every `create` / `str_replace` / `rename` emits a `RevisionEntry` with the originating MCP client's name as `revision_reason`. The protocol is the audit boundary — no silent commits.
3. **AEVS gates writes, not reads.** `view` is cheap and never gated. Writes pass through the existing AEVS restore-check before being committed; on failure the MCP op returns an error and the store is unchanged.
4. **`delete` is soft.** Per Anthropic semantics, `delete` removes the fact from `view` output but preserves the audit row. Maps to `deprecate_fact`, not row removal.
5. **System-prompt mandate is the client's job, not the server's.** 35C ships an opt-in *instruction shim* — text the LLM-side client injects when configuring the memory tool against this server. The server does not modify model behaviour.

## 35A Design — Subcommand surface

CLI mock (per [`src/knowledge/cli.rs`](../../src/knowledge/cli.rs) argc-derive style):

```
aichat knowledge-mcp serve <kb-name>
        [--transport stdio|sse]           # default: stdio
        [--bind 127.0.0.1:PORT]            # required if --transport sse
        [--readonly]                        # disables create/str_replace/delete/insert/rename
        [--instruction-shim]                # print the system-prompt shim to stdout and exit
        [--audit-tag <tag>]                 # included in every RevisionEntry reason
```

Default invocation:

```
$ aichat knowledge-mcp serve project-facts
{"jsonrpc":"2.0","method":"initialized",...}
```

The server reads `~/.config/aichat/knowledge/<kb-name>/manifest.yaml` to locate the store, opens it, and serves over stdio. The KB must exist (Phase 25/27 manages KB lifecycle); this subcommand does *not* create KBs.

**Files:** [`src/knowledge/cli.rs`](../../src/knowledge/cli.rs) (subcommand declaration), new `src/knowledge/mcp.rs` (the server implementation), `Argcfile.sh` (if subcommand surfacing requires it).

## 35B Design — Op-by-op mapping

The full table (deep design lives in [`phase-35-knowledge-mcp.md`](phase-35-knowledge-mcp.md)):

| `memory_20250818` op | `KnowledgeStore` method | Notes |
|---|---|---|
| `view` | [`KnowledgeStore::live_facts()`](../../src/knowledge/store.rs) + [`query::query("")`](../../src/knowledge/query.rs) | Returns descriptions; empty-string query returns all live facts. |
| `create` | [`KnowledgeStore::append_fact_with_reason`](../../src/knowledge/store.rs) | Reason includes the MCP client name and `--audit-tag` value. |
| `str_replace` | [`KnowledgeStore::patch_fact`](../../src/knowledge/store.rs) | The `old_str` / `new_str` pair is applied against the fact's `description` field. |
| `delete` | [`KnowledgeStore::deprecate_fact`](../../src/knowledge/store.rs) | Soft delete — `RevisionEntry` row persists, fact disappears from `live_facts()`. |
| `insert` | [`KnowledgeStore::append_fact_with_reason`](../../src/knowledge/store.rs) + ordering metadata | Position is recorded in the fact's frontmatter; ordering is best-effort (the store is not list-ordered natively). |
| `rename` | [`KnowledgeStore::patch_fact`](../../src/knowledge/store.rs) on the `entity` field | Treats the entity as the file path in Anthropic's mental model. |

**Files:** new `src/knowledge/mcp.rs` (the dispatch table), [`src/knowledge/store.rs`](../../src/knowledge/store.rs) (no changes — existing primitives suffice).

## 35C Design — System-prompt mandate

Anthropic's recommendation for memory-tool clients is to inject a system-prompt fragment instructing the model to "view memory before responding." This phase ships the shim text as an opt-in CLI output:

```
$ aichat knowledge-mcp serve project-facts --instruction-shim
You have access to a memory tool backed by a typed knowledge store. Before
answering, call the memory tool's `view` operation to see what is already
known about the user's project. Append new facts via `create`, correct
existing facts via `str_replace`, and soft-delete obsolete facts via
`delete`. Every mutation is audited; do not write facts you cannot justify.
```

The shim is *not* injected automatically into the served session — that would violate the "server does not modify model behaviour" tenet. The client is expected to read it once (via `--instruction-shim`) and add it to whatever system-prompt construction logic they already use. This keeps the server narrow and the protocol contract pure.

**Files:** `src/knowledge/mcp.rs` (`--instruction-shim` flag handling); the shim text itself ships as a `const &str` inline in the file.

## 35D Design — AEVS gating

The existing [`src/knowledge/restore.rs`](../../src/knowledge/restore.rs) AEVS (Append-Edit-Validate-Soft-delete) restore ladder runs on every mutation through `--knowledge-curate`. 35D extends the gate to MCP writes: before `append_fact_with_reason` / `patch_fact` / `deprecate_fact` commits, the AEVS restore-check fires. On failure the MCP op returns:

```json
{
  "error": {
    "code": "aevs_restore_failed",
    "message": "fact 'X' triggered restore-ladder rung 3 (semantic-overlap); not committed",
    "rung": 3,
    "candidate_id": "...",
    "blocking_existing_id": "..."
  }
}
```

The client decides what to do — surface the conflict to the LLM, retry with a `str_replace` instead of `create`, or abort. The store is unchanged.

This is the rung that makes `KnowledgeMcpServer` qualitatively different from a naïve filesystem-backed memory server: the typed store refuses writes that violate its consistency invariants, instead of accepting whatever the LLM emits.

**Files:** new `src/knowledge/mcp.rs` (call AEVS before commit), [`src/knowledge/restore.rs`](../../src/knowledge/restore.rs) (no changes — existing ladder API suffices).

## Open questions

### 1. Transport default: stdio or SSE?

**Question:** What transport should be default — stdio (matching Claude Code's MCP client expectations) or SSE (matching the existing `AichatMcpServer` HTTP transport)?

**Recommendation: stdio default; SSE behind `--transport sse`.** Anthropic's reference memory-tool clients all speak stdio MCP first. Defaulting to stdio matches the path of least friction for the highest-leverage audience (anyone already using `memory_20250818` with Claude API). SSE is shippable in the same PR but exists as the secondary transport for HTTP-only consumers.

### 2. KB scope: one KB per server invocation, or multi-KB routing?

**Question:** Should one `knowledge-mcp serve` invocation handle multiple KBs (routed by path prefix in the `view` op), or strictly one KB per process?

**Recommendation: one KB per process; multi-KB routing is out of scope.** One-per-process matches Unix discipline ("one tool per job" from `CLAUDE.md`). Multi-KB routing introduces ambiguity in `view` (which KB? all of them?) and conflicts with Anthropic's flat `/memories` mental model. Users who need multiple KBs run multiple `knowledge-mcp serve` processes. The trade is operational complexity vs protocol cleanliness; protocol cleanliness wins.

### 3. `insert` semantics with no native ordering

**Question:** `KnowledgeStore` is not list-ordered. How should `insert` (which Anthropic's protocol assumes is positional) behave?

**Recommendation: best-effort positional metadata; document the divergence.** `insert` stores the requested position in fact frontmatter (`position: 3`) but does not enforce iteration order on `view`. The server emits a one-time warning to stderr the first time `insert` is called per process: `warn: insert position is advisory; KnowledgeStore returns facts in append-time order`. Anthropic-API clients that depend on strict positional semantics need a different backend; aichat's typed store is the wrong tool for that workload.

### 4. Deferred — `dependencies.md` / `success-metrics.md` updates

This phase does **not** update [`docs/roadmap/dependencies.md`](dependencies.md) or [`docs/roadmap/success-metrics.md`](success-metrics.md). Tracked as a follow-up doc PR with Phases 34 and 36.

## Testing

Per project guideline, the implementation PR(s) must add:

- **`tests/integration/knowledge-mcp.sh`** — bats integration test covering:
  - 35A: `aichat knowledge-mcp serve nonexistent-kb` errors with a clear "KB not found" message and exit code matching the documented `exit_code.rs` taxonomy.
  - 35A: `--instruction-shim` prints the shim and exits 0 without starting the server.
  - 35B/`view`: send an MCP `tools/call` with `memory_20250818.view`, assert the response contains all live-fact descriptions in the KB.
  - 35B/`create`: send a `create` op, assert the fact appears in a subsequent `view`, assert `revisions.jsonl` has a new entry with the expected reason.
  - 35B/`str_replace`: send a `create` followed by `str_replace`, assert the patched description in `view`, assert two `revisions.jsonl` entries (one per op).
  - 35B/`delete`: send `create` then `delete`, assert the fact is absent from `view`, assert the `RevisionEntry` row persists with `op: deprecate`.
  - 35B/`rename`: send `create` then `rename` of the entity field, assert the new entity name in `view`.
  - 35C: `--instruction-shim` output starts with the documented "You have access to a memory tool" prefix.
  - 35D: send a `create` op for a fact that violates AEVS restore (use a fixture KB with a known restore-conflicting candidate), assert the response carries `error.code = "aevs_restore_failed"` and the store is unchanged.
  - 35D: `--readonly` rejects every write op with a clear error and exit 0 on the server side.

- **Rust unit tests** in `src/knowledge/mcp.rs` for the op-dispatch table (one test per op covering the happy path) and for the AEVS-failure error envelope.

## Sequencing

- **35A and 35B should land together** (one PR). 35A without 35B is a subcommand with no implementation; 35B without 35A has no entry point.
- **35C is trivial** (a `const &str` and a flag) and can land in the same PR as 35A+35B.
- **35D depends on 35A+35B** for the call sites where AEVS gates fire. Land in a follow-up PR after the protocol shim is exercised end-to-end without gates first — this validates the dispatch table before adding the failure path.

## Files (consolidated)

- [`src/knowledge/cli.rs`](../../src/knowledge/cli.rs) — `knowledge-mcp serve` subcommand declaration
- `src/knowledge/mcp.rs` (new) — `KnowledgeMcpServer` implementation; op-dispatch table; AEVS gate; `--instruction-shim` text
- [`src/knowledge/store.rs`](../../src/knowledge/store.rs) — no changes (existing primitives suffice)
- [`src/knowledge/restore.rs`](../../src/knowledge/restore.rs) — no changes (existing AEVS ladder API suffices)
- `Argcfile.sh` — subcommand wiring if needed
- See deep design notes: [`phase-35-knowledge-mcp.md`](phase-35-knowledge-mcp.md)

## References

- Theme 1 (`[dom-ai-00020]`), Posture C (`[prj-ai-00025]`, `[prj-ai-00026]`) of the divergence playbook
- [Anthropic memory-tool docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool) — protocol spec for `memory_20250818`
- [Phase 34 overview](phase-34-overview.md) — sibling phase under Epic 14 (Auto-Memory, the freeform side of the dual-store)
- [Phase 25 knowledge compilation](phase-25-knowledge-compilation.md), [Phase 27 knowledge evolution](phase-27-knowledge-evolution.md) — the typed-store substrate this phase exposes
- [`src/mcp.rs:31`](../../src/mcp.rs) — `AichatMcpServer`, the existing MCP server (disambiguated above)