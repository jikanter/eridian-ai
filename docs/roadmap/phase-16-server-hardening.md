# Phase 16: Server — Hardening & Knowledge Exposure

**Status:** Done (2026-05-29)
**Epic:** 5 — Server Pipeline Engine
**Design:** [epic-5.md](../analysis/epic-5.md)

> **[DONE 2026-05-29]** 16A–E and 16I landed in `src/serve.rs` (config keys
> in `src/config/mod.rs`), completing the hardening surface. 16F/G/H shipped
> earlier alongside Phase 20 federation. Tests: unit in `src/serve.rs` +
> [`tests/integration/server-hardening.sh`](../../tests/integration/server-hardening.sh).
> 16J/F7/F8 (OpenAPI spec, cost-estimation endpoint) were never part of
> Phase 16's table and remain unbuilt — see Phase 18 / [epic-5.md](../analysis/epic-5.md).

---

> **[ADDED 2026-03-16]** Makes the server usable beyond localhost and exposes AIChat's knowledge model safely.
> AIChat's `--serve` already works as an OpenWebUI backend (OpenAI-compatible HTTP). These changes
> remove friction and fix data leakage without building platform features.
> Full design: [`docs/analysis/epic-5.md`](../analysis/epic-5.md)

| Item | Status | Notes                                                                                                                                                                                                                                                                                              |
|---|---|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| 16A. Configurable CORS origins | **Done** | `CorsPolicy` replaces the hardcoded `is_local_origin()` gate: localhost is always allowed, `serve_cors_origins:` widens the allowlist, `serve_cors_allow_all: true` echoes any origin. Unblocks Docker/OpenWebUI bridge network access. |
| 16B. Optional bearer token auth | **Done** | `serve_api_key:` in config.yaml. When set, `check_api_key` enforces `Authorization: Bearer <key>` → 401 on mismatch. `OPTIONS` and `GET /health` exempt; the `/v1/state/*` bridge keeps its own token. Unset → no auth (historical behavior). |
| 16C. Health endpoint | **Done** | `GET /health` → `200 {"status":"ok","models":N,"roles":N}`, unauthenticated. `models` excludes `role:*` virtual models. Required for Docker/K8s/systemd orchestration. |
| 16D. Streaming usage in final SSE chunk | **Done** | `stream_options.include_usage` (OpenAI semantics) → trailing usage-only chunk (`choices:[]`) with `prompt/completion/total_tokens` + `cost_usd` before `[DONE]`. aichat injects `stream_options` upstream; falls back to 0 when the provider doesn't report. |
| 16E. Hot-reload endpoint | **Done** | `POST /v1/reload` → re-reads roles/prompts/rags from disk and rebuilds the model listing (`Server.listing` is `RwLock<Listing>`). Returns `{roles, models}`. Provider `clients:` changes still need a restart. |
| 16F. Role metadata security (`RolePublicView`) | **Done** | Full `Role` serialization in `/v1/roles` replaced with `RolePublicView`: exposes name, description, model_id, input/output schema, variable names, pipeline stage names; hides prompt text, pipe_to/save_to, MCP configs, shell defaults. |
| 16G. Single-role retrieval | **Done** | `GET /v1/roles/{name}` returns `RolePublicView` for one role, 404 on miss. |
| 16H. Cost in API responses | **Done** | `X-AIChat-Cost-USD` header on `/v1/roles/{name}/invoke`, `/v1/pipelines/run`, `/v1/batch`; `usage.cost_usd` in role/pipeline envelopes. |
| 16I. Playground Refresh | **Done** | `ask()` in `assets/playground.html` wrapped in `try/finally` so `asking` (and `askAbortController`) always reset. Previously a throw before/around the stream left `asking` stuck `true`, and `handleAsk()`'s `if (this.asking) return` guard froze the UI. |

**Implementation:** All items are server-side changes to `src/serve.rs`; 16A/16B config keys parse in `src/config/mod.rs`; 16I is a JS fix in `assets/playground.html`. Tests: unit (`serve.rs` — `CorsPolicy`, `check_api_key`, `stream_options`, usage chunk) and integration ([`tests/integration/server-hardening.sh`](../../tests/integration/server-hardening.sh), 12 cases including a Python mock-SSE streaming-usage round-trip).

**Key files:** `src/serve.rs` (all server items), `src/config/mod.rs` (16A/16B config parsing), `assets/playground.html` (16I).

**Demo:** [`docs/demos/phase-16-server-hardening.md`](../demos/phase-16-server-hardening.md) — a runnable showboat walk-through that exercises each knob against a live server and finishes with the full integration suite. User-facing reference: [`docs/features/server.md`](../features/server.md).

**16I Bug description:** In some cases the Playground UI becomes unresponsive — typically during a chat session while the server is responding. Root cause: `buildBody()` (and any other throw) sat outside the try block, and `this.asking = false` ran only at the function tail, so an exception left the send/input path permanently disabled.