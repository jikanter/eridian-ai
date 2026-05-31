# The aichat HTTP server

`aichat --serve` starts an OpenAI-compatible HTTP server. The same server
also powers the [pi REPL](repl-pi.md) bridge: when you launch the REPL,
aichat needs a server for `pi` to talk to, and it will either **reuse a
server you already have running** or **start a private one** for the
session.

This document covers the server itself and, in detail, the
`AICHAT_BRIDGE_TOKEN` mechanism that lets a REPL launch safely reuse an
already-running server.

## Starting a server

```bash
aichat --serve            # binds the configured serve_addr (default 127.0.0.1:8000)
aichat --serve 8080       # bare integer → 127.0.0.1:8080
aichat --serve 0.0.0.0    # bare IP → 0.0.0.0:8000
aichat --serve 0.0.0.0:9000
```

The server exposes the OpenAI-compatible surface (`/v1/chat/completions`,
`/v1/models`, `/v1/embeddings`, `/v1/rerank`), aichat-native routes
(`/v1/roles`, `/v1/prompts`, `/v1/rags`, `/v1/pipelines/run`, `/v1/batch`),
and the browser [playground](playground.md)/arena. See
[`src/serve.rs`](../../src/serve.rs).

## Production hardening

By default the server is localhost-only and unauthenticated — the right
choice for a developer machine. A few opt-in config keys make it safe to put
behind a reverse proxy or expose to a Docker bridge network. None of them
change the default behavior when unset.

### CORS (`serve_cors_origins`, `serve_cors_allow_all`)

Localhost origins are always allowed (the bundled playground/arena are
same-origin). To let another origin — e.g. an OpenWebUI dev server or a
container on `host.docker.internal` — call the API from a browser, list it:

```yaml
serve_cors_origins:
  - http://localhost:3000
  - http://host.docker.internal:3000
```

On a fully trusted network you can echo *any* origin instead:

```yaml
serve_cors_allow_all: true   # reflects the request Origin on every response
```

Requests from an origin that is neither localhost nor listed receive no
`Access-Control-Allow-Origin` header, so the browser blocks the cross-origin
read — exactly as before this knob existed.

### Bearer-token auth (`serve_api_key`)

```yaml
serve_api_key: sk-my-secret-key
```

When set, every request must carry `Authorization: Bearer sk-my-secret-key`
or it gets **401**. Two routes are deliberately exempt:

- `OPTIONS` preflight (it can't carry credentials), and
- `GET /health` (orchestration probes must work without a key).

The `/v1/state/*` bridge keeps its own `AICHAT_BRIDGE_TOKEN` gate and is not
affected by `serve_api_key`. This is a single static token, not a user
system — for multi-user auth, front the server with nginx or a gateway.

### Health probe (`GET /health`)

```bash
curl http://127.0.0.1:8000/health
# {"status":"ok","models":42,"roles":15}
```

Unauthenticated and dependency-free — suitable for Docker `HEALTHCHECK`,
Kubernetes liveness/readiness, or a systemd watchdog. `models` counts the
provider models in `/v1/models` (the `role:*` virtual models are excluded);
`roles` counts the roles currently served.

### Hot reload (`POST /v1/reload`)

```bash
curl -X POST http://127.0.0.1:8000/v1/reload
# {"roles":15,"models":42}
```

Re-reads role, prompt, and RAG files from disk so role-authoring edits take
effect without a restart. Provider `clients:` changes in `config.yaml` still
require a restart (the model wiring is fixed at boot). Subject to
`serve_api_key` when configured.

### Streaming usage (`stream_options.include_usage`)

`/v1/chat/completions` follows OpenAI's convention: pass
`"stream_options": {"include_usage": true}` and the stream ends with a
usage-only chunk (`choices: []`) carrying token counts plus aichat's
`cost_usd`, right before `data: [DONE]`:

```json
data: {"choices":[],"usage":{"prompt_tokens":892,"completion_tokens":341,"total_tokens":1233,"cost_usd":0.012}}
```

aichat asks the upstream provider for the same usage block, so accuracy
depends on the provider reporting it during streaming; when it doesn't, the
token counts fall back to `0`. Without `include_usage`, the stream is byte-for-byte
what it was before (no extra chunk). The non-streaming response and the
`/v1/roles/{name}/invoke` endpoints already report cost via the
`X-AIChat-Cost-USD` header (Phase 16H).

## The bridge surface (`/v1/state/*`)

On top of the public API the server has a **bridge**: a small set of
state-mutating routes — `/v1/state/info`, `/v1/state/role`,
`/v1/state/session`, `/v1/state/rag`, `/v1/state/agent`,
`/v1/state/macro`, `/v1/state/exit-context` — that change the live
`Config` of the running server. The pi REPL's slash commands (`/role`,
`/agent`, `/macro`, `/rag`, `/info`, …) are thin wrappers over these
routes.

The bridge is **gated by a bearer token**, never exposed unauthenticated:

| Server state | `/v1/state/*` behavior |
|---|---|
| Started with **no** `AICHAT_BRIDGE_TOKEN` (plain `--serve`) | Routes return **404** — the surface is invisible. |
| Started **with** `AICHAT_BRIDGE_TOKEN`, request token wrong/absent | **401 Unauthorized**. |
| Started **with** `AICHAT_BRIDGE_TOKEN`, request token matches | **200** — the state change applies. |

Token comparison is constant-time. A plain `aichat --serve` user who never
sets the variable can never reach the bridge — exactly the intent.

## `AICHAT_BRIDGE_TOKEN` — the token export mechanism

`AICHAT_BRIDGE_TOKEN` is the single shared secret that ties a bridge
client to a bridge server. It is read from the **process environment** at
server start (in `Server::new`) and at REPL launch.

### Default: a fresh per-launch token

When you run the REPL the ordinary way (`aichat`, `aichat --pi-repl`) and
no server is reused, aichat:

1. mints a fresh 32-hex-char token (`uuid::Uuid::simple()`, ~122 bits),
2. sets it in its own environment **before** starting the in-process
   server, so the server picks it up,
3. passes the same value to the spawned `pi` via `AICHAT_BRIDGE_TOKEN`.

You never see or manage this token; it lives and dies with the launch.

### Exporting your own token (to share one server)

If you want **one long-lived server** that multiple REPL sessions — or
other bridge clients — share, export the token yourself so every party
agrees on it:

```bash
# Pick any non-empty secret. Generate one if you like:
export AICHAT_BRIDGE_TOKEN=$(uuidgen | tr -d - | tr 'A-Z' 'a-z')

# Terminal 1 — the shared server. It reads the token from the environment.
aichat --serve 8000

# Terminal 2 — a REPL that discovers and reuses that server.
# Same AICHAT_BRIDGE_TOKEN exported here → the probe authenticates and reuses.
aichat
```

Because both processes inherit the same `AICHAT_BRIDGE_TOKEN`, the REPL's
probe can authenticate against the server's bridge, and every slash
command works against that shared server's live state.

If you export the variable for the server but **not** for the REPL (or
export a different value), the REPL will not reuse that server — it will
fall back to a private in-process one. This is deliberate: aichat only
reuses a server it can actually drive.

## Server discovery and reuse

When the pi REPL launches, before starting its own server it **probes
`127.0.0.1`, ports 8000–9000**, for a server it can reuse. The probe is
strict — it does an authenticated `GET /v1/state/info`:

- A non-aichat server (or aichat with no bridge token) answers **404** → skipped.
- An aichat bridge with a **different** token answers **401** → skipped.
- An aichat bridge that **accepts the exported `AICHAT_BRIDGE_TOKEN`**
  answers **200** → **reused**. The lowest matching port wins.

So reuse happens only when (a) a server is listening in that range and
(b) `AICHAT_BRIDGE_TOKEN` is exported and matches that server's token.
Probes run concurrently; a closed range resolves in well under the 300 ms
per-port timeout.

When a server is reused, aichat does **not** start an in-process server,
and it leaves the reused server running when the REPL exits. When no
server is reused, aichat starts a private in-process server on an
ephemeral port and shuts it down on exit, as before.

### Opting out

```bash
AICHAT_NO_SERVER_PROBE=1 aichat      # never probe; always start a private server
```

Use this if port 8000–9000 holds an unrelated service you don't want
touched even by a probe, or to force an isolated server per launch.

## Environment variables

| Variable | Read by | Effect |
|---|---|---|
| `AICHAT_BRIDGE_TOKEN` | server start; REPL launch | Shared bridge secret. Unset → fresh per-launch token, no reuse. Exported → enables a shared, reusable server. |
| `AICHAT_NO_SERVER_PROBE` | REPL launch | When set, skip the 8000–9000 probe; always start a private in-process server. |
| `AICHAT_KEEP_PI_STAGE` | REPL exit | When set, keep the staged `aichat-bridge.js` instead of cleaning it up. |
| `AICHAT_SERVE_API_KEY` | server start | Overrides `serve_api_key:`. Sets the public bearer token (Phase 16B). |
| `AICHAT_SERVE_CORS_ALLOW_ALL` | server start | Overrides `serve_cors_allow_all:` (`true`/`false`). |
| `AICHAT_SERVE_CORS_ORIGINS` | server start | Overrides `serve_cors_origins:`. Comma-separated list of allowed origins. |

## Troubleshooting

**REPL slash commands return 401 against a shared server** — the REPL and
the server have different `AICHAT_BRIDGE_TOKEN` values. Export the *same*
non-empty value in both shells before starting either process.

**REPL won't reuse my running server** — check that the server is on
`127.0.0.1` in the 8000–9000 range, that it was started with
`AICHAT_BRIDGE_TOKEN` set, and that the REPL shell exports the identical
value. A server started with a plain `aichat --serve` (no token) is
intentionally never reused, because its bridge would 404 every slash
command. `AICHAT_NO_SERVER_PROBE` also disables reuse — make sure it isn't
set.

**Probe seems slow** — it shouldn't be; localhost ports refuse instantly.
A slow probe means something in 8000–9000 accepts connections but stalls.
Set `AICHAT_NO_SERVER_PROBE=1` to bypass it.