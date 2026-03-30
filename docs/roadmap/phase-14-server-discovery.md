# Phase 14: Server — Discovery & Estimation

**Status:** Planned
**Epic:** 3 — Server Pipeline Engine
**Design:** [epic-3.md](../analysis/epic-3.md)

---

| Item | Status | Notes |
|---|---|---|
| 14A. Cost estimation endpoint | — | `POST /v1/estimate` accepts `{"role": "...", "input": "...", "model": "..."}`. Returns `{estimated_input_tokens, estimated_output_tokens, estimated_cost_usd, alternatives: [...]}`. Alternatives list all configured models sorted by estimated cost, filtered by required capabilities. Zero LLM cost — pure arithmetic against `models.yaml`. |
| 14B. OpenAPI specification | — | `GET /v1/openapi.json` serves a static OpenAPI 3.0 spec documenting all endpoints, schemas, errors, and auth. Embedded in binary like `models.yaml`. |
| 14C. Root page | — | `GET /` returns lightweight HTML listing all endpoints with descriptions and links. Replaces current 404. |

**Parallelization:** All independent. 14B should be done last (documents endpoints from all other phases).

**Key files:** `src/serve.rs` (all items), new `assets/openapi.json` (14B).

---

## What NOT to build (server scope)

| Proposal | Reason |
|---|---|
| Multi-user auth (OAuth/LDAP) | Platform feature. Delegate to gateway. Bearer token (12B) is sufficient. |
| Conversation persistence / database | Server is stateless. Callers manage message history. OpenWebUI owns persistence. |
| Rich web UI / SPA beyond playground/arena | Violates "no desktop UI" constraint. Freeze at current scope. |
| WebSocket streaming | SSE works. WebSocket adds dependency for no capability gain. |
| Pipeline designer / role editor GUI | Roles are YAML files. Text editor is the authoring tool. |
| Agent hosting (long-running sessions) | `call_react` is single-invocation. Concurrent long-running agents need different architecture. |
| MCP-over-HTTP in `--serve` | MCP server (`--mcp`) uses stdio. MCPO bridges to HTTP. Keep separate. |
