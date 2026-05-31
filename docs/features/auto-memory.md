# Auto-Memory (`memory/MEMORY.md`)

**Status:** 34A–34D shipped — read-on-startup, lazy topic-load, and the
session-exit Reflector/Curator write loop. See
[`docs/roadmap/phase-34-overview.md`](../roadmap/phase-34-overview.md).

Auto-memory is aichat's freeform, read-on-startup notes layer. It is the
session-level counterpart to the typed [`knowledge/`](knowledge.md) store:
where `knowledge/` holds citable, deduplicated facts compiled from source
files, `memory/` holds incidental learnings a human jots down by hand —
preferences expressed mid-conversation, project conventions, reminders. The
two stores never merge and never silently promote into one another.

## What 34A does

At startup aichat reads `memory/MEMORY.md` (if present), caps it, and injects
the capped content into the **system prompt**, appended after the active
role's prompt body. It is **read-only**: aichat never writes to `memory/` in
this phase.

This mirrors Claude Code's first-~200-lines auto-load discipline: a small
standing-context file the agent always sees, kept short enough not to crowd
the role's own instructions.

## Discovery precedence

The first match wins; the stores never merge (phase-34 open question 1):

1. **`AICHAT_MEMORY_DIR`** — explicit override. If set, only this directory is
   consulted; no fallback. Used by tests and by power users who keep memory
   outside the default chain.
2. **Project-local** — `./memory/MEMORY.md` relative to the working directory.
   Each project carries its own memory, matching the `knowledge/` precedent.
3. **User-level** — `<config_dir>/memory/MEMORY.md` (e.g.
   `~/.config/aichat/memory/MEMORY.md`). The global preference layer,
   equivalent to `~/.claude/CLAUDE.md`.

An absent or empty `MEMORY.md` is a clean no-op — no system-prompt change and
zero added tokens.

## The cap

The preamble is capped at **200 lines or 8 KiB, whichever hits first** (Claude
Code parity; phase-34 open question 3). When the cap drops content, aichat
emits a one-line warning to stderr:

```
warning: <path>/MEMORY.md exceeds the 200-line / 8-KiB memory preamble cap;
         split it into topic files so context is not dropped
```

The cap never splits a UTF-8 character: it drops whole trailing lines first,
and hard-truncates a lone over-budget line on a char boundary.

## Inspecting the loaded preamble

The preamble is observable without a model call via `--info`:

```bash
aichat --info -o json        # -> { ..., "memory_preamble": "# Project memory\n- ..." }
aichat --info                # -> a `memory_preamble  <N> chars` row
```

The injected block is framed with a `# Project memory` header so the model
reads it as standing context rather than task instructions. `--info` shows the
raw memory; the `--dry-run` role preview does **not** include it (dry-run
previews the role file, not the assembled messages).

## Which surfaces inject memory

| Surface | Injection point |
|---|---|
| `aichat "..."`, `aichat -r <role>`, `aichat -a <agent>` | Rust `Input::build_messages` (`src/config/input.rs`) |
| Legacy built-in REPL | same Rust path |
| HTTP server, role path (`/v1/chat/completions` with a role) | same Rust path |
| **pi REPL — native agent turns** | `before_agent_start` hook in the bundled pi extension (`assets/pi-extensions/aichat-bridge.js`) |

Pi's native turns build their own system prompt independent of any aichat
role, so the pi extension carries a matching reader capped to the same
200-line / 8-KiB budget. The OpenAI-compatible passthrough path (raw
`messages`, no role) is intentionally **not** injected — those requests carry
their own system prompt and may originate from external clients.

## Relationship to `knowledge/`

| | `memory/` (this feature) | `knowledge/` (Phases 25–27) |
|---|---|---|
| Storage | Markdown + YAML frontmatter, freeform | Typed JSONL + `manifest.yaml` |
| Writer | Human (by hand); session-exit Reflector (34C) gated by Curator (34D) | `--knowledge-reflect` / `--knowledge-curate` |
| Query | Read on startup; lazy topic-load by reference (34B) | Tag → BM25 → graph-walk → RRF |
| Audit | Git history | Append-only `revisions.jsonl` |

## Lazy topic-loading (34B)

`MEMORY.md` is the always-loaded index; the topic files it links to are loaded
only on demand. A `memory:<reference>` path resolves against the index links
(`[label](topic.md)`) and topic filenames, then expands to the topic file at
`Input::from_files`:

```bash
aichat -f memory:cite_sources "draft the intro"   # loads memory/feedback_cite_sources.md
aichat --memory-load cite_sources                 # prints the resolved topic (capped)
```

A reference matches by filename stem or by substring of an index link target;
`MEMORY.md` itself is never resolvable as a topic. An unresolvable reference
errors (`--memory-load`) or passes through unchanged (`-f`).

## The write loop (34C/34D)

At session exit — opt-in via `--memory-reflect-on-exit` (or
`AICHAT_MEMORY_REFLECT_ON_EXIT=1`) — aichat reflects over the conversation and
gates any candidate notes through the Curator. The same loop is reachable as a
one-shot from a transcript:

```bash
aichat --memory-reflect --memory-transcript chat.txt    # emit candidate set as JSON
aichat --memory-curate  --memory-transcript chat.txt    # reflect, then gate interactively
aichat --memory-curate  --memory-candidates cands.json  # gate a pre-built candidate set
```

**Secret redaction is mandatory and runs first.** Before the transcript ever
reaches the Reflector, `redact_secrets` rewrites recognized credentials
(`api_key`/`secret`/`password`/`token` assignments, `Bearer <tok>`, and
`sk-ant-`/`sk-`/`xoxb-`/`ghp_` prefixes) to `[REDACTED:<class>]`. There is no
flag to disable it — it is the only defense against persisting a leaked key.

The Reflector reuses the `knowledge/evolve.rs` pattern: a role whose name ends
`-memory-reflector` emits a structured candidate set (`topic`, `body`,
`turns_referenced`). Each candidate's topic slug is sanitized so it can never
inject a path separator.

**The Curator gate** prompts per candidate:

```
[a]ccept   write memory/<topic>.md + append an index line to MEMORY.md
[s]kip     drop this candidate, continue
[e]dit     open $EDITOR on the body, then re-prompt
[r]eject-all   abort the rest of the set, exit 0, write nothing
```

`accept` writes atomically (temp file + rename) and stamps frontmatter
(`created`, `session`, `reflector_model`, `curator`). `--memory-auto-curate`
(hidden, opt-in) accepts every candidate without prompting — for non-interactive
runs. A closed stdin (EOF) is treated as `reject-all`, so a broken pipe never
silently writes. An accepted candidate is immediately lazy-loadable by its
topic reference (34B).

See the [demo](../demos/phase-34-auto-memory.md) for runnable examples.