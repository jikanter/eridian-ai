# Zed (ACP backend)

Run aichat's roles, agents, sessions, and macros from inside [Zed](https://zed.dev)'s
AI panel. Zed speaks the [Agent Client Protocol](../reference/standards/acp/README.md)
(ACP); `aichat --acp` exposes aichat's pi surface to it.

## How it fits together

aichat does **not** speak ACP directly. `aichat --acp` runs aichat *as* an ACP
agent over stdio: it brings up the aichat bridge, pins pi to aichat's models,
stages the bridge extension into the agent dir pi reads, then delegates ACP
protocol translation to the [`pi-acp`](https://www.npmjs.com/package/pi-acp)
adapter.

```
Zed  (ACP client)
  └─ spawns  aichat --acp
        ├─ brings up the aichat bridge (/v1/state/*) + stages aichat-bridge.js
        └─ spawns  pi-acp             ACP JSON-RPC/stdio ⇄ Zed
              └─ spawns  pi --mode rpc
                    └─ loads  aichat-bridge.js   → HTTP ⇒ aichat bridge
```

- **`aichat --acp`** is the single entry point — no separate `aichat --serve`,
  no manual extension install, no bridge env to hand-wire. It stages everything
  per launch and tears it down on exit.
- **`pi-acp`** is the ACP backend proper: it maps ACP methods to pi and emits
  `agent_message_chunk` / `tool_call` updates back to Zed.
- **`aichat-bridge.js`** exposes aichat's slash-commands (`/role`, `/agent`,
  `/session`, `/rag`, `/macro`, `/info`, `/aichat-edit`, …) inside the pi
  session via aichat's `/v1/state/*` routes. On load under ACP it registers via
  `POST /v1/state/subprocess` so Zed can surface aichat's live context.

## Prerequisites

```bash
# aichat's companion tools (installs pi, showboat, uv if missing)
aichat --install-deps

# the ACP adapter (not yet covered by --install-deps)
npm install -g pi-acp
```

`aichat --acp` fails fast with an install hint if `pi` or `pi-acp` is missing.
Point aichat at a non-default adapter with `AICHAT_ACP_COMMAND`
(e.g. `AICHAT_ACP_COMMAND='npx -y pi-acp'`).

## Wiring Zed

Register aichat as a custom ACP agent server in Zed `settings.json`:

```json
{
  "agent_servers": {
    "aichat": {
      "command": "aichat",
      "args": ["--acp"]
    }
  }
}
```

Open Zed's agent panel, pick **aichat**, start a thread. aichat's slash-commands
are available, and switching a role/agent/session there mutates the live aichat
context the `--acp` process holds.

## The surface signal

The bundled extension self-detects its host via `AICHAT_BRIDGE_SURFACE`, which
the launcher sets on the child:

| Value | Set by | Behavior |
|-------|--------|----------|
| `acp` | `aichat --acp` (on the pi-acp adapter) | Registers via `POST /v1/state/subprocess`, surfaces context |
| `repl` | `aichat --pi-repl` (aichat's own terminal REPL) | Skips registration — aichat already owns the context |
| unset | manual/other pi launch | Skips registration |

`POST /v1/state/subprocess` returns the live entity context:

```json
{
  "ok": true,
  "kind": "subprocess",
  "surface": "acp",
  "context": { "role": "coder", "agent": null, "session": "demo", "rag": null }
}
```

If registration fails (bridge down, wrong token) the extension logs the error
to stderr — captured in Zed's agent-server logs — rather than failing silently.

## Verifying

Drive the same route Zed's session uses against a running bridge (e.g. an
`aichat --serve` you started with `AICHAT_BRIDGE_TOKEN` exported):

```bash
curl -sS -X POST \
  -H "Authorization: Bearer $AICHAT_BRIDGE_TOKEN" \
  -H 'Content-Type: application/json' --data '{"surface":"acp"}' \
  http://127.0.0.1:8000/v1/state/subprocess | jq
```

## Troubleshooting

- **`pi-acp` not found** — `npm install -g pi-acp`, or set `AICHAT_ACP_COMMAND`.
- **`pi` not found** — `aichat --install-deps` (or `npm install -g @earendil-works/pi-coding-agent`).
- **Slash-commands missing in Zed** — the extension didn't stage. Re-run with
  `AICHAT_KEEP_PI_STAGE=1` and check the staged agent dir's `extensions/`.
- **No context in the startup block** — registration was skipped; confirm the
  child saw `AICHAT_BRIDGE_SURFACE=acp` (it is set automatically by `--acp`).

## Test Log
- Tested 2026-07-04 by jordan on macosx

## See also

- [server.md](./server.md) — the `aichat --serve` HTTP surface.
- [repl-pi.md](./repl-pi.md) — the terminal (`--pi-repl`) surface of the same bridge.
- [ACP standard (cached)](../reference/standards/acp/README.md).
