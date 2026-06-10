# REPL via pi

aichat's interactive REPL is provided by [`pi`](https://github.com/earendil-works/pi),
the open-source coding-agent harness from earendil-works. When you run
`aichat` with no input, aichat starts its OpenAI-compatible HTTP server
on an ephemeral localhost port, stages a shipped pi extension into
`<cwd>/.pi/extensions/`, and hands the terminal over to `pi`. Pi owns the
TUI; aichat owns the inference, roles, agents, RAG, MCP pool, and macros.

> Built-in Reedline REPL: the legacy surface remains available behind
> `--legacy-repl` for long-term side-by-side comparison. There is no
> scheduled removal date.

## Install

```bash
# Either path works.
curl -fsSL https://pi.dev/install.sh | sh
npm install -g @earendil-works/pi-coding-agent
```

Confirm:

```bash
pi --version   # 0.74.0 or newer
```

## Launch

```bash
aichat                  # default: pi if installed, else legacy with a note
aichat --pi-repl        # strict: error if pi missing
aichat --legacy-repl    # force the built-in REPL
AICHAT_REPL=legacy aichat   # same as --legacy-repl, via env
AICHAT_REPL=pi aichat       # same as --pi-repl, via env
```

On launch you should see pi's editor with the bundled `aichat-bridge`
extension loaded. The bridge is the bidirectional connection back to
aichat — every slash command below routes through it.

### Behavior when pi isn't installed

Bare `aichat` does a soft fallback: it warns once and routes to the
built-in Reedline REPL.

```
aichat: `pi` not on PATH; using the built-in REPL. Install pi at
https://pi.dev for the new REPL surface, or pass --legacy-repl to
silence this message. `--pi-repl` requires pi and will error if missing.
```

Pass `--legacy-repl` (or set `AICHAT_REPL=legacy`) to opt into the
built-in REPL silently. Pass `--pi-repl` (or set `AICHAT_REPL=pi`) to
require pi — aichat will exit with an actionable install hint if it's
missing.

## Slash-command mapping

The bridge re-exposes aichat's interactive surface as pi slash commands.
Pi's own commands (`/model`, `/new`, `/fork`, `/clone`, `/compact`, `/copy`,
`/quit`, etc.) continue to work alongside them.

| Aichat legacy `.dot` | Pi slash command | Notes |
|---|---|---|
| `.role <name>` | `/role <name>` | Switches active role on aichat's `Config`. |
| `.role <name> <text>` | `/role <name>` then send `<text>` | One-shot role + prompt is a two-step in pi. |
| `.prompt <text>` | `/role <name>` (temp `%%`) then send | Or just send the prompt — pi has its own send mechanic. |
| `.session [name]` | `/aichat-session [name]` | Without a name, opens a temp session. Namespaced because pi reserves `/session`. |
| `.empty session` | not yet bridged | Use pi's `/new` to drop the conversation. |
| `.compress session` | not yet bridged | Use pi's `/compact` instead. |
| `.agent <name> [session]` | `/agent <name> [session]` | Binds an aichat agent for subsequent turns. |
| `.starter <id>` | not yet bridged | |
| `.rag [name]` | `/rag [name]` | Without a name, opens a temp RAG. |
| `.rebuild rag` | not yet bridged | |
| `.sources rag` / `.sources knowledge` | not yet bridged | |
| `.macro <name> [text]` | `/macro <name> [text]` | Runs the macro headlessly; result is shown in pi. |
| `.model <name>` | pi-native `/model` (Ctrl+P) | Pi's model picker lists **only aichat's models** — the launcher pins pi to them (see [Model pinning](#model-pinning)). Selecting one routes the turn through aichat. |
| `.continue` / `.regenerate` | pi-native `/fork` | Pi's session-tree navigation subsumes both. |
| `.copy` | pi-native `/copy` | |
| `.edit config` / `.edit role` / `.edit rag-docs` / `.edit agent-config` | `/aichat-edit <target>` | Opens the file in pi's native editor, then persists + reloads. See [`/aichat-edit`](#aichat-edit). |
| `.edit session` | pi-native (`/session`) | Pi owns the session format, so sessions are edited through pi's own surface, not the bridge. |
| `.save role` / `.save session` | not yet bridged | |
| `.set <k> <v>` | not yet bridged | |
| `.delete <kind>` | not yet bridged | |
| `.file <paths> -- <text>` | not yet bridged | Use pi's `@path` autocomplete to attach files. |
| `.extensions set <k> <v>` | not yet bridged | |
| `.exit role/session/rag/agent` | `/exit-context <kind>` | |
| `.exit` | pi-native `/quit` | |
| (no legacy equivalent) | `/aichat-flags [query]` | Discover CLI flags, optionally filtered. See [discovery.md](discovery.md). |
| (no legacy equivalent) | `/aichat-docs [name]` | List bundled feature docs, or print one inline. See [discovery.md](discovery.md). |
| `.help` | pi-native `/help` | Pi lists its own commands; the bridge commands appear under "Extensions". |

"Not yet bridged" entries land in follow-up phases. The endpoints are
already defined in `src/serve.rs` for `/role`, `/agent`, `/macro`, `/rag`,
`/aichat-session`, `/info`, `/exit-context`, `/edit`; the others ride on the
same HTTP contract once an extension command is registered.

### `/info`

```
/info               # current implicit context
/info role          # active role only
/info agent
/info session
/info rag
```

Equivalent to the legacy `.info` family. Output is the same export string
the legacy REPL printed.

### `/aichat-edit`

```
/aichat-edit config         # the aichat config.yaml
/aichat-edit role           # the active role's file
/aichat-edit rag-docs       # the active RAG's document paths (one per line)
/aichat-edit agent-config   # the active agent's config.yaml
```

The legacy `.edit` family spawned `$EDITOR` on a YAML file. Pi owns the
terminal, so `/aichat-edit` instead round-trips through **pi's own in-TUI
editor**: it reads the current text from the bridge, opens it for editing, and
POSTs the result back. Aichat then persists the file and applies the same
reload the legacy command did:

| Target | Reload behavior |
|---|---|
| `role` | Saved **and** reloaded into the live context immediately. |
| `rag-docs` | Document paths refreshed (re-embeds changed docs). |
| `config` | Written; restart aichat for changes to take effect. |
| `agent-config` | Written; reload the agent to apply. |

Editing requires an active context for the target: `/aichat-edit role` with no
role active returns an error telling you to `/role <name>` first (likewise for
`agent-config` and `rag-docs`). Cancelling pi's editor, or saving with no
change, leaves everything untouched.

`session` is **not** a target — pi owns the session format (its native JSONL
tree), so sessions are edited through pi's built-in `/session`, not the bridge.

## Sessions: pi owns the format

Pi sessions use the v3 JSONL tree format documented in pi's
[`docs/session-format.md`](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md).
**Where** they live depends on which of two parallel session-store modes is
active:

| Mode | Trigger | Pi session store | Models |
|---|---|---|---|
| **Segregated** (default) | model-pinned launch (the default) | `<config>/pi-sessions/` (aichat-owned; override `AICHAT_PI_SESSIONS_DIR`) | aichat's |
| **Native** | `AICHAT_PI_NATIVE_MODELS=1` | `~/.pi/agent/sessions/` (device-wide pi store) | pi's own |

In the **segregated** default, the launcher stages pi's agent dir with
`sessions/` symlinked to an aichat-owned directory instead of the device-wide
`~/.pi/agent/sessions/`. REPL history from aichat-driven pi launches therefore
never mixes with sessions from a bare `pi` invocation, and vice-versa. The
store is persistent (it outlives the throwaway agent stage), so pi's `/resume`
list carries across launches within this store. Point a launch at a different
store — e.g. per-project history — by exporting `AICHAT_PI_SESSIONS_DIR`.

In **native** mode (`AICHAT_PI_NATIVE_MODELS=1`), aichat does not stage an
agent dir at all: pi reads and writes its own device-wide
`~/.pi/agent/sessions/` store (and its own models). Use this when you want one
unified pi history shared with standalone `pi`.

`~/.pi/agent/sessions/` is pi's own default store, used in native mode and by
standalone `pi`.

uAichat's own sessions (`<config>/sessions/<name>.jsonl`) are now stored in the
**same pi v3 JSONL format**. A batch-mode session (`aichat -s <name> ...`) is
therefore already a pi session file — see [`session-format.md`](session-format.md)
for the format cutover, the `aichat` metadata header, and `--migrate-sessions`.
The legacy YAML format is deprecated and read-only.

To inspect or copy a single session into pi's own store, `--convert-session`
still emits pi JSONL to stdout or `--out PATH` without touching the original:

```bash
aichat --convert-session my-session --to pi --out ~/.pi/agent/sessions/imported.jsonl
# or stream to stdout, e.g. for inspection
aichat --convert-session ~/.config/aichat/sessions/my-session.yaml --to pi | head
```

Notes on the converted output:

- System-role messages are dropped; pi composes the system prompt from the
  active model and extension config at session start.
- Token usage and cost numbers are zeroed — aichat never recorded those
  per-message.
- Tool calls split into one assistant entry with `toolCall` content blocks
  plus one `toolResult` entry per call, matching pi's expected shape.
- The compression boundary, structured tool outputs (`aichatOutput`), and image
  URLs (`aichatUrl`) are preserved in pi-ignored side fields, so an aichat
  re-import is lossless; a pi loader simply ignores them. See
  [`session-format.md`](session-format.md).

## Bridge security

Each pi launch:

1. Probes `127.0.0.1:8000-9000` for an already-running aichat server it
   can authenticate against. If one is found it is **reused**; otherwise
   aichat binds an ephemeral port on `127.0.0.1` (no external listener)
   and starts a private in-process server.
2. For a private server, mints a 32-hex-char bearer token
   (`uuid::Uuid::simple()`, ~122 bits of entropy). For a reused server,
   uses the `AICHAT_BRIDGE_TOKEN` you exported (reuse only happens when
   that token authenticates against the target).
3. Exposes the URL + token to the spawned `pi` via `AICHAT_BRIDGE_URL`
   and `AICHAT_BRIDGE_TOKEN` env vars.
4. The aichat server rejects any `/v1/state/*` request without the
   matching `Authorization: Bearer <tok>` header. Token comparison is
   constant-time.

### Server discovery

The probe sends an authenticated `GET /v1/state/info` to each port and
classifies the response — reuse is deliberately strict, so every bridged
slash command is guaranteed to work against whatever server it attaches
to:

| Target | Response | Probe verdict |
|---|---|---|
| Non-aichat server, or `aichat --serve` with no bridge token | `404` | skip |
| aichat bridge, **different** token | `401` | skip |
| aichat bridge, **matching** token | `200` + `{"info": ...}` | **reuse** (lowest matching port wins) |

A token-less `aichat --serve` is therefore invisible to discovery: its
`/v1/state/*` routes return `404`, so the probe never attaches a REPL to
a server whose slash commands would all fail. Reuse requires
`AICHAT_BRIDGE_TOKEN` to be exported both for the long-lived server and
for the REPL session, and the two values must match.

- **Match found** — the discovered URL becomes the bridge URL; no
  in-process server is started, and the reused server is left running
  when the REPL exits.
- **No match** — aichat binds an ephemeral port, mints a fresh
  per-launch token, and starts a private in-process server it shuts down
  on exit.

Set `AICHAT_NO_SERVER_PROBE=1` to skip discovery entirely and always
start a private server.

The CLI `--serve` mode does **not** see these routes — when
`AICHAT_BRIDGE_TOKEN` is unset at server start, `/v1/state/*` returns 404.

For how to export `AICHAT_BRIDGE_TOKEN` to share one long-lived server
across REPL sessions, see [`server.md`](server.md).

## Model pinning

By default, pi resolves models from its own provider config
(`~/.pi/agent/models.json` + `settings.json`: Google, Anthropic, Ollama, …).
When aichat launches pi, it overrides that so **pi sees only aichat's
models** — every turn flows through aichat's inference, caching, roles, and
cost accounting rather than pi calling providers directly.

How it works:

1. After the bridge server is up, the launcher reads aichat's current model
   set (`list_all_models`) and default model.
2. It stages a throwaway pi agent dir (under the system temp dir) whose
   `models.json` registers a **single** provider, `aichat`, with
   `baseUrl: <bridge>/v1`, `api: openai-completions`, and a `models:` list of
   aichat's chat models (`contextWindow`/`maxTokens` carried over from each
   model's `max_input_tokens`/`max_output_tokens`). `settings.json` is copied
   from the user's real one with `defaultProvider`/`defaultModel` overridden to
   the aichat provider; all other prefs (theme, thinking level) are preserved.
3. Every other entry of the real agent dir (`auth.json`, themes, prompts) is
   symlinked into the stage, so pi config keeps working.
   `models.json`/`settings.json` are replaced, and `sessions/` is **not**
   symlinked through — it is pointed at the segregated aichat-owned session
   store instead (see [Sessions](#sessions-pi-owns-the-format)).
4. Pi is exec'd with `PI_CODING_AGENT_DIR` pointed at the stage. The stage is
   removed on exit (kept when `AICHAT_KEEP_PI_STAGE=1`).

Pi's `/model` picker (Ctrl+P) then lists only aichat models; `--list-models`
shows the `aichat` provider exclusively. Selecting `role:<name>` virtual models
still routes through the corresponding aichat role.

**Opt out** with `AICHAT_PI_NATIVE_MODELS=1` — pi then uses its own provider
config untouched, and turns call providers directly (no aichat caching/roles).
If staging fails for any reason the launcher logs a warning and falls back to
pi's native config rather than aborting the launch.

## Troubleshooting

**`pi not found on PATH`** — install pi per the instructions above. The
launcher prints the exact install commands on failure.

**Slash command returns 401 in pi** — usually a stale `aichat-bridge.js`
in `<cwd>/.pi/extensions/` from an earlier crashed launch. Remove it and
re-run:

```bash
rm -rf .pi/extensions/aichat-bridge.js
```

(The launcher cleans this up on a normal exit. To opt out of cleanup for
debugging, set `AICHAT_KEEP_PI_STAGE=1`.)

**`pi exited with status N`** — propagated verbatim from pi. `130` is
Ctrl-C; anything else is a pi-side error and pi's own logging (`pi
--debug`, `~/.pi/agent/logs/`) is the place to look.

**Pi sees the bridge env but slash commands aren't visible** — the
extension is staged into `<cwd>/.pi/extensions/`, project-scoped. If you
launched aichat from a directory you don't have write access to, staging
fails silently and the bridge becomes a no-op. Cd somewhere writable and
re-launch.

## Pinned versions

Tested against `@earendil-works/pi-coding-agent` ≥ 0.74.0. If pi changes
its extension API or RPC framing in a breaking way the bridge will need a
matching update; the failure mode is "slash commands no longer appear in
`/help`" rather than a crash.
