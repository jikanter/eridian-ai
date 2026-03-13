# AIChat Roadmap

**Last updated:** 2026-03-13
**112 unit tests passing, 0 failures**

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
| 5A. Remote MCP servers (HTTP/SSE) | Done | — | `endpoint:` + `headers:` fields on `McpServerConfig`. Streamable HTTP transport via `ReqwestClient` adapter (avoids reqwest version conflict). CLI auto-detects `http://`/`https://` URLs. |
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
| `src/function.rs` | ~490 | Tool declarations, tool_search, tool dispatch |
| `src/mcp_client/mod.rs` | ~530 | MCP client: connection pool, tool conversion, CLI commands, caching |
| `src/mcp_client/streamable_http.rs` | ~250 | HTTP/SSE transport adapter (Phase 5A) |
| `src/mcp.rs` | ~270 | MCP server mode with lazy discovery (Phase 5B) |
| `src/pipe.rs` | ~220 | Pipeline execution with tool-calling and config isolation |
| `src/utils/exit_code.rs` | ~285 | Semantic exit codes, error chain classification |
| `src/client/common.rs` | — | `call_react` agent loop, model data, provider abstraction |

### Configuration Hierarchy

```
Global Config (config.yaml)
  └─ Agent Config (agents/{name}/config.yaml)
       └─ Role Config (role YAML frontmatter)
            └─ Session Config (sessions/{name}.yaml)
```

Fallback order: Session > Agent > Role > Global defaults.

### Tool Dispatch

```
select_functions()
  ├─ < 15 tools → eager load all schemas
  └─ ≥ 15 tools → inject tool_search meta-function
                     └─ on search → inject selected schemas

eval_tool_calls()
  ├─ tool name contains ':' + MCP pool exists → eval_mcp_tool()
  ├─ tool has pipeline stages → eval_pipeline_role()
  ├─ tool is tool_search → eval_tool_search()
  └─ default → extract call config → run_llm_function()
```

### Error Flow (Phase 4)

```
AichatError { category, message, context }
  │
  ├─ classify_error() fast path: downcast_ref → ExitCode
  │
  └─ render_error()
       ├─ -o json → {"error": {"code": 8, "category": "schema_validation", ...}}
       ├─ -o text → human-readable stderr with field/location
       └─ pipeline → stage traceback (stage 2/4, role: review, model: claude)

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
| Macro documentation | [`docs/macros.md`](./macros.md) |
| Role parameters | [`docs/analysis/2026-03-02-role-parameters.md`](./analysis/2026-03-02-role-parameters.md) |
