# Cross-surface command map

aichat is reachable through three command surfaces with three different
syntaxes. This is by design, not an inconsistency:

- **Batch CLI** (`aichat --flag …`) — the authoritative surface. aichat owns all
  batch interfaces; everything else is a convenience over it.
- **Legacy REPL** (`.command`) — the built-in Reedline REPL, a frozen fallback.
  Forced with `--legacy-repl` or `AICHAT_REPL=legacy`.
- **pi** (`/command`) — the default interactive surface. [pi](https://github.com/earendil-works/pi)
  owns the TUI; aichat owns inference and state, surfaced through the
  `aichat-bridge` extension. See [`repl-pi.md`](repl-pi.md).

**Ownership rule:** pi owns *interactive* sessions; aichat owns *batch*. The
legacy REPL exists only as a fallback and for testing. When the same operation
exists on more than one surface, the batch CLI form is canonical.

## Equivalent operations

| Operation | Batch CLI | Legacy REPL | pi |
|---|---|---|---|
| Select model | `-m, --model NAME` | `.model NAME` | `/model` *(pi-native)* |
| Use a role | `-r, --role NAME` | `.role NAME` | `/role` |
| Start an agent | `-a, --agent NAME` | `.agent NAME` | `/agent` |
| Run a macro | `--macro NAME` | `.macro NAME` | `/macro` |
| Use a RAG | `--rag NAME` | `.rag NAME` | `/rag` |
| Start/join a session | `-s, --session [NAME]` | `.session [NAME]` | `/aichat-session` |
| System / entity info | `--info` | `.info [role\|session\|rag\|agent]` | `/info` |
| Include files / URLs | `-f, --file PATH` | `.file PATH…` | pi-native context |
| Read a config value | `--config-get KEY` | `.info` (table) | `/info` |
| Config file path | `--config-path` | `.edit config` (opens it) | — |
| Edit role/session/config | edit the file directly | `.edit role\|session\|config` | `/aichat-edit` |
| Rebuild a RAG | `--rebuild-rag` | `.rebuild rag` | — |
| Leave a role/session context | (per-invocation) | `.exit role\|session` | `/exit-context` |
| Copy last reply | — | `.copy` | `/copy` *(pi-native)* |
| Continue / regenerate | — | `.continue` / `.regenerate` | pi-native |
| Delete an entity | — *(planned: Phase 54F)* | `.delete TYPE NAME` | — |
| Discover flags / docs | `--help`, `--man` | `.help` | `/aichat-flags`, `/aichat-docs` |
| Exit | (process exits) | `.exit` | `/quit` *(pi-native)* |

A `—` means the operation has no first-class form on that surface. *pi-native*
marks commands pi provides itself (not bridged from aichat); the bridge surfaces
the aichat-owned ones (`/role`, `/agent`, `/macro`, `/rag`, `/aichat-session`,
`/info`, `/aichat-edit`, `/aichat-flags`, `/aichat-docs`, `/exit-context`).

## Why three syntaxes

The CLI uses GNU-style `--flags` for Unix composition and scripting. The legacy
REPL uses `.dot` commands (Reedline). pi uses `/slash` commands (its TUI
convention). aichat does not try to unify them — instead the bridge maps pi's
slash commands onto the same aichat state the CLI mutates, so all three observe
one runtime. Cross-reference: [`repl-pi.md`](repl-pi.md),
[`pi-repl-migration.md`](pi-repl-migration.md), [`discovery.md`](discovery.md).
