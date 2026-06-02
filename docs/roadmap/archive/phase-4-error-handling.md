# Phase 4: Error Handling & Schema Fidelity

**Status:** Done

---

| Item | Status | Commit | Notes |
|---|---|---|---|
| 4A. Stop silent data loss | Done | — | 6 `.ok()` swallowing sites in `role.rs` replaced with `warn!()` on parse failure |
| 4B. Structured error types (`AichatError`) | Done | — | `AichatError` enum in `exit_code.rs` with `SchemaValidation`, `ConfigParse`, `ToolNotFound`, `PipelineStage`, `McpError` variants. `classify_error()` fast-path via `downcast_ref`. |
| 4C. Structured error output (`-o json`) | Done | — | Errors emit `{"error": {"code", "category", "message", "context"}}` on stderr when `-o json` active |
| 4D. Fix `JsonSchema` lossiness | Done | — | `FunctionDeclaration.parameters` changed from `JsonSchema` (8 keywords) to `serde_json::Value` (full fidelity). MCP tools now preserve `oneOf`, `allOf`, `$ref`, `additionalProperties`, etc. |
| 4E. Pipeline stage tracebacks | Done | — | Pipeline failures include stage number, total, role name, model ID via `AichatError::PipelineStage` |

Phase 4 enables cheap error recovery for agents consuming aichat. The [tool analysis](../analysis/2026-03-10-tool-analysis.md) argues that aichat should be "the cheapest tool an agent can reach for" — cheap invocation now pairs with cheap error recovery via structured JSON error payloads and semantic exit codes.

**Exit code reference** (from `src/utils/exit_code.rs`):

| Code | Meaning | Agent recovery hint |
|---|---|---|
| 0 | Success | — |
| 1 | General / unknown | Retry or escalate |
| 2 | Usage / invalid input | Fix invocation syntax |
| 3 | Config / role not found | Check role name, config path |
| 4 | Auth / API key | Re-authenticate |
| 5 | Network / connection | Retry with backoff |
| 6 | API response error | Check rate limits, model availability |
| 7 | Model error | Switch model or reduce input |
| 8 | Schema validation failed | Fix input/output against schema |
| 9 | Aborted by user | Do not retry |
| 10 | Tool / function error | Check tool availability, arguments |
