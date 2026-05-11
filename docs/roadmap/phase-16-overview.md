# Phase 16: Server Hardening : Overview - Epic 5

> **[PARTIALLY UN-DEFERRED 2026-05-11]** 16F (`RolePublicView`), 16G
> (`GET /v1/roles/{name}`), and 16H (`X-AIChat-Cost-USD`) shipped as
> prerequisites for Phase 20's federation path. 16A/B/C/D/E remain
> deferred until the server surface needs broader hardening.

| Item | Description | Status |
|---|---|---|
| 16A | Configurable CORS origins (`serve_cors_origins:` in config.yaml) | -- |
| 16B | Optional bearer token auth (`serve_api_key:`) | -- |
| 16C | Health endpoint (`GET /health`) | -- |
| 16D | Streaming usage in final SSE chunk | -- |
| 16E | Hot-reload endpoint (`POST /v1/reload`) | -- |
| 16F | Role metadata security (`RolePublicView` — hides prompt text, pipeline stage names, server-local wiring) | **Done** |
| 16G | Single-role retrieval (`GET /v1/roles/{name}`, 404 on miss) | **Done** |
| 16H | Cost in API responses (`X-AIChat-Cost-USD` header on `/v1/roles/{name}/invoke`, `/v1/pipelines/run`, `/v1/batch`) | **Done** |

## [Epic Details](./phase-16-server-hardening.md)
