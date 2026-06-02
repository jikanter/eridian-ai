# Phase 7: Error Messages, Tooling & Config

**Status:** Done

---

## Phase 7: User-Friendly Error Messages for llm-functions

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

---

## Phase 7+: Tool Timeout & Concurrent Execution

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

## Phase 7.5: Macro & Agent Config Override (`.set` Expansion)

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

2. **`input_schema`** — Medium value. Agents already have interactive variable prompting (`AgentVariable`), which is a runtime form of input validation. `input_schema` adds machine-checkable validation for non-interactive (pipeline/batch) use. Relevant when agents are invoked via `--each` (Phase 8B).

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
