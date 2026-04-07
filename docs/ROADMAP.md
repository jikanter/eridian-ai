# AIChat Roadmap

**Last updated:** 2026-04-07
**317 tests passing (144 unit + 173 compatibility), 0 failures**

---

## Vision

AIChat is becoming **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and humans compose multi-model pipelines, consume external tools via MCP, and expose roles as callable infrastructure. The REPL remains a debug/interactive surface, not the primary interface.

### Governing Constraints

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No new languages, no desktop UI, no breaking argc/llm-functions** without explicit approval.

---

## Epic Overview

| Epic | Scope | Phases | Status | Design |
|---|---|---|---|---|
| 1 | Core Platform | 0-8 | **Done** | -- |
| 2 | Runtime Intelligence | 9-11 | Planned | [epic-2.md](./analysis/epic-2.md) |
| 3 | Server Pipeline Engine | 12-14 | Planned | [epic-3.md](./analysis/epic-3.md) |
| 4 | RAG Evolution | 15-17 | Planned | [epic-4.md](./analysis/epic-4.md) |
| 5 | Entity Evolution | 18-19 | Planned | [epic-5.md](./analysis/epic-5.md) |

Architecture reference: [architecture.md](./roadmap/architecture.md)

---

## Pre-Roadmap Features

| Feature | Commit | Reference |
|---|---|---|
| Model-aware variables and conditionals | `589b9b1` | [demo](./demos/demo-model-aware.md) |
| Composable roles (`extends`, `include`) | `cdb5d9e` | [demo](./demos/demo-composable-roles.md) |
| Schema-aware stdin/stdout (`input_schema`, `output_schema`) | `b57668d` | [demo](./demos/demo.md) |
| Role parameters (`-v key=value`) and env bridging (`{{$VAR}}`) | `1dbab28` | [analysis](./analysis/2026-03-02-role-parameters.md) |
| Output format flag (`-o json/jsonl/tsv/csv/text`) | `e72d776` | [analysis](./analysis/2026-03-06-output-format.md) |
| `__INPUT__` de-hoisting in extended roles | `9ce9755` | [demo](./demos/demo-dehoist-input.md) |
| Macro system | -- | [docs](./macros.md) |
| Semantic exit codes (11 codes, error chain walking) | -- | `src/utils/exit_code.rs` |

---

## Epic 1: Core Platform

### Phase 0: Prerequisites -- [detail](./roadmap/phase-0-prerequisites.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 0A | Tool count warning (>20 tools) | Done | `dde1078` |
| 0B | Pipeline tool-calling (`call_react` in `pipe.rs`) | Done | `dde1078` |
| 0C | Pipeline config isolation | Done | `dde1078` |

### Phase 1: Token Efficiency Foundations -- [detail](./roadmap/phase-1-token-efficiency.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 1A | `-o json` for `--list-*` and `--info` | Done | `dde1078` |
| 1B | Role `description` field | Done | `dde1078` |
| 1C | Deferred tool loading (`tool_search`) | Done | `dde1078` |
| 1D | Tool use examples in role frontmatter | Done | `dde1078` |

### Phase 2: Pipeline & Output Maturity -- [detail](./roadmap/phase-2-pipeline-output.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 2A | Pipeline-as-Role | Done | `dde1078` |
| 2B | Compact output modifier (`-o compact`) | Done | `dde1078` |

### Phase 3: MCP Consumption -- [detail](./roadmap/phase-3-mcp-consumption.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 3A | Design document | Done | -- |
| 3B | Discovery (`--mcp-server <CMD> --list-tools`) | Done | `7b31472` |
| 3C | Execution (`--call <TOOL> --json '{...}'`) | Done | `7b31472` |
| 3D | Config-based servers (`mcp_servers:` in config.yaml) | Done | `7b31472` |

### Phase 4: Error Handling & Schema Fidelity -- [detail](./roadmap/phase-4-error-handling.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 4A | Stop silent data loss | Done | -- |
| 4B | Structured error types (`AichatError`) | Done | -- |
| 4C | Structured error output (`-o json`) | Done | -- |
| 4D | Fix `JsonSchema` lossiness | Done | -- |
| 4E | Pipeline stage tracebacks | Done | -- |

### Phase 5: Remote MCP & Token-Efficient Discovery -- [detail](./roadmap/phase-5-remote-mcp.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 5A | Remote MCP servers (HTTP/SSE) | Done | -- |
| 5B | Lazy role discovery via MCP | Done | -- |

### Phase 6: Metadata Framework Enhancements -- [detail](./roadmap/phase-6-metadata-framework.md)

| Item | Description | Status |
|---|---|---|
| 6A | Shell-injective variables (`{ shell: "..." }` in variable defaults) | Done |
| 6B | Lifecycle hooks (`pipe_to:`, `save_to:`) | Done |
| 6C | Unified resource binding (`rag:`, `mcp_servers:` per-role) | Done |

Phase 6A is the most immediately useful — it turns roles into self-contained context-gathering units that leverage existing CLI tools (`git`, `grep`, `find`) as context providers.

**Error handling dependency:** Phase 6A requires Phase 4A (done) — role parsing now warns on malformed entries instead of silently dropping them. Shell command failures in variable defaults must follow the same pattern.

#### YAML Examples of 6A, 6B, and 6C

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

### Phase 7: User-Friendly Error Messages for llm-functions -- [detail](./roadmap/phase-7-error-messages.md)

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

#### Phase 7 continued: Tool Timeout & Concurrent Execution

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

### Phase 7.5: Macro & Agent Config Override (`.set` Expansion)

| Item | Status | Notes |
|---|---|---|
| 7.5A. Extend `.set` with role-level fields | Done | Add `model`, `output_schema`, `input_schema`, `pipe_to`, `save_to` to `.set` dispatch in `Config::set()`. Schema fields accept inline JSON or `@file` path. |
| 7.5B. Macro frontmatter assembly | Done | Macros can now use `.set` to dress up a `.prompt` or override fields on a `.role` before prompting. This turns macros into role factories without collapsing the declarative/imperative boundary. |
| 7.5C. Agent `.set` parity | Done | Agents gain the same `.set` overrides through the `RoleLike` trait. `AgentConfig` adds optional `output_schema`, `input_schema`, `pipe_to`, `save_to`. `Agent::to_role()` propagates them via `role.sync()`. |
| 7.5D. Guard rails | Done | `.set output_schema` and `.set input_schema` validate the schema itself (meta-validation via `jsonschema::is_valid`) before accepting. `.set pipe_to` validates the target exists. Errors use Phase 4 structured format. |

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
| `model` | Model ID string | `.prompt`, `.role`, agents | Overrides model for current session |
| `output_schema` | Inline JSON or `@file` | `.prompt`, `.role`, agents | Meta-validated via `jsonschema::is_valid` |
| `input_schema` | Inline JSON or `@file` | `.prompt`, `.role`, agents | Meta-validated via `jsonschema::is_valid` |
| `pipe_to` | Shell command | `.prompt`, `.role`, agents | Target existence validated |
| `save_to` | File path template | `.prompt`, `.role`, agents | Supports `{{timestamp}}` interpolation |

### Phase 8: Data Processing & Observability -- [detail](./roadmap/phase-8-data-observability.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 8A1 | Run log & cost accounting | Done | -- |
| 8A2 | Pipeline trace metadata | Done | -- |
| 8B | Batch record processing (`--each`) | Done | -- |
| 8C | Record field templating (`{{.field}}`) | Done | -- |
| 8D | Headless RAG | Done | -- |
| 8F | Interaction trace (`--trace`) | Done | -- |
| 8G | Trace JSONL (`AICHAT_TRACE=1`) | Done | -- |

---

## Epic 2: Runtime Intelligence -- [design](./analysis/epic-2.md)

### Phase 9: Schema Fidelity -- [detail](./roadmap/phase-9-schema-fidelity.md)

| Item | Description | Status |
|---|---|---|
| 9A | Provider-native structured output (OpenAI `response_format`) | -- |
| 9B | Provider-native structured output (Claude tool-use-as-schema) | -- |
| 9C | Schema validation retry loop | -- |
| 9D | Capability-aware pre-flight validation | -- |

### Phase 10: Resilience -- [detail](./roadmap/phase-10-resilience.md)

| Item | Description | Status |
|---|---|---|
| 10A | API-level retry with exponential backoff | -- |
| 10B | Pipeline stage output cache | -- |
| 10C | Pipeline stage retry | -- |
| 10D | Pipeline model fallback | -- |

### Phase 11: Context Budget -- [detail](./roadmap/phase-11-context-budget.md)

| Item | Description | Status |
|---|---|---|
| 11A | Context budget allocator core | -- |
| 11B | BM25-ranked file inclusion | -- |
| 11C | Budget-aware RAG | -- |

---

## Epic 3: Server Pipeline Engine -- [design](./analysis/epic-3.md)

### Phase 12: Hardening & Knowledge Exposure -- [detail](./roadmap/phase-12-server-hardening.md)

| Item | Description | Status |
|---|---|---|
| 12A | Configurable CORS origins | -- |
| 12B | Optional bearer token auth | -- |
| 12C | Health endpoint | -- |
| 12D | Streaming usage in final SSE chunk | -- |
| 12E | Hot-reload endpoint | -- |
| 12F | Role metadata security (`RolePublicView`) | -- |
| 12G | Single-role retrieval | -- |
| 12H | Cost in API responses | -- |

### Phase 13: Role & Pipeline Execution -- [detail](./roadmap/phase-13-server-execution.md)

| Item | Description | Status |
|---|---|---|
| 13A | Roles as virtual models | -- |
| 13B | Role invocation endpoint (non-streaming) | -- |
| 13C | Role invocation endpoint (streaming) | -- |
| 13D | Pipeline execution endpoint | -- |
| 13E | Batch processing endpoint | -- |

### Phase 14: Discovery & Estimation -- [detail](./roadmap/phase-14-server-discovery.md)

| Item | Description | Status |
|---|---|---|
| 14A | Cost estimation endpoint | -- |
| 14B | OpenAPI specification | -- |
| 14C | Root page | -- |

---

## Epic 4: RAG Evolution -- [design](./analysis/epic-4.md)

### Phase 15: Structured Retrieval -- [detail](./roadmap/phase-15-rag-structured.md)

| Item | Description | Status |
|---|---|---|
| 15A | Sibling chunk expansion | -- |
| 15B | Metadata-enriched chunks | -- |
| 15C | Incremental HNSW insertion | -- |
| 15D | Binary vector storage | -- |

### Phase 16: Composability -- [detail](./roadmap/phase-16-rag-composability.md)

| Item | Description | Status |
|---|---|---|
| 16A | Role `rag:` field | -- |
| 16B | Pipeline RAG integration | -- |
| 16C | CLI RAG mode | -- |
| 16D | Search-only mode | -- |
| 16E | Multi-RAG search | -- |
| 16F | RAG as LLM tool | -- |

### Phase 17: Graph Expansion & Observability -- [detail](./roadmap/phase-17-rag-graph.md)

| Item | Description | Status |
|---|---|---|
| 17A | Chunk-adjacency graph | -- |
| 17B | RAG trace integration | -- |

---

## Epic 5: Entity Evolution -- [design](./analysis/epic-5.md)

### Phase 18: Agent Composability -- [detail](./roadmap/phase-18-agent-composability.md)

| Item | Description | Status |
|---|---|---|
| 18A | Agent-as-tool | -- |
| 18B | Unified entity resolution | -- |
| 18C | Configurable react loop | -- |
| 18D | Agent-in-pipeline | -- |
| 18E | Agent MCP binding | -- |

### Phase 19: Agent Dynamism -- [detail](./roadmap/phase-19-agent-dynamism.md)

| Item | Description | Status |
|---|---|---|
| 19A | ReactPolicy trait | -- |
| 19B | Agent memory (JSONL) | -- |
| 19C | Macro output chaining | -- |
