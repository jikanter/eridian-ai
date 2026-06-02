# Epic 1 - [Done]
## Pre-Roadmap Features

| Feature | Commit | Reference |
|---|---|---|
| Model-aware variables and conditionals | `589b9b1` | [demo](./demos/demo-model-aware.md) |
| Composable roles (`extends`, `include`) | `cdb5d9e` | [demo](./demos/demo-composable-roles.md) |
| Schema-aware stdin/stdout (`input_schema`, `output_schema`) | `b57668d` | [demo](./demos/demo.md) |
| Role parameters (`-v key=value`) and env bridging (`{{$VAR}}`) | `1dbab28` | [analysis](./analysis/2026-03-02-role-parameters.md) |
| Output format flag (`-o json/jsonl/tsv/csv/text`) | `e72d776` | [analysis](./analysis/2026-03-06-output-format.md) |
| `__INPUT__` de-hoisting in extended roles | `9ce9755` | [demo](./demos/demo-dehoist-input.md) |
| Macro system | `30dae5c` | [docs](./macros.md) |
| Semantic exit codes (11 codes, error chain walking) | `c7d4e7e` | `src/utils/exit_code.rs` |

---

## Epic 1: Core Platform

### Phase 0: Prerequisites -- [detail](./phase-0-prerequisites.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 0A | Tool count warning (>20 tools) | Done | `dde1078` |
| 0B | Pipeline tool-calling (`call_react` in `pipe.rs`) | Done | `dde1078` |
| 0C | Pipeline config isolation | Done | `dde1078` |

### Phase 1: Token Efficiency Foundations -- [detail](./phase-1-token-efficiency.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 1A | `-o json` for `--list-*` and `--info` | Done | `dde1078` |
| 1B | Role `description` field | Done | `dde1078` |
| 1C | Deferred tool loading (`tool_search`) | Done | `dde1078` |
| 1D | Tool use examples in role frontmatter | Done | `dde1078` |

### Phase 2: Pipeline & Output Maturity -- [detail](./phase-2-pipeline-output.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 2A | Pipeline-as-Role | Done | `dde1078` |
| 2B | Compact output modifier (`-o compact`) | Done | `dde1078` |

### Phase 3: MCP Consumption -- [detail](./phase-3-mcp-consumption.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 3A | Design document | Done | -- |
| 3B | Discovery (`--mcp-server <CMD> --list-tools`) | Done | `7b31472` |
| 3C | Execution (`--call <TOOL> --json '{...}'`) | Done | `7b31472` |
| 3D | Config-based servers (`mcp_servers:` in config.yaml) | Done | `7b31472` |

### Phase 4: Error Handling & Schema Fidelity -- [detail](./phase-4-error-handling.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 4A | Stop silent data loss | Done | `1e5b7d2` |
| 4B | Structured error types (`AichatError`) | Done | `1e5b7d2` |
| 4C | Structured error output (`-o json`) | Done | `1e5b7d2` |
| 4D | Fix `JsonSchema` lossiness | Done | `fec32e4` |
| 4E | Pipeline stage tracebacks | Done | `fe60f03` |

### Phase 5: Remote MCP & Token-Efficient Discovery -- [detail](./phase-5-remote-mcp.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 5A | Remote MCP servers (HTTP/SSE) | Done | `7f500b8` |
| 5B | Lazy role discovery via MCP | Done | `7f500b8` |

### Phase 6: Metadata Framework Enhancements -- [detail](./phase-6-metadata-framework.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 6A | Shell-injective variables (`{ shell: "git diff --cached" }`) | Done | `30669d7` |
| 6B | Lifecycle hooks (`pipe_to`, `save_to`) | Done | `30669d7` |
| 6C | Unified resource binding (`mcp_servers:` per-role) | Done | `30669d7` |

### Phase 7: Error Messages & Tool Execution -- [detail](./phase-7-error-messages.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 7A | Stderr capture + tool error diagnostics | Done | `d125ee0` |
| 7B | Pre-flight checks + typed error variants | Done | `d125ee0` |
| 7C | Retry budget + loop detection | Done | `d125ee0` |
| 7C1 | Per-tool timeout | Done | `d125ee0` |
| 7D1 | Async tool execution | Done | `d125ee0` |
| 7D2 | Concurrent tool execution | Done | `d125ee0` |

### Phase 7.5: Macro & Agent Config Override

<!-- detail doc (phase-7.5-set-expansion.md) was never committed to the tree; summary below is canonical -->


| Item | Description | Status | Commit |
|---|---|---|---|
| 7.5A | Extend `.set` with role-level fields | Done | `fe60f03` |
| 7.5B | Macro frontmatter assembly | Done | `fe60f03` |
| 7.5C | Agent `.set` parity | Done | `fe60f03` |
| 7.5D | Guard rails (schema meta-validation) | Done | `fe60f03` |

### Phase 8: Data Processing & Observability -- [detail](../phase-8-data-observability.md) *(active — still in `docs/roadmap/`)*

| Item | Description | Status | Commit |
|---|---|---|---|
| 8A1 | Run log & cost accounting | Done | `fe60f03` |
| 8A2 | Pipeline trace metadata | Done | `fe60f03` |
| 8B | Batch record processing (`--each`) | Done | `fe60f03` |
| 8C | Record field templating (`{{.field}}`) | Done | `fe60f03` |
| 8D | Headless RAG | Done | `fe60f03` |
| 8F | Interaction trace (`--trace`) | Done | `fe60f03` |
| 8G | Trace JSONL (`AICHAT_TRACE=1`) | Done | `fe60f03` |
