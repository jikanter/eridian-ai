# Completed Phases — Comprehensive Archive

The authoritative record of every **shipped** aichat phase. The forward-looking roadmap lives in
[`../../ROADMAP.md`](../../ROADMAP.md); this file is where Done phases retire. Each per-phase
design doc moved here on the **2026-06-04 refresh** (see [`../REFRESH-NOTES.md`](../REFRESH-NOTES.md));
the detail links below point to those archived siblings.

> **Not here:** Phase 8 (Data Processing & Observability) is Epic-1-numbered but treated as
> **active** work and stays in the live roadmap dir ([`../phase-8-data-observability.md`](../phase-8-data-observability.md)).
> Phase 32 (Pi as REPL Surface) shipped but is documented as a feature, not a roadmap doc
> ([`../../features/repl-pi.md`](../../features/repl-pi.md)).

---

## Epic 1 — Core Platform — **[Done]**

### Pre-Roadmap Features

| Feature | Commit | Reference |
|---|---|---|
| Model-aware variables and conditionals | `589b9b1` | [demo](../../demos/demo-model-aware.md) |
| Composable roles (`extends`, `include`) | `cdb5d9e` | [demo](../../demos/demo-composable-roles.md) |
| Schema-aware stdin/stdout (`input_schema`, `output_schema`) | `b57668d` | [demo](../../demos/demo-schema-validation.md) |
| Role parameters (`-v key=value`) and env bridging (`{{$VAR}}`) | `1dbab28` | [analysis](../../analysis/2026-03-02-role-parameters.md) |
| Output format flag (`-o json/jsonl/tsv/csv/text`) | `e72d776` | [analysis](../../analysis/2026-03-06-output-format.md) |
| `__INPUT__` de-hoisting in extended roles | `9ce9755` | [demo](../../demos/demo-dehoist-input.md) |
| Macro system | `30dae5c` | [docs](../../features/macros.md) |
| Semantic exit codes (11 codes, error chain walking) | `c7d4e7e` | `src/utils/exit_code.rs` |

### Phase 0: Prerequisites -- [detail](phase-0-prerequisites.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 0A | Tool count warning (>20 tools) | Done | `dde1078` |
| 0B | Pipeline tool-calling (`call_react` in `pipe.rs`) | Done | `dde1078` |
| 0C | Pipeline config isolation | Done | `dde1078` |

### Phase 1: Token Efficiency Foundations -- [detail](phase-1-token-efficiency.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 1A | `-o json` for `--list-*` and `--info` | Done | `dde1078` |
| 1B | Role `description` field | Done | `dde1078` |
| 1C | Deferred tool loading (`tool_search`) | Done | `dde1078` |
| 1D | Tool use examples in role frontmatter | Done | `dde1078` |

### Phase 2: Pipeline & Output Maturity -- [detail](phase-2-pipeline-output.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 2A | Pipeline-as-Role | Done | `dde1078` |
| 2B | Compact output modifier (`-o compact`) | Done | `dde1078` |

### Phase 3: MCP Consumption -- [detail](phase-3-mcp-consumption.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 3A | Design document | Done | -- |
| 3B | Discovery (`--mcp-server <CMD> --list-tools`) | Done | `7b31472` |
| 3C | Execution (`--call <TOOL> --json '{...}'`) | Done | `7b31472` |
| 3D | Config-based servers (`mcp_servers:` in config.yaml) | Done | `7b31472` |

### Phase 4: Error Handling & Schema Fidelity -- [detail](phase-4-error-handling.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 4A | Stop silent data loss | Done | `1e5b7d2` |
| 4B | Structured error types (`AichatError`) | Done | `1e5b7d2` |
| 4C | Structured error output (`-o json`) | Done | `1e5b7d2` |
| 4D | Fix `JsonSchema` lossiness | Done | `fec32e4` |
| 4E | Pipeline stage tracebacks | Done | `fe60f03` |

### Phase 5: Remote MCP & Token-Efficient Discovery -- [detail](phase-5-remote-mcp.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 5A | Remote MCP servers (HTTP/SSE) | Done | `7f500b8` |
| 5B | Lazy role discovery via MCP | Done | `7f500b8` |

### Phase 6: Metadata Framework Enhancements -- [detail](phase-6-metadata-framework.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 6A | Shell-injective variables (`{ shell: "git diff --cached" }`) | Done | `30669d7` |
| 6B | Lifecycle hooks (`pipe_to`, `save_to`) | Done | `30669d7` |
| 6C | Unified resource binding (`mcp_servers:` per-role) | Done | `30669d7` |

### Phase 7: Error Messages & Tool Execution -- [detail](phase-7-error-messages.md)

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

> Phase 8F/8G's ad-hoc trace is **superseded by Phase 42** (SPEC-001 trace emission) in the
> next-year roadmap — see [`../phase-42-overview.md`](../phase-42-overview.md).

---

## Epics 2–14 — Shipped phases

One row per **Done** phase. "Sub-phases" records the granular end state; "Detail" links the
archived per-phase design doc (or the live feature doc for Phase 32).

| Epic | Phase | Scope | Sub-phases (end state) | Detail |
|---|---|---|---|---|
| 2 Runtime Intelligence | 9 | Schema fidelity | 9A–D **Done** | [phase-9-overview.md](phase-9-overview.md) · [detail](phase-9-schema-fidelity.md) |
| 2 Runtime Intelligence | 10 | Resilience & retry | 10A–D **Done**; `model_policy` ruled out | [phase-10-overview.md](phase-10-overview.md) · [detail](phase-10-resilience.md) |
| 2 Runtime Intelligence | 11 | Context budget | 11A/B **Done**, 11C **Superseded**, 11D **Done** | [phase-11-overview.md](phase-11-overview.md) · [detail](phase-11-context-budget.md) |
| 3 Composition UX | 12 | Discoverability | 12A–D **Done** | [phase-12-overview.md](phase-12-overview.md) |
| 3 Composition UX | 13 | Authoring & teaching | 13A–D **Done** | [phase-13-overview.md](phase-13-overview.md) |
| 4 Typed Ports | 14 | Capability manifests | 14A–D **Done** | [phase-14-overview.md](phase-14-overview.md) |
| 4 Typed Ports | 15 | Contract testing | 15A–C **Done** | [phase-15-overview.md](phase-15-overview.md) |
| 4 Typed Ports | 33 | Typed input surface | 33A–E **Done** | [phase-33-overview.md](phase-33-overview.md) |
| 5 Server Engine | 16 | Server hardening | 16A–I **Done** | [phase-16-overview.md](phase-16-overview.md) · [detail](phase-16-server-hardening.md) |
| 5 Server Engine | 17 | Role & pipeline execution | 17A–E **Done** (un-deferred) | [phase-17-overview.md](phase-17-overview.md) · [detail](phase-17-server-execution.md) |
| 6 Universal Addressing | 19 | RoleResolver | 19A–D **Done** | [phase-19-overview.md](phase-19-overview.md) |
| 6 Universal Addressing | 20 | Remote & federated | 20A–D **Done** | [phase-20-overview.md](phase-20-overview.md) |
| 7 DAG Execution | 21 | DAG primitives | 21A–D **Done** | [phase-21-overview.md](phase-21-overview.md) |
| 7 DAG Execution | 22 | DAG observability & budget | 22A–E **Done** | [phase-22-overview.md](phase-22-overview.md) |
| 7 DAG Execution | 36 | Pipeline stage config isolation | 36A–D **Done** | [phase-36-overview.md](phase-36-overview.md) · [plan](phase-36-implementation-plan.md) |
| 8 Feedback Loop | 23 | Role evaluation | 23A–D **Done** | [phase-23-overview.md](phase-23-overview.md) |
| 9 Knowledge Evolution | 25 | Knowledge compilation | **Done** (rewritten 2026-04-16) | [phase-25-knowledge-compilation.md](phase-25-knowledge-compilation.md) |
| 9 Knowledge Evolution | 26 | Knowledge query & composability | **Done** | [phase-26-knowledge-query.md](phase-26-knowledge-query.md) |
| 9 Knowledge Evolution | 27 | Evolution, attribution & trace | **Done** | [phase-27-knowledge-evolution.md](phase-27-knowledge-evolution.md) |
| 11 Bridge Retirement | 31 | MCP pool hardening | 31A–E **Done** | [phase-31-overview.md](phase-31-overview.md) · [detail](phase-31-bridge-retirement.md) |
| 12 Developer Experience | 30 | Macro compilation | 30A–D **Done** | [phase-30-macro-compilation.md](phase-30-macro-compilation.md) |
| 13 Pi as REPL Surface | 32 | Pi cutover | 32A–D **Done** | [repl-pi.md](../../features/repl-pi.md) *(feature doc)* |
| 14 Memory Surface | 34 | Auto-memory wiring | 34A–D **Done** | [phase-34-overview.md](phase-34-overview.md) · [detail](phase-34-auto-memory.md) |

> **Still in flight / planned** (not archived): caching **37–41** (Epic 2), **24**
> (Feedback Loop), **28–29** (Entity Evolution), **35** (Memory Surface), and the next-year
> phases **42–51**. **18** (server discovery) is **deferred**. All live in
> [`../`](../) and the [status ledger](../../ROADMAP.md#status-ledger).
