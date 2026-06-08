# Discovery: find flags and docs from the REPL

aichat's discovery surface lets you answer "what can this thing do?" without
leaving the [pi REPL](repl-pi.md) or memorizing `--help`. It has two halves —
**flags** (the live CLI surface) and **docs** (the bundled feature
documentation) — exposed both as pi slash commands and as read-only HTTP
routes on the bridge server.

## In the pi REPL

The bridge registers two slash commands alongside `/role`, `/agent`, etc.

```
/aichat-flags              # every aichat CLI flag
/aichat-flags role         # only flags whose name or help mentions "role"
/aichat-docs               # list the bundled feature docs
/aichat-docs server        # print the server.md doc inline
```

`/aichat-flags <query>` filters case-insensitively across each flag's long
name, short name, and help text, so `/aichat-flags session` surfaces every
session-related switch in one shot. `/aichat-docs` with no argument lists the
doc slugs and titles; pass a slug to read the whole doc in the pi pane.

Both are pure reads — they never change the active role, session, RAG, or
agent.

## HTTP routes

The same data is available on the bridge server (the in-process server
`aichat` starts for the REPL, or a long-lived `aichat --serve` with
`AICHAT_BRIDGE_TOKEN` exported). Like the `/v1/state/*` routes, the discovery
routes are gated by the bridge bearer token and return `404` when no token is
configured — see [repl-pi.md](repl-pi.md#bridge-security) for the token model.

| Route | Returns |
|---|---|
| `GET /v1/discovery/flags` | `{ "flags": [...], "count": N }` |
| `GET /v1/discovery/flags?q=<query>` | the same, filtered to matches |
| `GET /v1/discovery/docs` | `{ "docs": [{name, file, title}], "count": N }` |
| `GET /v1/discovery/docs?name=<slug>` | `{ "name", "content" }` (404 if unknown) |

A flag entry is `{ long, short, help, takes_value }`. `takes_value`
distinguishes a value flag (`--model X`) from a bare switch (`--list-roles`).

```bash
curl -s -H "Authorization: Bearer $AICHAT_BRIDGE_TOKEN" \
  "http://127.0.0.1:8000/v1/discovery/flags?q=rag" | jq '.flags[].long'
```

## Why it's always accurate

The flag list is introspected from aichat's live Clap command tree at request
time, so it can never drift from `src/cli.rs` — there is no second catalog to
maintain. The feature docs are embedded into the binary at build time from
`docs/features/*.md`, so `/aichat-docs` works for an installed aichat with no
source checkout on disk.
