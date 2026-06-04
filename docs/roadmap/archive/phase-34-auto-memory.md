# Phase 34: Auto-Memory Wiring — Deep Design

Detail companion to [`phase-34-overview.md`](phase-34-overview.md). The overview lists what 34A–34D do; this doc explains *why* the dual-store split is the right shape, *how* the Reflector wiring composes with the existing typed-knowledge surface, and *what* the security posture demands of the writer.

## Why two stores, not one

Every divergence between aichat and Claude Code's memory surface is, at root, a consequence of the storage-model choice — see Theme 11 (`[shr-ai-00025]`) of the divergence playbook. Anthropic's memory is freeform markdown; aichat's `knowledge/` is typed JSONL. The two are opposite ends of a tradeoff that no amount of cross-pollination collapses:

| Tradeoff axis | Freeform markdown (Anthropic) | Typed JSONL (aichat) |
|---|---|---|
| User ergonomics | Open in any editor, no schema to learn | Requires `--knowledge-show` to read |
| Writer flexibility | LLM writes whatever shape it wants | Constrained to `EntityDescriptionPair` |
| Transparency | The file *is* the memory | Need tooling to inspect |
| Auditability | Git history only | `revisions.jsonl` per-mutation with reason |
| Dedup | None | `FactId` content hash |
| Provenance | None | `SourceAnchor` + AEVS restore gate |
| Query precision | Filename grep | Typed tags + BM25 + RRF |

The research-backed reframe (per Theme 11 of the playbook) is that these are *complementary surfaces serving different user populations*, not competing solutions to one problem. Power users editing memory by hand want freeform markdown. Agents writing memory at scale in long-running multi-agent systems want typed-JSONL with audit logs. **aichat already ships the typed side; this phase adds the freeform side as a peer, not a replacement.** The user's workflow chooses which writer is appropriate for which insight.

This is Posture C ("compose") from the playbook's strategic-posture section (`[prj-ai-00025]`, `[prj-ai-00026]`). The fork's value proposition becomes "the only CLI where you don't have to choose between Anthropic's ergonomics and ACE's discipline."

## File layout

```
$PROJECT_ROOT/
├── memory/                       # Freeform side — this phase
│   ├── MEMORY.md                 # Index file, read on REPL/CLI startup (34A)
│   ├── rust_async_preferences.md # Topic file, lazy-loaded on reference (34B)
│   ├── feedback_cite_sources.md  # Existing topic file (already in repo)
│   └── ...
└── .aichat/                       # Typed side — Phases 25-27 (existing)
    └── knowledge/
        ├── <kb-name>/
        │   ├── manifest.yaml
        │   ├── facts.jsonl
        │   ├── edges.jsonl
        │   └── revisions.jsonl
        └── ...
```

The user-level fallback mirrors the project-local layout:

```
~/.config/aichat/
├── memory/                # User-level freeform memory (fallback)
└── knowledge/             # User-level typed KBs (existing)
```

The project-vs-user precedence is binary (project wins; user is fallback when project has no `MEMORY.md`); no merge.

## Topic-file frontmatter convention

Every Reflector-emitted topic file (34C) carries minimal YAML frontmatter so future tooling can filter by recency / session without parsing markdown bodies:

```markdown
---
created: 2026-05-25T12:04:00Z
session: session-2026-05-25-114417-vygh
turns_referenced: [3, 5, 11]
reflector_model: claude-haiku-4-5     # which model produced this candidate
curator: interactive                   # or "auto" if --memory-auto-curate
---

The user prefers `tokio::spawn` over `async_std::task::spawn` for new code
in this project. Cited rationale: "we standardize on tokio across the
codebase already."
```

Frontmatter is optional for hand-edited files — the read-on-startup path (34A) tolerates plain markdown without frontmatter. The curator gate (34D) emits frontmatter by default; the user can edit it out during the `e` (edit) branch if they want a hand-curated entry to look hand-written.

The convention is documented in `docs/features/auto-memory.md` (new user-facing doc, sibling to [`docs/features/repl-pi.md`](../../features/repl-pi.md)) when the implementation ships.

## Reflector wiring

The existing `--knowledge-reflect` Reflector lives in [`src/knowledge/evolve.rs`](../../../src/knowledge/evolve.rs) and emits typed `EntityDescriptionPair` candidates. 34C reuses the *invocation primitive* (transcript-in, structured-out, role-driven) but with a different *output schema* and a different *system prompt*.

The refactor extracts a `ReflectorJob` enum:

```rust
// Sketch — actual implementation lives in src/knowledge/evolve.rs
pub enum ReflectorJob {
    Knowledge {                              // Existing — Phase 27
        kb_name: String,
        candidates: Vec<EntityDescriptionPair>,
    },
    Memory {                                 // NEW — 34C
        candidates: Vec<MemoryCandidate>,
    },
}

pub struct MemoryCandidate {
    pub topic: String,                       // derived from recurring noun phrase
    pub frontmatter: BTreeMap<String, Value>,
    pub body: String,                        // freeform markdown
    pub turns_referenced: Vec<usize>,        // for audit trail
}
```

The `Memory` Reflector role lives at `assets/roles/_memory_reflector.md` (underscore-prefixed by aichat convention for internal-only roles, matching `_curator.md` in Phase 27). Its system prompt is shaped roughly:

```
You are extracting durable preferences and incidental learnings from a
conversation transcript that the user will want to remember in future
sessions. You emit zero or more topic files, each scoped to ONE coherent
theme.

DO NOT emit topics for:
- Single-use answers to one-shot questions (e.g. "what's the syntax for X")
- Information already captured in the project's CLAUDE.md / AGENTS.md
- Anything matching the redacted patterns below

DO emit topics for:
- Stated user preferences ("I always prefer X over Y for this project")
- Errors observed and the resolution path the user adopted
- Hints about which roles or models worked well for which task today
```

The full prompt and its iteration history will live alongside the role file when implementation lands.

## Security: secret redaction is mandatory

The Reflector writes to disk. Any secret that survives into a topic file persists across sessions and may be exfiltrated by future LLM calls that load that topic file as context. The redaction pass runs **before** the Reflector sees the transcript, on the full session message vec.

Patterns to redact (extensible — the implementation reads from `~/.config/aichat/memory/redact_patterns.yaml` with these defaults):

| Pattern | Source |
|---|---|
| `sk-[A-Za-z0-9]{20,}` | OpenAI API key shape |
| `sk-ant-[A-Za-z0-9_-]{20,}` | Anthropic API key shape |
| `xoxb-[0-9]+-[0-9]+-[A-Za-z0-9]+` | Slack bot token |
| `ghp_[A-Za-z0-9]{36}` | GitHub personal access token |
| `Bearer\s+[A-Za-z0-9._-]+` | Generic HTTP bearer token |
| `(?i)(api[_-]?key\|secret\|password\|token)\s*[:=]\s*\S+` | Generic key=value assignment |
| Custom regexes loaded from `redact_patterns.yaml` | Project-specific patterns |

Each redacted match is replaced with `[REDACTED:<category>]` so the Reflector retains enough context to understand structure ("the user set their API key here") without retaining the secret itself.

This is not defence-in-depth — it is the *only* defence. There is no second filter at the curator step (the curator displays whatever the Reflector produces). The redaction pass is therefore tested aggressively: every supported pattern has a positive and a negative test in `src/memory/reflect.rs`.

## Composability with Theme 3 (role `@path` imports)

Theme 3 of the divergence playbook (not yet ingested into the roadmap as its own phase) proposes adding `@path` imports to role `prompt:` bodies — letting a role pull in markdown files at prompt-resolution time. Once Theme 3 ships, a role can directly cite a memory topic file:

```yaml
---
name: code-review
model: claude-sonnet-4-6
---
You are reviewing a code change for this project.

@../memory/rust_async_preferences.md
@../memory/feedback_cite_sources.md

Review the diff below for adherence to the preferences above.
```

This phase's 34B (lazy-load on reference) is designed to plug into Theme 3's resolver without rework — the loader API takes a memory-root prefix and returns markdown content, which is exactly what `@path` resolution needs. The two phases are independent but compose cleanly when both ship.

## Composability with Phase 35 (Knowledge-MCP)

Phase 35 ships an MCP server that exposes the typed `knowledge/` store via Anthropic's `memory_20250818` op set. The two phases together complete the dual-writer architecture:

- **Phase 34** wires the freeform `memory/` side at the session level (one user, project-local, hand-curated or session-Reflector-curated).
- **Phase 35** wires the typed `knowledge/` side over MCP (any client implementing the memory tool can read/write typed facts with full audit).

A user could legitimately have *both* surfaces active: `memory/MEMORY.md` carries their personal preferences (34A reads on startup), and `knowledge/` carries the project's compiled facts (Phase 35 exposes as an MCP server to whatever LLM they're using). The two stores never collide because they target different writers (session-scope vs cross-client) and different schemas (freeform vs typed).

## Testing plan

### Bats integration tests (`tests/integration/auto-memory.sh`)

Per project guideline that bats integration tests accompany every feature, the implementation PR(s) must add:

- **`@test "34A: MEMORY.md content appears in system prompt"`** — populate `memory/MEMORY.md` with a known marker string, run `aichat --info -o json`, assert the marker appears in the system-prompt block.
- **`@test "34A: missing MEMORY.md does not error"`** — remove `memory/MEMORY.md`, run `aichat --info -o json`, assert exit 0 and no marker.
- **`@test "34A: truncation warning fires above 200 lines"`** — generate a 250-line `MEMORY.md`, run `aichat`, assert stderr contains the truncation warning.
- **`@test "34B: lazy-load on Reflector candidate reference"`** — stub the Reflector to emit a candidate referencing `feedback_cite_sources.md`, accept it, run a follow-up turn, assert the topic file content is in the LLM's input context.
- **`@test "34C: secret redaction replaces sk-... before Reflector"`** — instrument an echo-Reflector that returns its input verbatim as the candidate body. Feed a transcript containing `OPENAI_API_KEY=sk-test-12345`. Assert the resulting candidate contains `[REDACTED:openai_key]`, not the literal key.
- **`@test "34C: secret redaction handles every default pattern"`** — parameterised test that feeds one transcript per default pattern (OpenAI key, Anthropic key, Slack token, GitHub PAT, Bearer header, generic key=value), asserts each is redacted.
- **`@test "34D: accept writes topic file atomically"`** — run the Reflector with a known candidate, accept via stdin, assert `memory/<topic>.md` exists with the expected content and frontmatter, assert `memory/MEMORY.md` has a new index line.
- **`@test "34D: reject-all aborts cleanly with exit 0"`** — Reflector emits 3 candidates, user inputs `r` (reject-all), assert exit 0 and no files written.
- **`@test "34D: --memory-auto-curate accepts every candidate"`** — Reflector emits 2 candidates, run with `--memory-auto-curate`, assert both files written without prompts.

### Rust unit tests

- `src/memory/reflect.rs::tests::redacts_all_default_patterns` — positive coverage per pattern.
- `src/memory/reflect.rs::tests::does_not_redact_lookalike_text` — negative coverage (e.g. `sk-abc` standalone should *not* match the 20-char OpenAI pattern).
- `src/memory/reflect.rs::tests::derives_topic_name_from_noun_phrase` — golden tests for topic-name derivation.
- `src/memory/curate.rs::tests::atomic_write_survives_crash_midway` — fault-injection test using a fake filesystem that fails on second `write()` call; assert no half-written file remains.

## Cited source ranges

- [`src/config/session.rs:594-613`](../../../src/config/session.rs) — `session.compress()`, the existing client-side LLM-driven session mutation that the Reflector pattern extends.
- [`src/config/session.rs:641`](../../../src/config/session.rs) — `session.exit()`, the call site for 34C.
- [`src/config/session.rs:724`](../../../src/config/session.rs) — `add_message`, the per-turn hook where a future Theme 7 (assume-interruption JSONL log) would write.
- [`src/knowledge/evolve.rs`](../../../src/knowledge/evolve.rs) — the existing Reflector primitive 34C factors against.
- [`src/knowledge/store.rs:117`](../../../src/knowledge/store.rs) — `RevisionEntry` struct (memory writes do not use this; cited for comparison — `memory/` deliberately does *not* gain a per-mutation log; git history is the audit substrate).
- [`memory/MEMORY.md`](../../../memory/MEMORY.md) — the existing index file (currently unread).
- [`docs/analysis/open-memory/claude-code.md`](../../analysis/open-memory/claude-code.md) — Claude Code precedent for the `memory/` discipline.

## References

- Theme 2 (`[prj-ai-00023]`), Theme 11 (`[shr-ai-00025]`), Posture C (`[prj-ai-00025]`, `[prj-ai-00026]`) of the divergence playbook
- [Phase 34 overview](phase-34-overview.md) — status table and sub-item summary
- [Phase 35 overview](../phase-35-overview.md) — typed side of the dual-store
- [Phase 27 knowledge evolution](phase-27-knowledge-evolution.md) — the typed Reflector this phase factors against