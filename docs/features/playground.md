# The browser playground

The playground is a single-page chat UI that the [aichat HTTP
server](server.md) serves alongside its API. It is a convenient way to
try models, roles, and RAGs interactively without leaving the browser —
the same inference paths the CLI uses, behind a chat window.

## Opening it

Start a server, then open the playground in a browser:

```bash
aichat --serve                       # 127.0.0.1:8000 by default
open http://127.0.0.1:8000/playground
```

`/playground` and `/playground.html` both serve the page. Its sibling,
[`/arena`](server.md), runs the same models side by side for comparison.

The playground is a static page — all logic runs in the browser and
talks to the server over the OpenAI-compatible API. Nothing is installed
and no build step is involved; the HTML ships inside the `aichat` binary.

## The interface

The sidebar holds per-request **settings**; the main panel is the chat.

| Control | Effect |
|---|---|
| **Model** (chat-header) | The model the request runs against. Populated from `/v1/models`. |
| **RAG** | Augment each message with retrieved context — see [RAG](#rag) below. Populated from `/v1/rags`. |
| **Role** | Run the chat through a saved [role](../architecture/architecture.md). Populated from `/v1/roles`. |
| **System Prompt** | A free-text system prompt; supports the [structured-prompt syntax](#structured-prompts). |
| **Max Output Tokens** | Caps response length. The label shows the model's ceiling when known. |
| **Temperature** / **Top P** | Standard sampling controls. Left blank → the model/role default. |

Picking a **Role** takes precedence over the **Model** select: the
request is sent with `model: role:<name>`, so the role's own pinned
model, temperature, and prompt apply.

Two sidebar buttons manage conversations:

- **New Chat** (`Ctrl/Cmd+Shift+O`) — archives the current conversation
  and starts a fresh one.
- **List Sessions** (`Ctrl/Cmd+Shift+L`) — reopens an archived
  conversation.

Once a conversation has at least one successful exchange it enters
**session mode**: the RAG, Role, and System Prompt controls lock so the
context stays consistent for the rest of that chat. Start a New Chat to
change them.

> **Sessions are in-memory only.** The session list lives in the page
> and is lost on reload or when the tab closes. It is not the same as an
> aichat [session](../architecture/architecture.md) on disk. To persist a
> conversation, use the CLI or REPL.

Paste an image into the input box to send it to a vision-capable model.

## Sharing a configuration via the URL

The playground reads its initial state from query-string parameters and
writes the live settings back into the URL as you change them. That
makes any configured playground **bookmarkable and shareable** — copy
the address bar and the recipient lands on the same setup.

| Parameter | Purpose |
|---|---|
| `model` | Pre-select a model. |
| `role` | Pre-select a role. |
| `rag` | Pre-select a RAG. |
| `max_output_tokens`, `temperature`, `top_p` | Pre-fill sampling settings. |
| `api_base` | Point the page at a different API root (default `/v1`). |
| `api_key` | Bearer token sent with every request (default: none). |

Example:

```
http://127.0.0.1:8000/playground?role=coder&temperature=0.2
```

`model`, `role`, `rag`, `max_output_tokens`, `temperature`, and `top_p`
are kept in sync with the URL automatically. `api_base` and `api_key`
are read once at load time.

## RAG

When a RAG is selected, the playground does a retrieval step before each
turn: it sends your latest message to `/v1/rags/search` and replaces it
with the augmented text (your question plus the retrieved passages)
before calling the model. The retrieval is transparent — you only see
your original message in the transcript.

## Structured prompts

The **System Prompt** box understands two authoring conveniences:

- **`__INPUT__` placeholder** — if the first user message contains
  `__INPUT__`, the system prompt is spliced in around it. Useful for
  wrapping every input in a fixed template.
- **`### INPUT:` / `### OUTPUT:` blocks** — text after the system prompt
  split into alternating `### INPUT:` and `### OUTPUT:` sections is
  turned into few-shot example turns. The leading text becomes the
  system message; each INPUT/OUTPUT pair becomes a user/assistant
  example.

```
You are a terse assistant.

### INPUT:
capital of France?
### OUTPUT:
Paris.
```

## Authentication

The playground and the API routes it uses (`/v1/chat/completions`,
`/v1/models`, `/v1/roles`, `/v1/rags`, `/v1/rags/search`) are **served
without authentication** by `aichat --serve`. The `api_key` query
parameter exists only for the case where you re-point the page at an
*external* OpenAI-compatible backend with `api_base` — aichat's own
server ignores it.

The token-gated [`/v1/state/*` bridge](server.md) (`AICHAT_BRIDGE_TOKEN`)
is a separate surface used by the [pi REPL](repl-pi.md) to mutate live
server state. The playground never calls those routes — it passes the
role and RAG with each request instead of changing server state — so it
needs no bridge token, and starting the server with or without
`AICHAT_BRIDGE_TOKEN` does not affect the playground.

## See also

- [The aichat HTTP server](server.md) — the server, the arena, and the bridge.
- [The pi REPL](repl-pi.md) — the interactive terminal surface.
- [`src/serve.rs`](../../src/serve.rs) — the server implementation.