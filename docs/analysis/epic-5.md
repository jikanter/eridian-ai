# Epic 5: Server Pipeline Engine

**Created:** 2026-03-16
**Updated:** 2026-04-07 (renumbered from Epic 3; phases 12-14 → 16-18)
**Status:** Planning
**Depends on:** Phase 8A (cost accounting), Phase 9 (schema fidelity)

---

## Motivation

AIChat's `--serve` mode (963 lines, `src/serve.rs`) is currently a commodity OpenAI-compatible proxy. It exposes chat completions, embeddings, rerank, model listing, and two embedded HTML UIs (playground, arena). Through this API, a consumer gets the same experience as connecting to LiteLLM or OpenRouter — none of AIChat's distinctive features (roles, pipelines, schema validation, cost database, tool ecosystem) are accessible.

The entire role system, pipeline engine, and schema validation machinery exist in `pipe.rs`, `role.rs`, and `function.rs` but are wired only to CLI entry points (`main.rs`). The server passes messages straight through to providers without role resolution, schema enforcement, pipeline execution, or cost calculation.

This epic exposes AIChat's unique runtime capabilities over HTTP, turning the server from a proxy into a **pipeline execution engine and role-as-API gateway**.

### Why Not Just Integrate With OpenWebUI?

AIChat's `--serve` **already works as an OpenWebUI backend today** — it speaks OpenAI-compatible HTTP. OpenWebUI can point at `http://host:8000` and get model listing + chat completions. No code changes needed.

What OpenWebUI **cannot** see through this interface: roles, pipelines, schema validation, cost data, tool ecosystem, variables. These are AIChat-specific concepts with no OpenAI API equivalent.

The correct architecture is **protocol compatibility without coupling**: AIChat owns the API and runtime logic; OpenWebUI (or any frontend) is an optional consumer. A client that knows about AIChat's extended endpoints gets more; a client that doesn't still works via standard OpenAI API.

### What OpenWebUI Provides That AIChat Should Never Build

- Multi-user authentication and authorization
- Persistent conversation database (SQLite/PostgreSQL)
- Rich web UI with component framework (SvelteKit)
- Plugin marketplace / function registry
- Document upload and management UI
- Collaborative features, workspace management
- Image generation, voice, web search integrations

---

## Feature 1: Roles as Virtual Models

### Problem

Roles are invisible to any OpenAI-compatible consumer. OpenWebUI, LiteLLM, and any standard client see only raw models via `/v1/models`. A user with 20 carefully crafted roles gets zero value from them through the API.

### Solution

Expose roles as virtual models in the `/v1/models` listing. When `/v1/chat/completions` receives `"model": "role:classify"`, the server intercepts it, resolves the role, executes the full role machinery (variable interpolation, schema validation, pipeline stages, tool binding), and returns the result through the standard completions response format.

### Implementation

**Model listing** (`src/serve.rs`, `list_models` function):

Append role-based virtual models to the model list:
```rust
// After listing real models, add roles as virtual models
for role in &self.roles {
    if !role.is_empty_prompt() {  // Only expose non-trivial roles
        models.push(json!({
            "id": format!("role:{}", role.name()),
            "object": "model",
            "owned_by": "aichat-role",
            "description": role.description(),
            "has_input_schema": role.input_schema().is_some(),
            "has_output_schema": role.output_schema().is_some(),
            "has_pipeline": role.pipeline().is_some(),
        }));
    }
}
```

**Chat completions dispatch** (`src/serve.rs`, `chat_completions` method):

Before normal model dispatch, check if the model starts with `role:`:
```rust
if model_id.starts_with("role:") {
    let role_name = &model_id["role:".len()..];
    return self.invoke_role(role_name, req_body, abort_signal).await;
}
```

The `invoke_role` method:
1. Resolves the role via `config.retrieve_role(name)`
2. Extracts the last user message as input text
3. Runs through the same path as CLI: schema validation → pipeline/single-stage execution → output validation
4. Returns standard OpenAI chat completion response format

**OpenWebUI integration effect**: Roles appear as selectable "models" in OpenWebUI's model dropdown. Selecting `role:code-reviewer` transparently executes the full role pipeline. Zero changes to OpenWebUI.

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | Add role entries to `list_models`; add role dispatch in `chat_completions`; new `invoke_role` method |

### Effort

Medium. ~100-150 lines in `serve.rs`. The role execution logic already exists in `pipe.rs` and `main.rs` — the work is routing and response formatting.

### Parallelization

Independent of Features 2-7. Can be implemented by one agent.

---

## Feature 2: Role Invocation Endpoint

### Problem

The `role:name` virtual model approach (Feature 1) works through the OpenAI API contract but cannot express variables, structured input matching `input_schema`, or return pipeline trace metadata. A richer endpoint is needed for API-native consumers.

### Solution

`POST /v1/roles/{name}/invoke` — a dedicated endpoint that accepts structured input, validates against schemas, executes the role (including pipeline stages), and returns output with full metadata.

### Implementation

**Request**:
```json
{
  "input": "the user input text or JSON matching input_schema",
  "variables": {"key": "value"},
  "model": "deepseek:deepseek-chat",
  "stream": false,
  "trace": true
}
```

- `input`: Required. Text or JSON. Validated against `input_schema` if defined.
- `variables`: Optional. Merged with role's declared variables. Shell-injective defaults execute on the server.
- `model`: Optional. Overrides the role's model. Supports `fallback_models` (Phase 10D).
- `stream`: Optional. Default false. When true, SSE stream with stage-boundary events.
- `trace`: Optional. Default false. When true, include trace in response.

**Response** (non-streaming):
```json
{
  "output": "the validated output",
  "role": "classify",
  "model": "deepseek:deepseek-chat",
  "usage": {
    "input_tokens": 892,
    "output_tokens": 341,
    "cost_usd": 0.0003
  },
  "schema_valid": true,
  "trace": {
    "stages": [
      {"role": "extract", "model": "deepseek:deepseek-chat", "input_tokens": 500, "output_tokens": 200, "cost_usd": 0.0001, "latency_ms": 800},
      {"role": "review", "model": "claude:claude-sonnet-4-6", "input_tokens": 200, "output_tokens": 141, "cost_usd": 0.012, "latency_ms": 1500}
    ],
    "total_cost_usd": 0.0121,
    "total_latency_ms": 2300
  }
}
```

**Error responses**:
- 404: Role not found
- 422: Input schema validation failed (structured error with field-level details, reusing Phase 4C format)
- 500: Pipeline stage failure (includes stage traceback per Phase 4E)

**Streaming response** (SSE):
```
event: stage_start
data: {"stage": 1, "role": "extract", "model": "deepseek:deepseek-chat"}

event: delta
data: {"content": "partial output..."}

event: stage_end
data: {"stage": 1, "input_tokens": 500, "output_tokens": 200, "cost_usd": 0.0001}

event: stage_start
data: {"stage": 2, "role": "review", "model": "claude:claude-sonnet-4-6"}

event: delta
data: {"content": "more output..."}

event: stage_end
data: {"stage": 2, "input_tokens": 200, "output_tokens": 141, "cost_usd": 0.012}

event: done
data: {"total_cost_usd": 0.0121, "schema_valid": true}
```

### Schema validation integration

The endpoint uses existing `validate_schema()` from `role.rs:835-848`:
- Before LLM call: validate `input` against `input_schema` → 422 on failure
- After LLM call: validate output against `output_schema` → retry per Phase 9C, then 500 if still invalid

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | New route handler, request parsing, response formatting, SSE stage events |
| `src/pipe.rs` | Refactor `run_stage` to emit stage events (callback or channel) instead of printing to stdout |

### Effort

Medium-large. ~200-300 lines. The main complexity is the streaming variant with stage-boundary SSE events, which requires refactoring `pipe.rs` to emit events rather than printing directly.

### Parallelization

Depends on Feature 1 for shared role resolution logic. The streaming and non-streaming paths can be developed by separate agents.

---

## Feature 3: Role Metadata Endpoint

### Problem

The current `/v1/roles` endpoint serializes the full `Role` struct, including the raw prompt text. This is a data leak for roles with proprietary instructions. Additionally, there is no single-role retrieval — consumers must list all roles and filter client-side.

### Solution

A `RolePublicView` that exposes the API contract without leaking implementation details, plus individual role retrieval.

### Implementation

**New struct**: `RolePublicView`
```rust
struct RolePublicView {
    name: String,
    description: Option<String>,
    model_id: Option<String>,
    input_schema: Option<Value>,
    output_schema: Option<Value>,
    variables: Vec<VariablePublicView>,  // name + type only, no shell commands
    pipeline: Option<Vec<PipelineStageView>>,  // role names + models only
    extends: Option<String>,
    use_tools: Option<String>,
    examples: Option<Vec<Value>>,
}

struct VariablePublicView {
    name: String,
    description: Option<String>,
    has_default: bool,
    default_type: String,  // "value" | "shell" (but NOT the shell command itself)
}

struct PipelineStageView {
    role: String,
    model: Option<String>,
}
```

**Excluded from public view**: `prompt` (proprietary instructions), `pipe_to`/`save_to` (filesystem paths), `mcp_servers` (infrastructure details), shell variable default commands.

**Endpoints**:
- `GET /v1/roles` — returns list of `RolePublicView` (replaces current full serialization)
- `GET /v1/roles/{name}` — returns single `RolePublicView`
- `GET /v1/roles/{name}/schema` — returns `{"input_schema": ..., "output_schema": ...}` (convenience for programmatic consumers)

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | Replace current `list_roles` serialization; add single-role and schema endpoints |

### Effort

Small. ~80-100 lines. Straightforward struct definition and serialization.

### Parallelization

Fully independent of all other features.

---

## Feature 4: Pipeline Execution Endpoint

### Problem

Pipelines — AIChat's most distinctive feature — cannot be invoked via HTTP. The `/v1/chat/completions` endpoint does not execute pipeline stages, and there is no pipeline-specific endpoint.

### Solution

`POST /v1/pipelines/run` — accepts named or inline pipeline definitions, executes them, and returns results with per-stage trace metadata.

### Implementation

**Request** (named pipeline):
```json
{
  "pipeline": "extract-review-format",
  "input": "Review this code for security issues...",
  "variables": {"language": "rust"},
  "stream": true
}
```

**Request** (inline stages):
```json
{
  "stages": [
    {"role": "extract", "model": "deepseek:deepseek-chat"},
    {"role": "review", "model": "claude:claude-sonnet-4-6"},
    {"role": "format"}
  ],
  "input": "...",
  "stream": false
}
```

Named pipelines resolve to roles with `pipeline:` stages. Inline stages are ad-hoc — the same as CLI `--stage extract@deepseek --stage review@claude`.

**Response**: Same format as Feature 2 (role invocation), with the trace envelope showing per-stage breakdown.

**Implementation path**: The endpoint deserializes the request, constructs `PipelineStage` structs matching `pipe.rs`'s internal format, and calls `run_pipeline_role()` (pipe.rs:240-276) or the equivalent stage loop. The HTTP handler is thin routing; the execution engine already exists.

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | New route handler, request parsing |
| `src/pipe.rs` | Expose `run_pipeline_role` or equivalent for server consumption (may need to accept a callback/channel for stage events) |

### Effort

Medium. ~100-150 lines. Most of the work is already done in `pipe.rs`. The inline-stages path reuses `parse_stages()`.

### Parallelization

Independent of Features 1, 3, 5, 6, 7. Shares the stage-event refactoring with Feature 2's streaming variant.

---

## Feature 5: Cost in API Responses

### Problem

`ModelData` carries `input_price`/`output_price`. Every API response carries `input_tokens`/`output_tokens`. The multiplication never happens in server mode. The roadmap (Phase 8A1) connects them for CLI but not for `--serve`.

### Solution

Add `cost_usd` to the `usage` object in every `/v1/chat/completions` response, plus `X-AIChat-Cost-USD` response header on all endpoints.

### Implementation

In `serve.rs`, after computing the response:
```rust
let cost_usd = if let (Some(input_price), Some(output_price)) =
    (model.data.input_price, model.data.output_price)
{
    Some(
        (output.input_tokens as f64 * input_price / 1_000_000.0)
        + (output.output_tokens as f64 * output_price / 1_000_000.0)
    )
} else {
    None
};

// Add to usage object
if let Some(cost) = cost_usd {
    usage["cost_usd"] = json!(cost);
}

// Add as response header
if let Some(cost) = cost_usd {
    headers.insert("X-AIChat-Cost-USD", cost.to_string().parse()?);
}
```

Also add: `X-AIChat-Model`, `X-AIChat-Input-Tokens`, `X-AIChat-Output-Tokens`, `X-AIChat-Latency-Ms` headers. These are zero-overhead for callers that ignore them and immediately useful for callers that want accounting.

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | Cost multiplication in response building; response headers on all endpoints |

### Effort

Small. ~30-50 lines. The data exists; this is just arithmetic and serialization.

### Parallelization

Fully independent. Can be done in parallel with everything else. Benefits from Phase 8A1 landing first (shared cost calculation logic), but can be implemented independently.

---

## Feature 6: Server Hardening

### Problem

The server lacks basic production safety features: no authentication, no health endpoint, CORS restricted to exact localhost strings (blocks Docker bridge networks), no streaming usage reporting.

### Solution

Minimal additions that make the server usable beyond local development without adding platform complexity.

### Implementation

**6A. Configurable CORS** (~15 lines):
```yaml
# config.yaml
serve_cors_origins:
  - "http://localhost:3000"    # OpenWebUI dev
  - "http://host.docker.internal:3000"
  # or
serve_cors_allow_all: true     # for trusted networks
```

Replace the hardcoded `is_local_origin()` check with a configurable origin list.

**6B. Optional bearer token auth** (~25 lines):
```yaml
# config.yaml
serve_api_key: "sk-my-secret-key"
```

When set, the server checks `Authorization: Bearer <token>` on every request. Returns 401 on mismatch. When not set, no auth (current behavior).

**6C. Health endpoint** (~10 lines):
```
GET /health
→ 200 {"status": "ok", "models": 42, "roles": 15}
```

**6D. Streaming usage** (~30 lines):

Add `usage` to the final SSE chunk in streaming mode:
```
data: {"choices":[],"usage":{"prompt_tokens":892,"completion_tokens":341,"cost_usd":0.012}}
```

OpenAI's API includes this when `stream_options: {"include_usage": true}`. OpenWebUI relies on it.

**6E. Hot-reload endpoint** (~20 lines):
```
POST /v1/reload
→ 200 {"roles": 15, "models": 42}
```

Reloads roles and models from disk without restarting the server. Eliminates restart friction during role development.

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | CORS config, auth check, health endpoint, streaming usage, reload |
| `src/config/mod.rs` | Parse `serve_cors_origins`, `serve_api_key` config fields |

### Effort

Small. ~100 lines total across all sub-features.

### Parallelization

6A-6E are all independent of each other and of other features. Can be split across up to 5 agents, though bundling them as one task for a single agent is more practical.

---

## Feature 7: Cost Estimation Endpoint

### Problem

There is no way to preview the cost of a role or pipeline execution before committing to it. For agents with budgets, this is a "should I even make this call?" gate.

### Solution

`POST /v1/estimate` — returns token and cost estimates without making any LLM call.

### Implementation

**Request**:
```json
{
  "role": "classify",
  "input": "some text to classify",
  "model": "claude:claude-sonnet-4-6"
}
```

**Response**:
```json
{
  "model": "claude:claude-sonnet-4-6",
  "estimated_input_tokens": 1847,
  "estimated_output_tokens": 500,
  "estimated_cost_usd": 0.015,
  "alternatives": [
    {"model": "deepseek:deepseek-chat", "estimated_cost_usd": 0.0004},
    {"model": "openai:gpt-4o-mini", "estimated_cost_usd": 0.002}
  ]
}
```

The `alternatives` field lists all configured models that support the required capabilities (function calling if role has `use_tools`, vision if input has images) sorted by estimated cost ascending. This leverages the full `models.yaml` database deterministically — zero LLM cost.

**Token estimation**: Uses existing `estimate_token_length()` from `src/utils/mod.rs:75-91`. Applies to the assembled prompt (system prompt + schema suffix + input). Output tokens estimated from `max_output_tokens` or a configurable default (500).

**Pipeline estimation**: For roles with `pipeline:` stages, estimate per-stage. The input to stage N+1 is assumed to be roughly the output size of stage N.

### Files to Modify

| File | Change |
|---|---|
| `src/serve.rs` | New endpoint handler |
| `src/utils/mod.rs` | May need to expose `estimate_token_length` as `pub` |

### Effort

Small-medium. ~80-120 lines. The token estimation and price multiplication are straightforward arithmetic.

### Parallelization

Fully independent of all other features.

---

## Feature 8: OpenAPI Specification

### Problem

The server has no machine-readable API description. Programmatic consumers must read source code to discover endpoints. For a tool positioning itself as "infrastructure for AI agents," a machine-readable spec is table stakes.

### Solution

Serve an OpenAPI 3.0 spec at `GET /v1/openapi.json` that documents all endpoints, request/response schemas, and error formats.

### Implementation

**Static spec**: Generate the OpenAPI JSON as a static file embedded in the binary (like `models.yaml`). It documents:
- All `/v1/*` endpoints with request/response schemas
- Authentication (optional bearer token)
- Error format (Phase 4C structured errors)
- Extended fields (`usage.cost_usd`, custom headers)

**Root page**: `GET /` returns a lightweight HTML page listing all endpoints with one-line descriptions and a link to the OpenAPI spec. Replaces the current 404 on root.

### Files to Modify

| File | Change |
|---|---|
| `assets/openapi.json` | New file: OpenAPI 3.0 specification |
| `src/serve.rs` | Serve the spec at `/v1/openapi.json`; root page at `/` |

### Effort

Medium. The OpenAPI spec itself is ~200-300 lines of JSON. The serving code is ~20 lines.

### Parallelization

Fully independent. Should be done last (after other features define the endpoints it documents).

---

## Cross-Feature Dependency Graph

```
Feature 1 (virtual models) ─────────────── Independent
Feature 2 (role invoke) ──── shares role resolution with F1 ──── Soft dep on F1
Feature 3 (role metadata) ───────────────── Independent
Feature 4 (pipeline exec) ── shares stage-event refactor with F2 ── Soft dep on F2
Feature 5 (cost in responses) ───────────── Independent
Feature 6 (server hardening) ────────────── Independent (6A-6E all independent)
Feature 7 (cost estimation) ─────────────── Independent
Feature 8 (OpenAPI spec) ──── depends on all others being defined ── Do last
```

**Maximum parallelism**: 7 independent work streams:
- F1 (virtual models)
- F2 (role invocation — non-streaming)
- F3 (role metadata / public view)
- F5 (cost in responses)
- F6 (server hardening bundle)
- F7 (cost estimation)
- F8 (OpenAPI spec — after others land)

F2-streaming and F4 share a dependency on refactoring `pipe.rs` to emit stage events.

**Recommended implementation order** (if sequential):
1. F6 (hardening) — smallest, unblocks non-localhost usage
2. F3 (metadata) — fixes the prompt leakage, adds single-role retrieval
3. F5 (cost) — trivial arithmetic, high identity alignment
4. F1 (virtual models) — zero-change OpenWebUI integration
5. F2 (role invocation) — the core value endpoint
6. F4 (pipeline execution) — builds on F2's role resolution
7. F7 (cost estimation) — nice-to-have
8. F8 (OpenAPI) — documents everything

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Multi-user auth (OAuth/LDAP) | Platform feature. Delegate to nginx/LiteLLM gateway. Static bearer token (F6B) is sufficient. |
| Conversation persistence / database | Server is stateless by design. Callers manage message history. OpenWebUI owns persistence. |
| Rich web UI / SPA | Violates "no desktop UI" constraint. Playground/arena frozen at current scope. |
| WebSocket streaming | SSE already works. WebSocket adds dependency for no capability gain. |
| Pipeline designer / role editor GUI | Roles are YAML files. Text editor is the authoring tool. |
| Plugin / extension system | Roles + llm-functions + MCP = the extension system. No marketplace needed. |
| Agent hosting (long-running agent sessions) | `call_react` is designed for single-invocation. Scaling to concurrent long-running agents needs fundamentally different architecture. Out of scope. |
| LiteLLM as dependency | Compose at HTTP boundary. Already works via `openai-compatible`. |
| MCP-over-HTTP in `--serve` | The MCP server (`--mcp`) uses stdio. MCPO bridges to HTTP. Keep them separate. |
| Webhook/event push for pipeline stages | JSONL trace (Phase 8G) is the observability mechanism. Callers poll; server doesn't push. |

---

## Relationship to Existing Roadmap

| Epic 5 Feature | Existing Phase | Relationship |
|---|---|---|
| F1 (virtual models) | None | **New** — no existing plan to expose roles through the model listing |
| F2 (role invocation) | None | **New** — no existing plan for role execution endpoint |
| F3 (role metadata) | Phase 1A (`-o json` for `--list-*` and `--info`) | **Extension** — Phase 1A added JSON output to CLI; F3 adds it to the server with a security boundary |
| F4 (pipeline execution) | None | **New** — no existing plan for HTTP pipeline execution |
| F5 (cost in responses) | Phase 8A1 (run log & cost accounting) | **Extension** — Phase 8A1 designs cost wiring for CLI; F5 extends to server responses |
| F6A (CORS) | None | **New** |
| F6B (auth) | None | **New** |
| F6C (health) | None | **New** |
| F6D (streaming usage) | None | **New** — fixes OpenWebUI streaming token tracking |
| F6E (hot-reload) | None | **New** |
| F7 (cost estimation) | None | **New** |
| F8 (OpenAPI) | None | **New** |

---

## Success Metrics

| Metric | Current State | Target |
|---|---|---|
| AIChat features accessible via HTTP | 3 (chat, embed, rerank) | 8+ (add role invoke, pipeline, batch, cost, estimate) |
| OpenWebUI integration | Works but roles invisible | Roles appear as virtual models, cost visible in usage |
| API discoverability | Zero (no spec, no root page) | Full OpenAPI spec at `/v1/openapi.json` |
| Non-localhost deployability | Impossible (CORS + no auth) | Configurable CORS + bearer token |
| Data leakage surface | Full prompt text exposed | Public view with prompt/paths/credentials hidden |
