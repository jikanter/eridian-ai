# Phase 16: Server Hardening : Overview - Epic 5

> **[UN-DEFERRED 2026-05-29]** 16A–E and 16I shipped, completing the server
> hardening surface. 16F/G/H shipped earlier (2026-05-11) as prerequisites
> for Phase 20's federation path. The whole phase is now **Done**.

| Item | Description | Status |
|---|---|---|
| 16A | Configurable CORS origins (`serve_cors_origins:`, `serve_cors_allow_all:` in config.yaml) | **Done** |
| 16B | Optional bearer token auth (`serve_api_key:`; `OPTIONS` + `GET /health` exempt) | **Done** |
| 16C | Health endpoint (`GET /health` → `{status, models, roles}`, unauthenticated) | **Done** |
| 16D | Streaming usage in final SSE chunk (`stream_options.include_usage` → trailing `usage` chunk with `cost_usd`) | **Done** |
| 16E | Hot-reload endpoint (`POST /v1/reload` → re-reads roles/prompts/rags from disk) | **Done** |
| 16F | Role metadata security (`RolePublicView` — hides prompt text, pipeline stage names, server-local wiring) | **Done** |
| 16G | Single-role retrieval (`GET /v1/roles/{name}`, 404 on miss) | **Done** |
| 16H | Cost in API responses (`X-AIChat-Cost-USD` header on `/v1/roles/{name}/invoke`, `/v1/pipelines/run`, `/v1/batch`) | **Done** |
| 16I | Playground refresh — `ask()` wrapped in `try/finally` so a thrown chat request can't leave the UI stuck (`asking` always resets) | **Done** |

Tests: unit (`src/serve.rs` — CorsPolicy, check_api_key, stream_options, usage
chunk) + integration ([`tests/integration/server-hardening.sh`](../../tests/integration/server-hardening.sh),
12 cases incl. a mock-SSE streaming-usage round-trip).

Demo: [`docs/demos/phase-16-server-hardening.md`](../demos/phase-16-server-hardening.md) —
runnable showboat walk-through of every knob (health, bearer auth, CORS,
hot-reload, streaming usage) against a live `aichat --serve`, capped by the
full integration suite.

## [Epic Details](./phase-16-server-hardening.md)
