# Architecture Notes

---

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
| `src/rag/mod.rs` | ~1,030 | RAG: hybrid HNSW+BM25 search, RRF, embedding, sync (Phases 15-17) |
| `src/serve.rs` | ~960 | HTTP server: OpenAI-compatible API, playground, arena (Phases 12-14) |
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
| `--each` batch (Phase 8B) | Yes | Yes | Yes | Yes |
| `{{.field}}` templating (Phase 8C) | Yes | Yes | Yes | Yes |

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
