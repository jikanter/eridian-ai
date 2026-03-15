# AIChat Roadmap

**Last updated:** 2026-03-15
**300 tests passing (127 unit + 173 compatibility), 0 failures**

---

## Vision

AIChat is becoming **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and humans compose multi-model pipelines, consume external tools via MCP, and expose roles as callable infrastructure. The REPL remains a debug/interactive surface, not the primary interface.

### Governing Constraints (from CLAUDE.md)

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No new languages, no desktop UI, no breaking argc/llm-functions** without explicit approval.

---

## Completed Phases

### Phase 0: Prerequisites

| Item | Status | Commit | Notes |
|---|---|---|---|
| 0A. Tool count warning (>20 tools) | Done | `dde1078` | Warns user, logs at debug level |
| 0B. Pipeline tool-calling (`call_react` in `pipe.rs`) | Done | `dde1078` | Stages with `use_tools` route through the agent loop |
| 0C. Pipeline config isolation | Done | `dde1078` | Model save/restore per stage, no shared-state mutation |

### Phase 1: Token Efficiency Foundations

| Item | Status | Commit | Notes |
|---|---|---|---|
| 1A. `-o json` for `--list-*` and `--info` | Done | `dde1078` | Structured metadata for agent consumption |
| 1B. Role `description` field | Done | `dde1078` | Frontmatter field, falls back to first sentence of prompt |
| 1C. Deferred tool loading (`tool_search`) | Done | `dde1078` | Threshold at 15 tools. Compact index, dynamic schema injection |
| 1D. Tool use examples in role frontmatter | Done | `dde1078` | `examples:` field with `input` + `args` pairs |

Phase 1C directly implements the pattern documented in the [tool efficiency analysis](./analysis/2026-03-10-tool-analysis.md): Anthropic's Tool Search reduced initial token cost from 55K to ~500 (85% reduction). aichat's implementation applies the same principle to llm-functions, dropping the `use_tools: all` penalty from ~21K tokens to ~1.3K. See [use_tools: all performance analysis](./analysis/2026-03-10-use-tools-all-performance.md).

### Phase 2: Pipeline & Output Maturity

| Item | Status | Commit | Notes |
|---|---|---|---|
| 2A. Pipeline-as-Role | Done | `dde1078` | Roles with `pipeline:` stages callable as tools |
| 2B. Compact output modifier (`-o compact`) | Done | `dde1078` | Prompt modifier for terse LLM output |

Pipeline-as-Role is aichat's answer to Anthropic's Programmatic Tool Calling pattern. Where Anthropic uses sandboxed Python to orchestrate multiple tools and return only final results, aichat does it declaratively in YAML — an agent sees one tool, internally three models run. See [tool analysis §3](./analysis/2026-03-10-tool-analysis.md#3-anthropic-programmatic-tool-calling-code-as-orchestrator).

### Phase 3: MCP Consumption

| Item | Status | Commit | Notes |
|---|---|---|---|
| 3A. Design document | Done | — | [`docs/roadmap/phase-3-mcp-consumption.md`](./roadmap/phase-3-mcp-consumption.md) |
| 3B. Discovery (`--mcp-server <CMD> --list-tools`) | Done | `7b31472` | Schema caching with 1hr TTL, `--tool-info` |
| 3C. Execution (`--call <TOOL> --json '{...}'`) | Done | `7b31472` | MCP tool dispatch in tool-calling loop |
| 3D. Config-based servers (`mcp_servers:` in config.yaml) | Done | `7b31472` | Connection pooling, env var passthrough, namespaced tools |

This is the first half of the mcp2cli pattern: aichat consumes MCP servers and exposes them as CLI subcommands (~30 tokens/call vs ~121 tokens/schema per turn). See [tool analysis §1](./analysis/2026-03-10-tool-analysis.md#1-mcp2cli-convert-mcp-to-cli-at-runtime).

### Earlier Features (Pre-Roadmap)

| Feature | Commit | Reference |
|---|---|---|
| Model-aware variables and conditionals | `589b9b1` | [`docs/demos/demo-model-aware.md`](./demos/demo-model-aware.md) |
| Composable roles (`extends`, `include`) | `cdb5d9e` | [`docs/demos/demo-composable-roles.md`](./demos/demo-composable-roles.md) |
| Schema-aware stdin/stdout (`input_schema`, `output_schema`) | `b57668d` | [`docs/demos/demo.md`](./demos/demo.md) |
| Role parameters (`-v key=value`) and env bridging (`{{$VAR}}`) | `1dbab28` | [`docs/analysis/2026-03-02-role-parameters.md`](./analysis/2026-03-02-role-parameters.md) |
| Output format flag (`-o json/jsonl/tsv/csv/text`) | `e72d776` | [`docs/analysis/2026-03-06-output-format.md`](./analysis/2026-03-06-output-format.md) |
| `__INPUT__` de-hoisting in extended roles | `9ce9755` | [`docs/demos/demo-dehoist-input.md`](./demos/demo-dehoist-input.md) |
| Macro system | — | [`docs/macros.md`](./macros.md) |
| Semantic exit codes (11 codes, error chain walking) | — | `src/utils/exit_code.rs` |

### Phase 4: Error Handling & Schema Fidelity

| Item | Status | Commit | Notes |
|---|---|---|---|
| 4A. Stop silent data loss | Done | — | 6 `.ok()` swallowing sites in `role.rs` replaced with `warn!()` on parse failure |
| 4B. Structured error types (`AichatError`) | Done | — | `AichatError` enum in `exit_code.rs` with `SchemaValidation`, `ConfigParse`, `ToolNotFound`, `PipelineStage`, `McpError` variants. `classify_error()` fast-path via `downcast_ref`. |
| 4C. Structured error output (`-o json`) | Done | — | Errors emit `{"error": {"code", "category", "message", "context"}}` on stderr when `-o json` active |
| 4D. Fix `JsonSchema` lossiness | Done | — | `FunctionDeclaration.parameters` changed from `JsonSchema` (8 keywords) to `serde_json::Value` (full fidelity). MCP tools now preserve `oneOf`, `allOf`, `$ref`, `additionalProperties`, etc. |
| 4E. Pipeline stage tracebacks | Done | — | Pipeline failures include stage number, total, role name, model ID via `AichatError::PipelineStage` |

Phase 4 enables cheap error recovery for agents consuming aichat. The [tool analysis](./analysis/2026-03-10-tool-analysis.md) argues that aichat should be "the cheapest tool an agent can reach for" — cheap invocation now pairs with cheap error recovery via structured JSON error payloads and semantic exit codes.

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

### Phase 5: Remote MCP & Token-Efficient Discovery

| Item | Status | Commit | Notes |
|---|---|---|---|
| 5A. Remote MCP servers (HTTP/SSE) | Done | — | `endpoint:` + `headers:` fields on `McpServerConfig`. Streamable HTTP transport via `ReqwestClient` adapter (avoids request version conflict). CLI auto-detects `http://`/`https://` URLs. |
| 5B. Lazy role discovery via MCP | Done | — | `discover_roles` meta-tool advertised when ≥8 tools. Full schemas injected on first use with `notifications/tools/list_changed`. Falls back to eager loading for small tool sets. |

**5A implementation notes:**
- `McpServerConfig.endpoint` is mutually exclusive with `command`. When set, uses rmcp's Streamable HTTP client transport.
- `McpServerConfig.headers` supports `${VAR}` env resolution (same as `env`). `Authorization` headers are extracted as bearer tokens.
- `ReqwestClient` wrapper in `src/mcp_client/streamable_http.rs` implements `StreamableHttpClient` for reqwest 0.12, avoiding the version conflict with rmcp's built-in reqwest 0.13 support.
- Connection retry/backoff handled by rmcp's `ExponentialBackoff` default.

**5B implementation notes:**
- Lazy mode activates when tool count ≥ `LAZY_DISCOVERY_THRESHOLD` (8). Below that, all tools served eagerly.
- `ServerCapabilities` advertises `list_changed: true` only in lazy mode.
- On `discover_roles` call: returns compact one-line-per-tool index (~30 tokens/tool vs ~121 tokens/schema).
- On first call to any tool: schema added to advertised set, `tools/list_changed` notification sent. Non-fatal if client ignores it.

**Token budget comparison** (from [tool analysis](./analysis/2026-03-10-tool-analysis.md)):

| Method | Per-turn cost | 20-turn session (30 roles) |
|---|---|---|
| Native MCP (all schemas) | ~3,630 tokens | 72,600 |
| aichat `discover_roles` + on-demand expansion | ~67 + ~121/used tool | ~1,940 (for 5 unique tools) |
| aichat `--list-roles` + on-demand `--describe` | ~67 + ~180/used role | ~1,940 (for 5 unique roles) |

### Phase 6: Metadata Framework Enhancements

| Item | Status | Notes |
|---|---|---|
| 6A. Shell-injective variables | Done | `VariableDefault` union type: `Value(String)` or `Shell { shell }`. Executed via `sh -c` at invocation time. Failures warn instead of crashing (Phase 4A pattern). |
| 6B. Lifecycle hooks | Done | `pipe_to:` pipes output to shell command via stdin. `save_to:` writes to file with `{{timestamp}}` interpolation. Fires in `start_directive` and pipeline last stage. |
| 6C. Unified resource binding | Done | `mcp_servers:` field per-role (list of server names from global config). Auto-expands `use_tools` with `server:*` wildcards. Warns on unknown server names. |

Phase 6A turns roles into self-contained context-gathering units that leverage existing CLI tools (`git`, `grep`, `find`) as context providers. Phase 6B enables zero-friction output routing. Phase 6C means selecting a role configures its entire tool environment. See [Junie metadata plan](./2026-03-10-junie-plan.md).

**YAML examples:**
```yaml
# 6A: Shell-injective variable
variables:
  - name: git_diff
    default: { shell: "git diff --cached" }

# 6B: Lifecycle hooks
pipe_to: "pbcopy"
save_to: "./logs/{{timestamp}}.md"

# 6C: Per-role MCP server binding
mcp_servers:
  - sqlite-server
```

### Phase 7: User-Friendly Error Messages for llm-functions

| Item | Status | Notes |
| --- | --- | --- |
| 7A. Capture stderr from tool processes | Done | `run_command_with_stderr` pipes stderr while inheriting stdout. 64KB cap with `[stderr truncated]` marker. |
| 7A. Include stderr + tool name in errors | Done | `AichatError::ToolExecutionError` carries `tool_name`, `exit_code`, `stderr`, `hint`. |
| 7A. Return tool errors as ToolResult to LLM | Done | `eval_tool_calls` catches errors per-tool, converts to `[TOOL_ERROR]`-prefixed ToolResult. LLM sees failures and can recover. |
| 7A. Replace "DONE" with structured null-result | Done | `json!({"status": "ok", "output": null})`. Removed `is_all_null` clearing that violated tool_use protocol. |
| 7B. Pre-flight checks | Done | Binary existence check (bin_dirs + system PATH), executable permission check (Unix `mode & 0o111`). |
| 7B. Typed error variants | Done | `ToolSpawnError { tool_name, message, hint }` and `ToolExecutionError { tool_name, exit_code, stderr, hint }` in `AichatError`. |
| 7B. Contextual hints on all error paths | Done | `generate_tool_hint()` maps exit codes (126/127) and stderr patterns (not found, permission denied, ECONNREFUSED, rate limit) to actionable suggestions. |
| 7C. Retry budget + loop detection | Done | `call_react` tracks `(tool_name, error_hash)` counts. 2nd identical failure → warning. 3rd → escalation notice. Step budget decays by 2 per repeat (floor: 2). |

Phase 7 transforms tool errors from `"Tool call exit with 1"` into structured, actionable diagnostics that both humans and LLMs can act on. The single highest-leverage change was switching `run_command` from `.status()` (which discards stderr) to `.output()` with `Stdio::piped()` for stderr capture.

**Key architectural changes:**

- **Error recovery, not error propagation.** `eval_tool_calls` no longer bails on first tool failure — each tool gets its own outcome. The LLM sees all results (successes and failures) and can retry, use alternatives, or ask the user for help.
- **Dual-format errors.** Humans see colored stderr warnings. LLMs see `[TOOL_ERROR]` prefixed plain text in tool_result with stderr tail and hint.
- **Typed errors at birth.** `ToolSpawnError` and `ToolExecutionError` carry classification from creation — no fragile string matching needed for new error paths.
- **Protocol-correct null handling.** Every tool call always gets a `ToolResult` response, preventing the LLM from re-issuing calls in a soft loop.

**Before/After:**

```text
# Before
Tool call exit with 1

# After
error: tool 'web_search' failed (exit code 1)
  stderr: curl: (6) Could not resolve host: api.serper.dev
  hint: a network service the tool depends on may be down.
```

**Key files:** `src/utils/command.rs` (`run_command_with_stderr`, `run_command_with_stderr_timeout`), `src/function.rs` (error-as-result, null handling, pre-flight, hints, async eval), `src/utils/exit_code.rs` (`ToolSpawnError`, `ToolExecutionError`, `ToolTimeout`), `src/client/common.rs` (retry budget in `call_react`).

### Phase 7+ : Tool Timeout & Concurrent Execution

| Item | Status | Notes |
| --- | --- | --- |
| 7C1. Per-tool timeout | Done | `run_command_with_stderr_timeout` uses `tokio::process::Command` + `tokio::time::timeout`. Kills child on timeout. `tool_timeout` config field (default 0 = disabled). Per-tool override via `timeout` in functions.json. |
| 7C1. ToolTimeout error variant | Done | `AichatError::ToolTimeout { tool_name, timeout_secs }` with Display, JSON context, and LLM error format. |
| 7D1. Async tool execution | Done | `run_llm_function` and `ToolCall::eval` converted from sync to async. Pipeline roles no longer need `block_in_place`. |
| 7D2. Concurrent tool execution | Done | `eval_tool_calls` runs independent calls via `futures_util::future::join_all`. Each call is independent — errors are per-tool (Phase 7 pattern). |

**Key architectural changes:**

- **No more hangs.** `tool_timeout: 30` in config.yaml (or per-tool `"timeout": 30` in functions.json) kills runaway tools after the deadline. Default is 0 (disabled) for backward compatibility.
- **Concurrent tool execution.** When an LLM requests multiple tool calls, they now run concurrently via `join_all`. This can dramatically reduce latency for multi-tool workflows (e.g., 3 API calls that each take 2s → 2s total instead of 6s).
- **Fully async tool chain.** `eval → run_llm_function → run_command_with_stderr_timeout` is now end-to-end async. Pipeline roles no longer need the `block_in_place` sync→async bridge.

**Config:**

```yaml
# Global timeout (seconds). 0 = disabled.
tool_timeout: 0
```

```json
// Per-tool override in functions.json
{"name": "slow_api", "timeout": 60, ...}
```

---

## Next Phases

### Phase 7.5: Macro & Agent Config Override (`.set` Expansion)

| Item | Status | Notes |
|---|---|---|
| 7.5A. Extend `.set` with role-level fields | — | Add `model`, `output_schema`, `input_schema`, `pipe_to`, `save_to` to `.set` dispatch in `Config::set()`. Schema fields accept inline JSON or `@file` path. |
| 7.5B. Macro frontmatter assembly | — | Macros can now use `.set` to dress up a `.prompt` or override fields on a `.role` before prompting. This turns macros into role factories without collapsing the declarative/imperative boundary. |
| 7.5C. Agent `.set` parity | — | Agents gain the same `.set` overrides through the `RoleLike` trait. `AgentConfig` adds optional `output_schema`, `input_schema`, `pipe_to`, `save_to`. `Agent::to_role()` propagates them via `role.sync()`. |
| 7.5D. Guard rails | — | `.set output_schema` and `.set input_schema` validate the schema itself (meta-validation via `jsonschema::is_valid`) before accepting. `.set pipe_to` validates the target exists. Errors use Phase 4 structured format. |

**Why this phase exists.** Today's entity hierarchy has an awkward gap:

```
                Declarative power    Control flow
  Role          Full frontmatter     None (fixed DAG)
  Prompt        Zero (text only)     None
  Agent         model/temp/tools     None
  Macro         Zero                 Imperative (REPL steps)
```

Roles carry all the metadata but can't branch. Macros can branch but can't shape metadata. Prompts are roles without clothes. Agents implement `RoleLike` but only expose `model`, `temperature`, `top_p`, and `use_tools` — missing the entire schema/lifecycle/pipeline surface that makes roles powerful.

Phase 7.5 closes this by extending `.set` to cover the fields that currently only exist in role frontmatter. This doesn't make macros into roles — it lets macros *configure* roles at runtime.

**`.set` field additions:**

| Field | Value format | Applies to | Notes |
|---|---|---|---|
| `model` | Model ID string | Config global | Already handled by `.model` command; adding to `.set` for consistency |
| `output_schema` | Inline JSON or `@path` | Active role | Parsed and meta-validated before acceptance |
| `input_schema` | Inline JSON or `@path` | Active role | Same as above |
| `pipe_to` | Shell command string | Active role | Validates non-empty; execution handled by existing Phase 6B hooks |
| `save_to` | File path (supports `{{timestamp}}`) | Active role | Same as above |

**Macro example — dynamic schema selection:**
```yaml
# macros/discover.yaml
variables:
  - name: schema
    default: "default"
  - name: query
    rest: true
steps:
  - ".role data-discovery"
  - ".set output_schema @schemas/{{schema}}.json"
  - "{{query}}"
```
```bash
aichat --macro discover ruby ruby programming
# Loads data-discovery role, overrides output_schema from schemas/ruby.json, prompts
```

**Macro example — inline prompt assembly:**
```yaml
# macros/quick-json.yaml
variables:
  - name: query
    rest: true
steps:
  - ".prompt You extract structured data from natural language."
  - ".set output_schema {\"type\": \"object\"}"
  - "{{query}}"
```

**Agent evaluation — is this worthwhile?**

Yes, but with a narrower scope than roles. The case for extending agents:

1. **`output_schema`** — Highest value. Agents have their own tools (`functions.json`) and RAG, but no way to enforce structured output. An agent that can guarantee JSON conformance becomes composable in pipelines. Today an agent's output is always unvalidated text.

2. **`input_schema`** — Medium value. Agents already have interactive variable prompting (`AgentVariable`), which is a runtime form of input validation. `input_schema` adds machine-checkable validation for non-interactive (pipeline/batch) use. Relevant when agents are invoked via `--each` (Phase 8C).

3. **`pipe_to` / `save_to`** — Lower value. Agent sessions already manage output persistence. But for headless agent invocations (`aichat -a agent "prompt"` without session), lifecycle hooks add the same zero-friction routing that roles get. Becomes more valuable with `--each`.

**Implementation path for agents:** Extend `AgentConfig` (not `AgentDefinition`) so overrides are per-invocation, not baked into the agent's definition in llm-functions. The `RoleLike` trait doesn't need new methods — `to_role()` already calls `role.sync(self)`, which copies all role fields. The change is making `AgentConfig` carry the additional fields so `sync` has something to copy.

```rust
// agent.rs — AgentConfig additions
pub struct AgentConfig {
    pub model_id: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub use_tools: Option<String>,
    // Phase 7.5C additions:
    pub output_schema: Option<serde_json::Value>,
    pub input_schema: Option<serde_json::Value>,
    pub pipe_to: Option<String>,
    pub save_to: Option<String>,
    // existing fields...
}
```

**What NOT to give agents:**
- `extends` / `include` — Agent identity is directory-based (`functions.json`, `_instructions`). Role inheritance doesn't map.
- `pipeline` — Agents already have tool-calling loops via `call_react`. Giving them declarative pipelines creates two orchestration models in one entity.
- `mcp_servers` — Agents use tools from `functions.json`. MCP binding is a role concern. If an agent needs MCP tools, wrap it in a role with `mcp_servers`.

**What to kill:**

| Proposal | Reason |
|---|---|
| `.set` for `extends` / `include` | These are compile-time composition — applying them at runtime after the role is resolved would require re-resolution, creating confusing ordering semantics. |
| `.set` for `pipeline` | Pipelines are structural, not parametric. Dynamic pipeline construction belongs in macros via multiple `.role` steps, not in mutating a role's pipeline field. |
| Macro-level frontmatter (macros with their own schema) | Macros are orchestrators, not LLM-facing entities. They don't have prompts, so they don't need schemas. They configure other entities that do. |

**Key files:** `src/config/mod.rs` (`Config::set()` at ~L700), `src/config/agent.rs` (`AgentConfig`, `RoleLike` impl), `src/config/role.rs` (`Role::sync()`), `src/repl/mod.rs` (`.set` REPL dispatch).

### Phase 8: Data Processing & Observability

| Item | Status | Notes |
|---|---|---|
| 8A. Run log & cost accounting | — | JSONL ledger from existing `ModelData.{input,output}_price` × `ChatCompletionsOutput.{input,output}_tokens`. Config: `run_log:` path. CLI: `--cost` displays per-invocation cost on stderr. |
| 8B. Pipeline trace metadata | — | `-o json` wraps pipeline output in envelope with per-stage `{role, model, input_tokens, output_tokens, cost_usd, latency_ms}` and totals. Text mode unaffected. |
| 8C. Batch record processing (`--each`) | — | `--each` reads stdin line-by-line, invokes once per record. `--parallel N` for concurrent execution. Works with `-r` (roles), `-a` (agents), and `--macro` (macros). Per-record errors on stderr, successes on stdout. |
| 8D. Record field templating (`{{.field}}`) | — | Dot-prefixed interpolation extracts JSON fields from the current input record. Available in role prompts, agent instructions, and macro steps. `{{.}}` is the full record. Non-JSON lines: `{{.}}` is the raw line, named fields resolve empty. |
| 8E. Headless RAG | — | Remove `IS_STDOUT_TERMINAL` gate from `Rag::init`. Non-interactive mode falls back to config defaults (`rag_embedding_model`, `rag_chunk_size`, `rag_chunk_overlap`). Unblocks RAG in pipelines and agent automation. |

**8A/8B — Cost wiring.** The infrastructure exists but is disconnected. `ModelData` carries `input_price`/`output_price` (loaded from `models.yaml`). Every API response populates `input_tokens`/`output_tokens` in `ChatCompletionsOutput`. The multiplication never happens — prices only appear in `--list-models`, token counts only in `serve.rs`. Phase 8A connects them into a ledger; 8B extends the `-o json` pipeline envelope with per-stage accounting.

Run log record:
```jsonl
{"ts":"2026-03-14T10:23:01Z","run_id":"a1b2c3","model":"claude:claude-sonnet-4-6","role":"classify","input_tokens":1847,"output_tokens":423,"cost_usd":0.012,"exit_code":0,"latency_ms":2340}
```

Pipeline trace envelope (`-o json`):
```json
{
  "output": "...",
  "trace": {
    "stages": [
      {"role": "extract", "model": "deepseek:deepseek-chat", "input_tokens": 892, "output_tokens": 341, "cost_usd": 0.0003, "latency_ms": 1100},
      {"role": "review", "model": "claude:claude-sonnet-4-6", "input_tokens": 341, "output_tokens": 423, "cost_usd": 0.012, "latency_ms": 2340}
    ],
    "total_cost_usd": 0.0123,
    "total_latency_ms": 3440
  }
}
```

**8C/8D — Record processing.** `--each` is the minimal batch primitive. Everything else — schema validation (`input_schema`/`output_schema`), lifecycle hooks (`pipe_to`/`save_to`), output formatting (`-o jsonl`) — already works per-invocation. `--each` adds only the iteration loop. `{{.field}}` adds only field extraction. Together they compose with the full feature set of whichever entity type is invoked.

**8C/8D work uniformly across all entity types because they are input-level features, resolved before entity dispatch:**

| Entity | `--each` | `{{.field}}` in... | Mechanism |
|---|---|---|---|
| Role (`-r`) | Yes | Prompt template | Fields interpolated alongside `{{var}}` and `{{$VAR}}` |
| Agent (`-a`) | Yes | `instructions` template | Fields interpolated via same path as `{{__tools__}}` |
| Macro (`--macro`) | Yes | Step interpolation | Fields available alongside positional `{{var}}` in each step |
| Prompt (bare) | Yes | Prompt text | Fields interpolated before sending |

**Template interpolation namespaces (cumulative with existing):**
```
{{var}}       Role/agent declared variable (-v key=value, --agent-variable)
{{$VAR}}      Environment variable
{{.field}}    Record field from current --each input line (Phase 8D)
{{.}}         Full record (entire input line)
{{timestamp}} Built-in (lifecycle hooks only)
```

**Example — JSONL dataset with a role:**
```bash
# classify.md has output_schema enforcing {"label": "string", "confidence": "number"}
cat emails.jsonl | aichat -r classify -o jsonl --each --parallel 4
```
Role prompt uses `{{.subject}}` and `{{.body}}`. `output_schema` validates each response. Output: one JSONL line per input.

**Example — JSONL dataset with an agent:**
```bash
cat tickets.jsonl | aichat -a triage-agent --each --parallel 2
```
Agent `instructions` uses `{{.title}}` and `{{.description}}`. Agent tools and RAG available per-invocation (8E required for RAG).

**Example — JSONL dataset with a macro:**
```yaml
# macros/enrich.yaml
variables:
  - name: model
    default: "openai:gpt-4o-mini"
steps:
  - ".role enricher -m {{model}}"
  - "Enrich: {{.}}"
```
```bash
cat records.jsonl | aichat --macro enrich --each
```

**8E — Headless RAG.** `Rag::init` currently calls `bail!("Failed to init rag in non-interactive mode")` when `!IS_STDOUT_TERMINAL`. This blocks any pipeline or automation use of agent RAG. The config defaults (`rag_embedding_model`, `rag_chunk_size`, `rag_chunk_overlap`) already exist — the fix is to use them instead of prompting interactively. Prerequisite for 8C to work with RAG-enabled agents.

**What to kill:**
| Proposal | Reason |
|---|---|
| `--resume` / checkpoint in `--each` | Unix composition: `tail -n +N` the input and re-run. Stateless batch is simpler. |
| Windowing / aggregation / streaming | `--each` processes one line at a time. Aggregation belongs downstream (`jq`, `duckdb`). |
| Per-record retry logic | Failed records emit structured errors (Phase 4C) on stderr. Filter and re-process. |
| Cost dashboard / visualization | JSONL run log is the interface. Pipe to `jq`, `duckdb`, Grafana. |
| `{{.field.nested}}` deep access | Premature. If needed, the role prompt can instruct the model to extract nested fields. |

---

## Architecture Notes

### Key Files

| File | Lines | Purpose |
|---|---|---|
| `src/config/mod.rs` | ~2,990 | Core config: loading, validation, model/role/session/agent management |
| `src/config/role.rs` | ~1,210 | Role parsing, frontmatter, extends/include, schema validation |
| `src/config/session.rs` | ~640 | Chat session persistence and compression |
| `src/config/agent.rs` | ~560 | Agent definition, config, variable initialization |
| `src/config/input.rs` | ~600 | Input processing, media handling |
| `src/function.rs` | ~742 | Tool declarations, tool_search, async tool dispatch, concurrent execution, error recovery |
| `src/mcp_client/mod.rs` | ~530 | MCP client: connection pool, tool conversion, CLI commands, caching |
| `src/mcp_client/streamable_http.rs` | ~250 | HTTP/SSE transport adapter (Phase 5A) |
| `src/mcp.rs` | ~270 | MCP server mode with lazy discovery (Phase 5B) |
| `src/pipe.rs` | ~220 | Pipeline execution with tool-calling and config isolation |
| `src/utils/exit_code.rs` | ~710 | Semantic exit codes, error chain classification, typed tool errors, ToolTimeout |
| `src/client/common.rs` | — | `call_react` agent loop, model data, provider abstraction |

### Configuration Hierarchy

```
Global Config (config.yaml)
  └─ Agent Config (agents/{name}/config.yaml)
       └─ Role Config (role YAML frontmatter)
            └─ Session Config (sessions/{name}.yaml)
```

Fallback order: Session > Agent > Role > Global defaults.

### Entity Types

aichat has four entity types. Three form a capability hierarchy (**Prompt < Role < Agent**); the fourth (**Macro**) is orthogonal.

**Prompt** — Anonymous, ephemeral. Raw text used as a system prompt. Under the hood, creates a temporary Role named `%%` (`TEMP_ROLE_NAME`). No file, no metadata, no persistence. Invoked via `aichat "text"` or `aichat --prompt "text"`.

**Role** — The core configuration unit. A markdown file (`<config>/roles/name.md`) with YAML frontmatter. Carries all metadata: model, temperature, top_p, use_tools, input_schema, output_schema, variables (including shell-injective defaults), pipe_to, save_to, mcp_servers, extends/include, pipeline, examples, description. Roles are the only entity supporting schema validation, pipelines, lifecycle hooks, inheritance, and MCP binding. Invoked via `aichat -r name`.

**Agent** — Directory-based (`<functions_dir>/agents/name/`). Implements `RoleLike` trait — wraps a Role via `to_role()`. Adds: own tool functions (`functions.json`), RAG (documents), dynamic instructions (`_instructions` shell function), interactive variable prompting, session management, env-var bridging (`LLM_AGENT_VAR_*`). Defined in llm-functions, not in aichat's config directory. Does NOT support: input_schema, output_schema, pipe_to, save_to, mcp_servers, extends/include, pipeline. Phase 7.5C proposes adding `output_schema`, `input_schema`, `pipe_to`, `save_to` to `AgentConfig` (per-invocation overrides, not baked into definition). Invoked via `aichat -a name`.

**Macro** — A YAML file (`<config>/macros/name.yaml`) with positional variables and a list of REPL command steps. Runs in an isolated config clone (session/agent/rag/role cleared). Can reference roles and agents in its steps but is not itself a role. Cannot perform `.edit` operations. Invoked via `aichat --macro name` or REPL `.macro name`.

**Feature matrix (verified against source):**

| Capability | Prompt | Role | Agent | Macro |
|---|---|---|---|---|
| System prompt | Raw text | Frontmatter + body | `instructions` + dynamic | N/A (REPL commands) |
| Model pinning | No | `model:` field | `model_id:` field | No (inherits current) |
| `input_schema` validation | No | **Yes** (`jsonschema` before LLM) | No (Phase 7.5C) | No |
| `output_schema` validation | No | **Yes** (`jsonschema` after LLM) | No (Phase 7.5C) | No |
| Variables (plain) | No | **Yes** (`-v key=value`) | **Yes** (`--agent-variable`) | **Yes** (positional args) |
| Variables (shell-injective) | No | **Yes** (`{shell: "cmd"}`) | No | No |
| `pipe_to` / `save_to` | No | **Yes** | No (Phase 7.5C) | No |
| `mcp_servers` binding | No | **Yes** (auto-expands `use_tools`) | No | No |
| `extends` / `include` | No | **Yes** | No | No |
| `pipeline` stages | No | **Yes** (multi-model chaining) | No | No |
| Own tool functions | No | Via `use_tools` | **Yes** (`functions.json`) | No |
| RAG (documents) | No | No | **Yes** (built-in) | No |
| Dynamic instructions | No | No | **Yes** (`_instructions`) | No |
| Session management | No | No | **Yes** (session vars, lifecycle) | No (clears session) |
| Callable as tool | No | **Yes** (if has `pipeline:`) | No | No |
| `-o` output format | Yes | Yes | Yes | N/A |
| `--each` batch (Phase 8C) | Yes | Yes | Yes | Yes |
| `{{.field}}` templating (Phase 8D) | Yes | Yes | Yes | Yes |

### Tool Dispatch

```text
select_functions()
  ├─ < 15 tools → eager load all schemas
  └─ ≥ 15 tools → inject tool_search meta-function
                     └─ on search → inject selected schemas

eval_tool_calls()  [Phase 7+: concurrent via join_all, errors per-tool as ToolResult]
  ├─ tool name contains ':' + MCP pool exists → eval_mcp_tool()
  ├─ tool has pipeline stages → eval_pipeline_role() (async)
  ├─ tool is tool_search → eval_tool_search()
  └─ default → preflight_check() → extract call config → run_llm_function() (async)
       └─ run_command_with_stderr_timeout() → async, timeout, stderr captured
```

### Error Flow (Phases 4 + 7)

```text
AichatError { category, message, context }
  │
  ├─ classify_error() fast path: downcast_ref → ExitCode
  │
  ├─ render_error()
  │    ├─ -o json → {"error": {"code": 8, "category": "schema_validation", ...}}
  │    ├─ -o text → human-readable stderr with field/location
  │    └─ pipeline → stage traceback (stage 2/4, role: review, model: claude)
  │
  └─ Tool error recovery (Phase 7)
       ├─ ToolExecutionError → caught in eval_tool_calls → ToolResult with [TOOL_ERROR]
       ├─ ToolSpawnError → caught in eval_tool_calls → ToolResult with [TOOL_ERROR]
       ├─ LLM sees error + stderr + hint → can retry or use alternatives
       └─ call_react retry budget → 2 identical failures → warning → 3rd → escalation

Legacy anyhow::Error paths still work via string-matching fallback in classify_error().
```

### What to Preserve

- **exec + stdout + exit code** as the tool contract — Unix IPC, not protocol
- **argc comment-driven schemas** — authoring format stays
- **`bin/` wrappers for CLI use** — `aichat -r summarize < input.txt` keeps working
- **Language-agnostic tools** — bash/js/py support via llm-functions
- **MCP as outward-facing facade only** — tools never speak MCP internally
- **`-o` as the single output control axis** — no `--machine-readable`, no `--agent-mode`

### What Was Killed

| Proposal | Reason |
|---|---|
| TOON output format | No model trained on it. Malformed output across providers. |
| `-r <role> --describe` | Duplicates `--info`. Use `--info -o json` instead. |
| `--mcp` flag overloading | Serve vs consume are opposite semantics. Separate flags. |
| MCP `discover_roles` in Phase 1 | Protocol dependency, model capability requirements. Deferred to Phase 5B. |

---

## Reference

| Document | Location |
|---|---|
| Initial phased roadmap | [`docs/roadmap/initial-phased-roadmap.md`](./roadmap/initial-phased-roadmap.md) |
| Phase 3 MCP design doc | [`docs/roadmap/phase-3-mcp-consumption.md`](./roadmap/phase-3-mcp-consumption.md) |
| Tool efficiency analysis | [`docs/analysis/2026-03-10-tool-analysis.md`](./analysis/2026-03-10-tool-analysis.md) |
| `use_tools: all` performance | [`docs/analysis/2026-03-10-use-tools-all-performance.md`](./analysis/2026-03-10-use-tools-all-performance.md) |
| Output format analysis | [`docs/analysis/2026-03-06-output-format.md`](./analysis/2026-03-06-output-format.md) |
| Strategic landscape analysis | [`docs/analysis/2026-03-02-analysis.md`](./analysis/2026-03-02-analysis.md) |
| Meta-analysis critique | [`docs/analysis/2026-03-02-meta-analysis.md`](./analysis/2026-03-02-meta-analysis.md) |
| Junie metadata plan | [`docs/analysis/2026-03-10-junie-plan.md`](./analysis/2026-03-10-junie-plan.md) |
| Error messages analysis | [`docs/analysis/2026-03-13-user-friendly-error-messages.mdx`](./analysis/2026-03-13-user-friendly-error-messages.mdx) |
| Macro documentation | [`docs/macros.md`](./macros.md) |
| Role parameters | [`docs/analysis/2026-03-02-role-parameters.md`](./analysis/2026-03-02-role-parameters.md) |
