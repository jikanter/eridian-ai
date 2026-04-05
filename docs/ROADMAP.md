# AIChat Roadmap

**Last updated:** 2026-03-30
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
| Schema-aware stdin/stdout (`input_schema`, `output_schema`) | `b57668d` | [demo](demos/demo-schema-validation.md) |
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

| Item | Description | Status | Commit |
|---|---|---|---|
| 6A | Shell-injective variables | Done | -- |
| 6B | Lifecycle hooks (`pipe_to`, `save_to`) | Done | -- |
| 6C | Unified resource binding (`mcp_servers:`) | Done | -- |

### Phase 7: Error Messages, Tooling & Config -- [detail](./roadmap/phase-7-error-messages.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 7A | Stderr capture + tool error diagnostics | Done | -- |
| 7A | Tool errors as ToolResult to LLM | Done | -- |
| 7A | Structured null-result (replace "DONE") | Done | -- |
| 7B | Pre-flight checks + typed error variants | Done | -- |
| 7B | Contextual hints on all error paths | Done | -- |
| 7C | Retry budget + loop detection | Done | -- |
| 7C1 | Per-tool timeout | Done | -- |
| 7D1 | Async tool execution | Done | -- |
| 7D2 | Concurrent tool execution | Done | -- |
| 7.5A | Extend `.set` with role-level fields | Done | -- |
| 7.5B | Macro frontmatter assembly | Done | -- |
| 7.5C | Agent `.set` parity | Done | -- |
| 7.5D | Guard rails (schema meta-validation) | Done | -- |

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
