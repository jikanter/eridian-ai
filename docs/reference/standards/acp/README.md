# Agent Client Protocol (ACP) — cached reference

> Cached per CLAUDE.md ("If asked to implement against a standard … download and
> cache that standard in docs/reference/standards"). This is a concise, working
> reference — not the full spec. Source of truth is upstream.
>
> - Spec home: <https://agentclientprotocol.com>
> - Overview: <https://agentclientprotocol.com/protocol/overview>
> - Fetched: 2026-07-03

## What it is

ACP is a JSON-RPC 2.0 protocol that lets a **Client** (a user-facing editor/IDE,
e.g. Zed) drive an **Agent** (an AI program that reads/writes code). It is
bidirectional: request/response methods plus one-way notifications.

## Transport

JSON-RPC 2.0 messages over **stdio**. The Agent is typically spawned as a
subprocess by the Client; the two speak framed JSON-RPC on the child's
stdin/stdout.

## Roles

- **Client** — editor/UI. Owns the environment, file access, permissions, and
  the human. Launches the Agent.
- **Agent** — the AI-driven program. Streams output, requests tool execution
  and permissions back through the channel.

## Lifecycle / core methods

1. `initialize` — negotiate protocol version + exchange capabilities;
   optionally `authenticate`.
2. `session/new` — create a session (or `session/load` to resume, when the
   agent advertises that capability).
3. `session/prompt` — Client sends user input for a turn. Agent streams
   progress via `session/update` notifications, and replies to `session/prompt`
   with a completion status when the turn ends. Client may interrupt with
   `session/cancel`.

`session/update` is a notification (no response) carrying the turn's streamed
content, including:

- `agent_message_chunk` — streamed assistant text.
- `tool_call` / `tool_call_update` — a tool the agent is running, with optional
  file **locations** so the Client can follow along (open the referenced file).
- plan updates, mode changes, and permission requests.

## How aichat relates to ACP

aichat does **not** speak ACP itself. `aichat --acp` is the entry point; the
chain under Zed is:

```
Zed (ACP Client)
  └─ spawns  aichat --acp
        ├─ brings up the aichat bridge (/v1/state/*) + stages aichat-bridge.js
        └─ spawns  pi-acp            (ACP adapter; JSON-RPC/stdio ⇄ Zed)
              └─ spawns  pi --mode rpc   (the coding agent)
                    └─ loads  aichat-bridge.js   (this repo's extension)
                          └─ HTTP ⇒ aichat bridge   (the /v1/state/* routes)
```

- **`pi-acp`** (npm, `pi-acp`) is the ACP↔pi adapter. It maps ACP methods to
  pi's `--mode rpc` and emits `agent_message_chunk` / `tool_call` updates. It is
  the actual ACP backend; aichat rides on top of pi.
- **`aichat-bridge.js`** (bundled from `pi-extensions/`) exposes aichat's
  slash-commands (`/role`, `/agent`, `/session`, `/rag`, `/macro`, …) inside a
  pi session by calling aichat's HTTP `/v1/state/*` routes.
- When the bundle detects it is running under an ACP host
  (`AICHAT_BRIDGE_SURFACE=acp`), it registers via `POST /v1/state/subprocess`,
  which returns the live aichat context so the host can surface it.

See [docs/features/zed.md](../../../features/zed.md) for the end-to-end wiring.

## Limitations of this integration

- aichat's ACP support is **indirect** — it depends entirely on `pi-acp`'s ACP
  conformance. aichat contributes the bridge extension + the
  `/v1/state/subprocess` registration endpoint, not an ACP server.
- `pi-acp` is MVP-stage and Zed-centric; other ACP clients may vary.
