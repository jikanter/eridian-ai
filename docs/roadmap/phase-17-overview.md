# Phase 17: Role & Pipeline Execution : Overview - Epic 5

> **[UN-DEFERRED 2026-05-11]** All five items shipped as the server-side
> half of Phase 20 (Universal Addressing — federated composition). Phase 18
> remains deferred until discovery / cost-estimation pressure emerges.

| Item | Description | Status |
|---|---|---|
| 17A | Roles as virtual models — `model: "role:NAME"` in `/v1/chat/completions` routes through `chat_completions_via_role`, which extracts the last user message and runs `invoke_role`. Streaming chunk emits the full output then `[DONE]`. | **Done** |
| 17B | Role invocation endpoint — `POST /v1/roles/{name}/invoke` with `{input, variables?, model?, trace?}`. Returns `{output, usage, schema_valid, trace?}` plus an `X-AIChat-Cost-USD` header. 404 before reading the body when the role doesn't exist. | **Done** |
| 17C | Streaming invoke — same endpoint with `stream: true` in the body. SSE events: `stage.start`, `stage.end` (per stage), `done` (final aggregate), `data: [DONE]`. Stage-granular, not token-granular. | **Done** |
| 17D | Pipeline execution endpoint — `POST /v1/pipelines/run` with either inline `stages` or a named `pipeline:` from `<config>/pipelines/<name>.yaml`. Mutually exclusive. | **Done** |
| 17E | Batch endpoint — `POST /v1/batch` applies a role, inline stages, or named pipeline to a list of `inputs`. Bounded concurrency (default 4, clamped to [1, 32]). Per-item errors land in the item's `error` field and don't fail the batch. | **Done** |

## 17A Design — Virtual Models

Every locally-known role appears in `/v1/models` as `{id: "role:NAME", owned_by: "aichat-role"}`. OpenWebUI sees them in its model dropdown. Selecting `role:code-reviewer` transparently runs `invoke_role`, returning a normal `chat.completion` envelope. The role's own model + pipeline win over the request's `temperature` / `top_p` / `tools`.

## 17B Design — Invoke Endpoint

```json
POST /v1/roles/classify/invoke
{
  "input": "Review this code for security issues...",
  "variables": {"language": "rust"},
  "model": "deepseek:deepseek-chat",
  "trace": true
}
```

Response:
```json
{
  "output": "...",
  "usage": {
    "input_tokens": 1234,
    "output_tokens": 567,
    "cost_usd": 0.00123,
    "latency_ms": 890,
    "model": "deepseek:deepseek-chat"
  },
  "schema_valid": true,
  "trace": { "stages": [...] }
}
```

`X-AIChat-Cost-USD` header carries the same `cost_usd` for proxies that strip bodies.

## 17C Design — Streaming Invoke

```text
event: stage.start
data: {"index":0,"total":2,"role":"extract","model":null}

event: stage.end
data: {"index":0,"role":"extract","trace":{...},"output":"..."}

event: done
data: {"output":"...","usage":{...},"schema_valid":true}

data: [DONE]
```

`done` carries the full final output, so simple consumers can ignore the `stage.*` events and only read the last frame.

## 17D Design — Pipelines Endpoint

```json
POST /v1/pipelines/run
{
  "input": "...",
  "stages": [{"role": "extract"}, {"role": "summarize", "model": "claude-haiku"}],
  "trace": true
}
```

or named:
```json
{ "input": "...", "pipeline": "summarize-then-rate" }
```

Reuses `pipe::run_inline_pipeline`. Trace is always emitted.

## 17E Design — Batch

```json
POST /v1/batch
{
  "inputs": ["text1", "text2", ...],
  "role": "classify",
  "concurrency": 4
}
```

Returns an ordered `results` array plus an aggregate `usage`. Bounded concurrency via `tokio::sync::Semaphore` so one batch doesn't blow past a provider's rate budget. Per-item failures carry through as `{"error": "msg", ...}` items.

## Files

- `src/serve.rs` — all five HTTP routes, request-body structs, streaming SSE forwarder
- `src/pipe.rs` — `invoke_role`, `invoke_role_streaming`, `run_inline_pipeline`, `load_pipeline_stages`, `InlineStage`, `StageTrace`, `StageEvent`, `InvokeResult` (all pub)
- `src/config/role.rs` — `RolePublicView` (Phase 16F) used by GET endpoints
- `tests/integration/federation.sh` — 10/13 tests target Phase 17 endpoints
