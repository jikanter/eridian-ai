# Phase 17: Server — Role & Pipeline Execution

**Status:** Deferred (2026-04-17)
**Epic:** 5 — Server Pipeline Engine
**Design:** [epic-5.md](../analysis/epic-5.md)

> **[DEFERRED 2026-04-17]** Epic 5 (Phases 16, 17, 18) is explicitly parked
> while Epic 9 (Knowledge Evolution) is in flight. Returning to the server
> execution surface is a future-session decision.

---

> **[ADDED 2026-03-16]** Exposes AIChat's unique capabilities over HTTP. Turns the server from a
> commodity proxy into a pipeline execution engine and role-as-API gateway.
> Full design: [`docs/analysis/epic-5.md`](../analysis/epic-5.md)

| Item | Status | Notes |
|---|---|---|
| 17A. Roles as virtual models | — | Roles appear as `role:{name}` in `/v1/models` listing. When `POST /v1/chat/completions` receives `"model": "role:classify"`, the server resolves the role, executes full machinery (schema validation, pipeline, tools), returns standard OpenAI response. Zero-change OpenWebUI integration — roles become selectable "models." |
| 17B. Role invocation endpoint (non-streaming) | — | `POST /v1/roles/{name}/invoke` accepts `{"input": "...", "variables": {...}, "trace": true}`. Validates against `input_schema`, executes role/pipeline, validates `output_schema`, returns output with cost and trace metadata. 422 for schema failures. |
| 17C. Role invocation endpoint (streaming) | — | Streaming variant of 17B with SSE stage-boundary events: `stage_start`, `delta`, `stage_end`, `done`. Requires refactoring `pipe.rs` to emit events via callback/channel instead of printing to stdout. |
| 17D. Pipeline execution endpoint | — | `POST /v1/pipelines/run` accepts named pipeline or inline `{"stages": [...]}`. Reuses `pipe.rs:run_pipeline_role()`. Returns Phase 8A2 trace envelope format. |
| 17E. Batch processing endpoint | — | `POST /v1/batch` accepts `{"role": "classify", "records": [...], "parallel": 4}`. Returns JSONL-shaped results. HTTP equivalent of `--each`. Per-record errors, not per-batch. Depends on Phase 8B (`--each`) landing first. |

**Parallelization:** 17A is independent. 17B is independent (non-streaming path). 17C depends on 17B and shares a `pipe.rs` refactor with 17D. 17D is independent of 17A/17B. 17E depends on Phase 8B.

**Recommended order:** 17A → 17B → 17D → 17C → 17E

**Pipe.rs refactoring note:** 17C and 17D both need `pipe.rs` to emit stage events rather than writing to stdout. This is a shared refactor: add an optional `stage_event_sender: Option<Sender<StageEvent>>` parameter to `run_stage`. When present, emit events. When absent, print as today. This preserves CLI behavior.

**Key files:** `src/serve.rs` (all items), `src/pipe.rs` (17C/17D stage-event refactor), `src/config/role.rs` (17A role resolution).
