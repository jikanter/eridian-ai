# Phase 16: Server — Hardening & Knowledge Exposure

**Status:** Planned
**Epic:** 5 — Server Pipeline Engine
**Design:** [epic-5.md](../analysis/epic-5.md)

---

> **[ADDED 2026-03-16]** Makes the server usable beyond localhost and exposes AIChat's knowledge model safely.
> AIChat's `--serve` already works as an OpenWebUI backend (OpenAI-compatible HTTP). These changes
> remove friction and fix data leakage without building platform features.
> Full design: [`docs/analysis/epic-5.md`](../analysis/epic-5.md)

| Item | Status | Notes |
|---|---|---|
| 16A. Configurable CORS origins | — | Replace hardcoded `is_local_origin()` with configurable `serve_cors_origins:` list in config.yaml. `serve_cors_allow_all: true` for trusted networks. Unblocks Docker/OpenWebUI bridge network access. |
| 16B. Optional bearer token auth | — | `serve_api_key:` in config.yaml. When set, checks `Authorization: Bearer <token>` on every request. Returns 401 on mismatch. When unset, no auth (current behavior). |
| 16C. Health endpoint | — | `GET /health` → `200 {"status": "ok", "models": N, "roles": N}`. Required for Docker/K8s/systemd orchestration. |
| 16D. Streaming usage in final SSE chunk | — | Add `usage` object (input_tokens, output_tokens, cost_usd) to the final SSE chunk. OpenWebUI relies on this for streaming token accounting. |
| 16E. Hot-reload endpoint | — | `POST /v1/reload` → reloads roles and models from disk without restart. Eliminates restart friction during role development. |
| 16F. Role metadata security (`RolePublicView`) | — | Replace full `Role` serialization in `/v1/roles` with `RolePublicView` that exposes: name, description, model_id, input_schema, output_schema, variable names (not shell commands), pipeline stage names. Hides: prompt text, pipe_to/save_to paths, MCP server configs, shell-injective defaults. |
| 16G. Single-role retrieval | — | `GET /v1/roles/{name}` returns `RolePublicView` for one role. `GET /v1/roles/{name}/schema` returns input/output schemas. Avoids listing all roles to find one. |
| 16H. Cost in API responses | — | Multiply `ModelData.{input,output}_price` × response tokens. Add `usage.cost_usd` to every `/v1/chat/completions` response. Add `X-AIChat-Cost-USD`, `X-AIChat-Model`, `X-AIChat-Latency-Ms` response headers. |

**Parallelization:** All items are independently implementable. 16A-16E are server infrastructure changes to `serve.rs`. 16F-16G are serialization changes. 16H is arithmetic. All can run in parallel.

**Key files:** `src/serve.rs` (all items), `src/config/mod.rs` (16A/16B config parsing).
