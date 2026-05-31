# Phase 34: Auto-Memory Wiring : Overview - Epic 14

**Status (2026-05-30):** **34A–34D Done** (read-only surface + topic-file lazy-load + Reflector/Curator write loop shipped). This phase fills the agent-writer slot at the session level by wiring a freeform `memory/` surface alongside the typed `knowledge/` store. Implements Theme 2 of [`260524_anthropic_memory_divergence.md`](https://github.com/jikanter/aichat-private/) (Posture C "compose"). Pairs with [Phase 35](phase-35-overview.md) (Knowledge-MCP) — together they form Epic 14's dual-writer architecture. User-facing doc: [`docs/features/auto-memory.md`](../features/auto-memory.md); demo: [`docs/demos/phase-34-auto-memory.md`](../demos/phase-34-auto-memory.md).

| Item | Description | Status |
|---|---|---|
| 34A | Read `memory/MEMORY.md` at REPL/CLI startup as additional system-prompt context — mirrors Claude Code's first-200-lines auto-load discipline. Read-only. | **Done** (2026-05-30) |
| 34B | Topic-file lazy loading — a `memory:<ref>` path (resolved against `MEMORY.md` links or topic filenames) expands to the topic file at `Input::from_files`. `--memory-load <ref>` prints a resolved topic. | **Done** (2026-05-30) |
| 34C | Session-exit Reflector pass — `--memory-reflect` / `--memory-reflect-on-exit` redact secrets, then emit candidate `memory/<topic>.md` files from the transcript, reusing the [`src/knowledge/evolve.rs`](../../src/knowledge/evolve.rs) Reflector pattern (`*-memory-reflector` role). | **Done** (2026-05-30) |
| 34D | Curator gate — `--memory-curate` prompts `[a]ccept [s]kip [e]dit [r]eject-all` before atomic-writing topic files + appending to `MEMORY.md`; `--memory-auto-curate` accepts all. Closes the dual-writer loop. | **Done** (2026-05-30) |

## Background

The `memory/` directory at the aichat repo root contains `MEMORY.md` (an index of feedback topics) and `feedback_cite_sources.md` (one topic file). A grep across `src/` returns zero references — the directory is forward-intent, not code. The 2026-05-24 commits `57f3247` ("memory discovery") and `3c8a395` ("open memory descriptions") added [`docs/analysis/open-memory/`](../analysis/open-memory/) notes documenting the Claude Code precedent (`~/.claude/projects/<path>/memory/`), but no consumer or producer is wired.

Anthropic's two-writer split — humans write CLAUDE.md, the agent writes auto-memory — maps to aichat as **(humans write CLAUDE.md / AGENTS.md / role YAMLs, agent writes ???)**. The agent-writer slot is vacant at the session level. The `knowledge/` store fills the slot *for typed citable facts compiled from source files*, but it does not fill it for the running session's incidental learnings — preferences the user expressed mid-conversation, errors observed once and not worth a full KB entry, hints about which roles are working well today. Anthropic's auto-memory is precisely the freeform-notes layer for this category. See divergence Theme 2 (`[prj-ai-00023]`) for the full framing.

The phase splits into two halves that can ship independently. **34A + 34B** is the *minimal* option (read-only consumption — aichat treats `memory/` as a startup-instruction layer). **34C + 34D** is the *maximal* option (write loop — session-exit Reflector emits candidate topic files, Curator gates persistence). The minimal half is shippable in ~3 days; the maximal half is gated on a separate design review of the Reflector prompt and curation flow.

## Design tenets

1. **Two stores, one ergonomics surface.** Freeform `memory/` for incidental learnings, typed `knowledge/` for citable facts. The user picks the writer; both stores are first-class. No silent promotion from one to the other.
2. **Read-on-startup is cheap; write-on-exit is auditable.** 34A injects `MEMORY.md` content as system-prompt context, costing one file read per REPL launch. 34C writes through the existing append-only `revisions.jsonl` discipline so every memory write is replayable.
3. **Curator gate is non-negotiable for writes.** A silent write loop violates the audit-by-default posture. Default is interactive prompt; `--memory-auto-curate` is opt-in, not default.
4. **Secrets do not leak.** The Reflector prompt explicitly excludes turns containing API-key-shaped tokens, `OPENAI_API_KEY=`-style assignments, and any value the session already redacted in `info` output.
5. **`memory/` location is project-local first.** Default path is `$PROJECT_ROOT/memory/`, fallback `~/.config/aichat/memory/` (per-user). Mirrors the `knowledge/` store's project-then-user lookup chain.

## Dual-store table

| Surface | `memory/` (this phase) | `knowledge/` (Phases 25–27) |
|---|---|---|
| Storage | Markdown + YAML frontmatter, freeform | Typed JSONL (`facts.jsonl`, `edges.jsonl`, `revisions.jsonl`) + `manifest.yaml` |
| Writer | Session Reflector (34C) | `--knowledge-reflect` / `--knowledge-curate` (Phase 27) |
| Dedup | None (markdown is overwritable) | `FactId` content hash |
| Query | Lazy-load on reference (34B) | Tag → BM25 → graph-walk → RRF |
| Audit | Git history (the markdown files are tracked) | Append-only `revisions.jsonl` with `RevisionEntry` per mutation |
| Target user | Power user editing memory by hand | Agents writing memory at scale; compliance-oriented teams |

## 34A Design — Read `memory/MEMORY.md` on startup

At REPL entry and at single-shot CLI launch (`aichat "..."` and `aichat -r role`), aichat reads `$PROJECT_ROOT/memory/MEMORY.md` (if present) and injects its content as an additional system-prompt message, appended after the role's `prompt:` body. Topic files referenced from `MEMORY.md` are *not* eagerly loaded — that's 34B.

Mirrors Claude Code's behaviour: read the first ~200 lines of `MEMORY.md`, treat as context, do not write. Cap at 200 lines (or 8 KiB, whichever is smaller) to bound token cost; emit a one-line warning to stderr if truncation fires so the user knows they should split the file into topics.

**Files:** [`src/repl/mod.rs`](../../src/repl/mod.rs) (read-on-launch hook before the prompt loop), [`src/config/session.rs`](../../src/config/session.rs) (session-level `memory_preamble: Option<String>` cached so multi-turn conversations don't re-read), [`src/config/mod.rs`](../../src/config/mod.rs) (project-root probe; reuse the `knowledge/` discovery logic at `src/knowledge/cli.rs`).

### 34A — as shipped (2026-05-30)

The implementation diverged from the draft above in two ways, both for a
single, broader chokepoint:

- **Injection point is `Input::build_messages`** (`src/config/input.rs`), not
  `repl/mod.rs`. That one function is the universal path every completion flows
  through — single-shot CLI, the legacy REPL, **and** the server's role path
  (`chat_completions_via_role`). Injecting there (right after the existing
  `-o` output-format suffix injection) covers all of them with one call to the
  new `memory::inject_preamble`. The draft's per-surface hooks would have
  missed the server path.
- **The cache lives on `Config`** (`memory_preamble: Option<String>`), read
  once in `Config::init` via `memory::load_preamble`, rather than on
  `Session`. The preamble is process-wide standing context, not session state,
  and roles/prompts without a session still need it.
- **New module `src/memory/mod.rs`** owns discovery, the cap, and injection
  (with unit tests for the cap edge cases and the injector). Discovery uses
  `current_dir()/memory/` for project-local and `Config::config_dir()/memory/`
  for user-level, with an `AICHAT_MEMORY_DIR` override (`get_env_name`
  convention) — it does **not** reuse `kb_root()` because that is config-dir-
  rooted, whereas memory is project-root-first per tenet 5.
- **Pi's native agent turns** (which bypass aichat roles entirely) are covered
  by a parallel reader in the bundled pi extension
  (`pi-extensions/src/index.ts` → `assets/pi-extensions/aichat-bridge.js`),
  capped to the same budget. This replaces the buggy first-pass reader.

Observability: the preamble surfaces in `aichat --info -o json` under
`memory_preamble` and in text `--info` as a `memory_preamble  <N> chars` row.
Tests: `tests/integration/auto-memory.sh` (bats) + `src/memory/mod.rs` units.

## 34B Design — Topic-file lazy loading

When a turn references a relative markdown link found inside `MEMORY.md` (e.g. the user types "see my feedback on cite_sources" and the role's tool-search surfaces `feedback_cite_sources.md`), aichat lazy-loads that file via the existing `Input::from_files_with_spinner` plumbing in [`src/config/input.rs`](../../src/config/input.rs).

The reference can come from three sources, in precedence order:

1. The role's `prompt:` body contains a `@path` reference (depends on Theme 3 of the playbook — out of scope here; flagged as a future link).
2. The user explicitly types `.memory load <topic>` in the REPL (depends on Theme 10; out of scope).
3. The Reflector's accepted output (34C) references a topic file by name.

For 34B alone, only (3) is wired. (1) and (2) become available when their parent themes ship; this phase reserves the loader API so they plug in without rework.

**Files:** [`src/config/input.rs`](../../src/config/input.rs) (extend `Input::from_files_with_spinner` to accept a memory-root prefix), `src/repl/mod.rs` (lazy-load hook on cross-reference).

## 34C Design — Session-exit Reflector pass

At session exit (`session.exit()` in [`src/config/session.rs:641`](../../src/config/session.rs) or every N turns under a future `--memory-reflect-every N` flag), aichat invokes a Reflector role with the full conversation transcript and asks it to emit candidate `memory/<topic>.md` files.

The Reflector role is structurally identical to the existing `--knowledge-reflect` Reflector in [`src/knowledge/evolve.rs`](../../src/knowledge/evolve.rs), but with three differences in its system prompt:

1. **Output format is freeform markdown with YAML frontmatter**, not typed `EntityDescriptionPair` JSONL.
2. **The candidate-topic name is derived from the conversation's recurring noun phrase**, not from a `FactId` content hash.
3. **Secret-filter pass runs first**: any turn matching the redaction patterns in `src/config/mod.rs` is replaced with `[REDACTED]` before the Reflector sees it. This is a hard requirement — the Reflector writes to disk and any leak persists.

The Reflector emits a list of `(topic_name, frontmatter, body)` tuples; nothing is written to disk yet. 34D handles the gate.

**Files:** [`src/knowledge/evolve.rs`](../../src/knowledge/evolve.rs) (factor out the Reflector invocation primitive so memory + knowledge share it), `src/config/session.rs` (call site at exit), new `src/memory/reflect.rs` (the secret-filter pass + topic-name derivation).

## 34D Design — Curator gate

Each Reflector-emitted candidate is presented to the user as:

```
Memory candidate (1/3): "rust_async_preferences"
─────────────────────────────────────────────
---
created: 2026-05-25T12:04:00Z
session: session-2026-05-25-114417-vygh
turns_referenced: [3, 5, 11]
---
The user prefers `tokio::spawn` over `async_std::task::spawn` for new code
in this project. Cited rationale: "we standardize on tokio across the
codebase already."
─────────────────────────────────────────────
[a]ccept  [s]kip  [e]dit  [r]eject-all
>
```

`accept` writes the file to `memory/<topic>.md` atomically (tmp + rename, mirroring `knowledge/store.rs`) and appends a one-line entry to `memory/MEMORY.md`. `skip` discards just this candidate. `edit` opens `$EDITOR` on the candidate before re-presenting. `reject-all` aborts the curation pass entirely.

Under `--memory-auto-curate`, every candidate auto-accepts. The flag is hidden from `--help` by default and documented only in the dual-store user-facing doc — the prompt is the default for a reason.

**Files:** `src/memory/curate.rs` (interactive prompt + atomic write), `src/repl/mod.rs` (`--memory-auto-curate` CLI plumb), `src/cli.rs` (flag declaration).

## Open questions

### 1. Project-local vs user-level memory precedence

**Question:** When both `$PROJECT_ROOT/memory/` and `~/.config/aichat/memory/` exist, which wins?

**Recommendation: project-local wins; user-level is the fallback if project-local has no `MEMORY.md`.** Project-local matches the `knowledge/` precedent (each project has its own KB). User-level becomes the global preference layer — equivalent to `~/.claude/CLAUDE.md`. The two never merge; the precedence is binary. A future `--memory-stack` flag could opt into concatenation, but the default is single-source-of-truth.

### 2. Reflector trigger cadence

**Question:** Run the Reflector at every session exit, every N turns, or only on explicit `.memory reflect`?

**Recommendation: explicit-only by default; `--memory-reflect-on-exit` opt-in.** Same logic as `--memory-auto-curate`: silent token spend on every exit (the Reflector itself costs ~500-2000 tokens) violates cost-conscious-above-all. The explicit `.memory reflect` REPL command (gated on Theme 10 landing) is the default surface. Power users can flip the opt-in flag and accept the cost.

### 3. Token budget for the startup preamble

**Question:** How aggressive should the 34A cap be? 200 lines, 100, 50?

**Recommendation: 200 lines / 8 KiB / 2k tokens, whichever hits first.** Matches Claude Code's published cap. Below 200 lines, the markdown is usually one project's preferences — fits in a system prompt without crowding role instructions. Above 200, the user has probably accumulated multi-project sprawl in one file and should be nudged to split into topic files (the warning fires). The cap is a default, overridable via `memory.preamble_max_lines: N` in `~/.config/aichat/config.yaml` — out of scope here but flagged for the implementation.

### 4. Deferred — `dependencies.md` / `success-metrics.md` updates

This phase does **not** update [`docs/roadmap/dependencies.md`](dependencies.md) or [`docs/roadmap/success-metrics.md`](success-metrics.md). Both files need rows for Phase 34 (and 35, 36) once implementation begins. Tracked as a follow-up doc PR.

## Testing

Per project guideline ("*Always* add integration tests via bats in addition to unit tests"), the implementation PR(s) for this phase must add:

- **`tests/integration/auto-memory.sh`** — bats integration test covering:
  - 34A: `memory/MEMORY.md` content appears in `aichat --info -o json` system-prompt output when present, is absent when the file does not exist.
  - 34A: truncation warning fires when `MEMORY.md` exceeds 200 lines.
  - 34B: a Reflector-accepted candidate referencing a topic file triggers lazy load on the next turn (mocked via a stub-Reflector that emits a deterministic candidate).
  - 34C: secret-redaction pass replaces an `OPENAI_API_KEY=sk-...` line with `[REDACTED]` before the Reflector sees it (assert against an instrumented Reflector that echoes its input).
  - 34D: `accept` writes `memory/<topic>.md` and appends to `MEMORY.md`; `skip` does neither; `reject-all` aborts cleanly with exit code 0.
- **Rust unit tests** in `src/memory/reflect.rs` for the secret-filter pass (covers known patterns: `sk-`, `pk_`, `Bearer `, `xoxb-`, etc.) and in `src/memory/curate.rs` for the atomic-write path.

## Sequencing

- **34A and 34B should land together** (one PR). 34A without 34B is shippable but 34B without 34A has nothing to lazy-load against.
- **34C and 34D must land together** (one PR). 34C without 34D writes candidates to disk without a gate, violating the audit-by-default tenet.
- **The minimal half (34A+B) can ship before the maximal half (34C+D)** is designed. Recommended sequence: ship 34A+B first, gather user signal on whether the read-only surface is enough, then commit to 34C+D.

## Files (consolidated)

- [`src/repl/mod.rs`](../../src/repl/mod.rs) — startup hook for 34A; lazy-load hook for 34B; `--memory-auto-curate` plumb
- [`src/config/session.rs`](../../src/config/session.rs) — `memory_preamble` cache; Reflector call site at exit
- [`src/config/input.rs`](../../src/config/input.rs) — memory-root-aware file loader
- [`src/config/mod.rs`](../../src/config/mod.rs) — project-vs-user memory directory probe
- [`src/knowledge/evolve.rs`](../../src/knowledge/evolve.rs) — factor out shared Reflector primitive
- `src/memory/reflect.rs` (new) — secret-filter + topic-name derivation
- `src/memory/curate.rs` (new) — interactive curator + atomic write
- [`src/cli.rs`](../../src/cli.rs) — `--memory-auto-curate` flag
- See deep design notes: [`phase-34-auto-memory.md`](phase-34-auto-memory.md)

## References

- Theme 2 of the divergence playbook (analysis source for this phase) — see [`docs/analysis/open-memory/claude-code.md`](../analysis/open-memory/claude-code.md)
- [`memory/MEMORY.md`](../../memory/MEMORY.md) — the existing (unread) index file
- [Phase 27 knowledge evolution](phase-27-knowledge-evolution.md) — the existing typed Reflector surface this phase factors against
- [Phase 35 overview](phase-35-overview.md) — sibling phase under Epic 14 (Knowledge-MCP, the typed side of the dual-store)