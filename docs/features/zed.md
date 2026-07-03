# Zed (ACP backend)

Run aichat's roles, agents, sessions, and macros from inside [Zed](https://zed.dev)'s
AI panel. Zed speaks the [Agent Client Protocol](../reference/standards/acp/README.md)
(ACP); aichat rides on top of `pi` through the [`pi-acp`](https://www.npmjs.com/package/pi-acp)
adapter and a bundled bridge extension.

## How it fits together

aichat does **not** speak ACP directly. The launch chain is:

```
Zed  (ACP client)
  └─ spawns  pi-acp            ACP JSON-RPC/stdio  ⇄  Zed
        └─ spawns  pi --mode rpc
              └─ loads  aichat-bridge.js        (bundled extension)
                    └─ HTTP ⇒ aichat --serve    (the /v1/state/* bridge)
```

- **`pi-acp`** is the ACP backend proper — it translates ACP methods to pi and
  emits `agent_message_chunk` / `tool_call` updates back to Zed.
- **`aichat-bridge.js`** exposes aichat's slash-commands (`/role`, `/agent`,
  `/session`, `/rag`, `/macro`, `/info`, `/aichat-edit`, …) inside the pi
  session by calling aichat's HTTP `/v1/state/*` routes.
- Because pi-acp — not aichat — launches pi under Zed, you must hand the bridge
  its wiring explicitly: a running `aichat --serve`, the bridge env vars, and
  the extension installed where pi will find it.

## Prerequisites

```bash
# pi + the ACP adapter
npm install -g @earendil-works/pi-coding-agent pi-acp

# aichat's companion tools (installs pi too, if missing)
aichat --install-deps
```

## Wiring

### 1. Run the aichat bridge server

The bridge routes require a bearer token. Export it, pick an address, and serve:

```bash
export AICHAT_BRIDGE_TOKEN="$(uuidgen)"   # any secret string
aichat --serve 127.0.0.1:8000
```

`aichat --serve` reads `AICHAT_BRIDGE_TOKEN` once at startup; the same value
must be handed to pi-acp below. Keep this process running.

### 2. Install the bridge extension where pi-acp's pi will load it

pi auto-discovers extensions from its agent dir, `~/.pi/agent/extensions/`.
Because pi-acp spawns a plain `pi` (not an aichat-managed one), install the
bundle there once:

```bash
aichat --install-pi-extension
# → Installed aichat pi bridge extension: ~/.pi/agent/extensions/aichat-bridge.js
```

Pass a directory to override the default (e.g. a project-local `.pi/agent`).
Re-run after an aichat upgrade to refresh the bundle — the install overwrites.

### 3. Point Zed's agent server at pi-acp with the bridge env

In Zed `settings.json`, register pi-acp as a custom ACP agent and pass the
three bridge env vars. `AICHAT_BRIDGE_SURFACE=acp` is the signal that tells the
extension it is running under an ACP host (so it registers itself and surfaces
aichat's live context in Zed's session startup block):

```json
{
  "agent_servers": {
    "aichat (pi-acp)": {
      "command": "pi-acp",
      "args": [],
      "env": {
        "AICHAT_BRIDGE_URL": "http://127.0.0.1:8000",
        "AICHAT_BRIDGE_TOKEN": "<same token you exported in step 1>",
        "AICHAT_BRIDGE_SURFACE": "acp"
      }
    }
  }
}
```

Open Zed's agent panel, pick **aichat (pi-acp)**, and start a thread. The
aichat slash-commands are available, and switching a role/agent/session there
mutates the same live aichat context the `--serve` process holds.

## How the surface signal works

The bundled extension self-detects its host:

| `AICHAT_BRIDGE_SURFACE` | Set by | Behavior |
|---|---|---|
| `acp` | Zed agent-server env (step 3) | Registers via `POST /v1/state/subprocess`, surfaces context |
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

With the bridge up and the env exported, the same routes Zed drives are
curl-able:

```bash
curl -sS -X POST \
  -H "Authorization: Bearer $AICHAT_BRIDGE_TOKEN" \
  -H 'Content-Type: application/json' --data '{"surface":"acp"}' \
  http://127.0.0.1:8000/v1/state/subprocess | jq
```

## Troubleshooting

- **Slash-commands missing in Zed** — the extension isn't installed where
  pi-acp's pi looks. Re-run `aichat --install-pi-extension` and restart the Zed
  thread. Confirm `~/.pi/agent/extensions/aichat-bridge.js` exists.
- **401 from the bridge** — `AICHAT_BRIDGE_TOKEN` in Zed's env doesn't match the
  one exported to `aichat --serve`. They must be identical.
- **No context in the startup block** — `AICHAT_BRIDGE_SURFACE` isn't `acp` in
  Zed's agent-server env, so registration was skipped.
- **Connection refused** — `AICHAT_BRIDGE_URL` doesn't match the address
  `aichat --serve` is listening on.

## See also

- [server.md](./server.md) — the `aichat --serve` HTTP surface.
- [repl-pi.md](./repl-pi.md) — the terminal (`--pi-repl`) surface of the same bridge.
- [ACP standard (cached)](../reference/standards/acp/README.md).
