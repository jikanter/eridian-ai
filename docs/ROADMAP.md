# AIChat Roadmap

**Last updated:** 2026-05-11
**697 tests passing (487 unit + 197 compatibility + 13 federation), 0 failures**

---

## Vision

AIChat is becoming **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and humans compose multi-model pipelines, consume external tools via MCP, and expose roles as callable infrastructure. The REPL remains a debug/interactive surface, not the primary interface.

Roles are the fundamental unit of composition. This roadmap evolves roles from static prompt templates into **typed, addressable, evaluable building blocks** that compose across machines, execution models, and cost budgets.

### Governing Constraints

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No new languages, no desktop UI, no breaking argc/llm-functions** without explicit approval.

---

## Epic Overview

| Epic | Scope | Phases | Status | Design |
|---|---|---|---|---|
| 1 | Core Platform | 0-8 | **Done** | -- |
| 2 | Runtime Intelligence | 9-11 | **Done** | [epic-2.md](./analysis/epic-2.md) |
| 3 | Composition UX | 12-13 | Phase 12 **Done**; Phase 13 planned | [epic-3.md](./analysis/epic-3.md) |
| 4 | Typed Ports & Capabilities | 14-15 | Phase 14 **Done**; Phase 15 planned | [epic-4.md](./analysis/epic-4.md) |
| 5 | Server Pipeline Engine | 16-18 | Phase 16 (F/G/H) + Phase 17 **Done 2026-05-11**; Phase 18 deferred | [epic-5.md](./analysis/epic-5.md) |
| 6 | Universal Addressing | 19-20 | **Done 2026-05-11** | [epic-6.md](./analysis/epic-6.md) |
| 7 | DAG Execution | 21-22 | Planned | [epic-7.md](./analysis/epic-7.md) |
| 8 | Feedback Loop | 23-24 | Planned | [epic-8.md](./analysis/epic-8.md) |
| 9 | Knowledge Evolution | 25-27 | **Done** | [epic-9.md](./analysis/epic-9.md) |
| 10 | Entity Evolution | 28-29 | Planned | [epic-10.md](./analysis/epic-10.md) |
| 11 | Bridge Retirement & MCP Pool Hardening | 31 | **Done** | [bridge-retirement.md](./architecture/integrated-architecture/bridge-retirement.md) |
| 12 | Developer Experience & Performance | 30 | **Done** | -- |

Architecture reference: [architecture.md](architecture/architecture.md)
Completed Epics: [completed-epics.md](archive/roadmap/completed-epics.md)

---

## Epic 2: Runtime Intelligence -- [design](./analysis/epic-2.md)

> Every token sent to an LLM should be a token that only an LLM can process. If deterministic logic can resolve a question, it should never reach the model.

### Phase 9: Schema Fidelity

[Phase 9 Overview](./roadmap/phase-9-overview.md)

### Phase 10: Resilience & Cost-Aware Routing

[Phase 10 Overview](./roadmap/phase-10-overview.md)

### Phase 11: Context Budget & Budget Propagation

[Phase 11 Overview](./roadmap/phase-11-overview.md)

---

## Epic 3: Composition UX (NEW)

> Apply token-consciousness to human attention, not just LLM calls. Every role invocation should make the user slightly more aware of what the system can do.

*Source: Theme 6 — UX Designer analysis. Focuses on reducing the cost of understanding before the cost of execution.*

### Phase 12: Discoverability & Previews

[Phase 12 Overview](./roadmap/phase-12-overview.md)

### Phase 13: Authoring & Teaching

[Phase 13 Overview](./roadmap/phase-13-overview.md)

---

## Epic 4: Typed Ports & Capabilities (NEW)

> Roles should declare what they *can do*, not just what they *are*. Type-based wiring instead of name-based wiring is what makes systems evolvable.

*Source: Theme 1 — convergence across all four expert analyses. This is the single highest-leverage abstraction change.*

### Phase 14: Capability Manifests

[Phase 14 Overview](./roadmap/phase-14-overview.md)

### Phase 15: Contract Testing

[Phase 15 Overview](./roadmap/phase-15-overview.md)

---

## Epic 5: Server Pipeline Engine -- [design](./analysis/epic-5.md)

*Renumbered 2026-04-07 from original Epic 3. Exposes AIChat's unique runtime capabilities over HTTP, turning the server from a proxy into a pipeline execution engine.*

> **[DEFERRED 2026-04-17]** Phases 16, 17, and 18 are parked while Epic 9
> (Knowledge Evolution) is in flight. The existing `--serve` behavior is
> unchanged; expanding the server surface is a future-session decision.

### Phase 16: Server Hardening

[Phase 16 Overview](./roadmap/phase-16-overview.md)

### Phase 17: Role & Pipeline Execution

[Phase 17 Overview](./roadmap/phase-17-overview.md)

### Phase 18: Discovery & Estimation

[Phase 18 Overview](./roadmap/phase-18-overview.md)

---

## Epic 6: Universal Addressing (NEW)

> A pipeline stage that says `role: "review"` should resolve identically whether `review` is a local YAML file, an agent directory, a role exposed by a remote aichat server, or an MCP tool.

*Source: Theme 5 — AI Architect. The "remote aichat" discovery is the seed of this epic. Absorbs Epic 10's F4 (unified entity resolution), F6 (agent-in-pipeline), and F7 (agent MCP binding).*

### Phase 19: RoleResolver & Unified Entity Resolution

[Phase 19 Overview](./roadmap/phase-19-overview.md)

### Phase 20: Remote & Federated Composition

[Phase 20 Overview](./roadmap/phase-20-overview.md)

---

## Epic 7: DAG Execution (NEW)

> Sequential pipelines are the bicycle. DAGs with conditional routing are the car. The runtime foundation (concurrent tool execution via `join_all`) already exists.

*Source: Theme 4 — AI Architect. Three new primitives within the existing pipeline model: fan-out, conditional, merge.*

### Phase 21: Pipeline DAG Primitives

[Phase 21 Overview](./roadmap/phase-21-overview.md)

### Phase 22: DAG Observability & Budget

[Phase 22 Overview](./roadmap/phase-22-overview.md)

---

## Epic 8: Feedback Loop (NEW)

> Roles have no metrics, no regression testing, no A/B comparison. Every role invocation should be a scored data point.

*Source: Theme 2 — ML Engineer + ML App Engineer analyses. Closes the gap between "prompt template" and "optimizable, testable, versionable component."*

### Phase 23: Role Evaluation

[Phase 23 Overview](./roadmap/phase-23-overview.md)

### Phase 24: Regression Testing & Prompt Distillation

[Phase 24 Overview](./roadmap/phase-24-overview.md)

---

## Epic 9: Knowledge Evolution -- [design](./analysis/epic-9.md)

*Renumbered 2026-04-07 from original Epic 4. Reshaped 2026-04-16 from vector-RAG improvements to atomic-fact knowledge compilation.*

### Phase 25: Knowledge Compilation

[Phase 25 Overview](./roadmap/phase-25-overview.md)

### Phase 26: Knowledge Query

[Phase 26 Overview](./roadmap/phase-26-overview.md)

### Phase 27: Knowledge Evolution

[Phase 27 Overview](./roadmap/phase-27-overview.md)

---

## Epic 10: Entity Evolution -- [design](./analysis/epic-10.md)

*Renumbered 2026-04-07 from original Epic 5. F4/F6/F7 absorbed by Epic 6 (Universal Addressing).*

### Phase 28: Agent Composability

[Phase 28 Overview](./roadmap/phase-28-overview.md)

### Phase 29: Agent Dynamism

[Phase 29 Overview](./roadmap/phase-29-overview.md)

---

## Epic 11: Bridge Retirement & MCP Pool Hardening -- [design](./architecture/integrated-architecture/bridge-retirement.md)

*Two upstream aichat fixes plus a portable-file loader, gated by `tests/integration/mcp-server.sh`. Lands the aichat-side enablement for retiring the Node HTTP bridge in [llm-functions](https://github.com/jikanter/personal-llm-functions). The retirement diff in `llm-functions` is out of scope for this repo.*

**Status (2026-05-04):** All five items (31A–31E) shipped. All four validation gates green: 10/10 mcp-server.sh passing (no skips), 9/9 mcp-validate.sh passing, demo refreshed, portable loader live. Bridge deletion in `llm-functions` is unblocked.

### Phase 31: Bridge Retirement & MCP Pool Hardening

[Phase 31 Overview](./roadmap/phase-31-overview.md)

---

## Epic 12: Developer Experience & Performance

### Phase 30: Macro Compilation

[Phase 30 Detail](./roadmap/phase-30-macro-compilation.md)

---

## Cross-Epic Dependency Graph

```
Epic 1 (Core Platform)         ──── DONE ──────────────────────────────────────────
  │
  ├── Epic 2 (Runtime Intelligence) ─── Phases 9-11 ──── DONE
  │     │
  │     ├── Epic 3 (Composition UX) ─── Phases 12-13
  │     │     │
  │     │     └── Epic 4 (Typed Ports) ─── Phases 14-15
  │     │           │
  │     │           ├── Epic 5 (Server Engine) ─── Phases 16-18 ─── DEFERRED
  │     │           │     │
  │     │           │     └── Epic 6 (Universal Addressing) ─── Phases 19-20
  │     │           │           │
  │     │           │           └── Epic 7 (DAG Execution) ─── Phases 21-22
  │     │           │
  │     │           └── Epic 8 (Feedback Loop) ─── Phases 23-24 ─── Independent track
  │     │
  │     └── Epic 9 (Knowledge Evolution) ─── Phases 25-27 ──── DONE
  │
  ├── Epic 10 (Entity Evolution) ─── Phases 28-29
  │
  ├── Epic 11 (Bridge Retirement) ─── Phase 31 ──── DONE
  │
  └── Epic 12 (Macro Compilation) ─── Phase 30 ──── DONE
```

**Critical path (active):** Epic 4 → Epic 6 → Epic 7. Epic 5 is deferred; Epic 8 is an independent track that can proceed in parallel.

---

## What NOT to Build

| Proposal | Reason | Source |
|---|---|---|
| LiteLLM as dependency | Python runtime conflicts with single-binary constraint. Already works via `openai-compatible` client. | Epic 2 |
| Semantic caching with vector DB | Exact-match cache (Phase 10B) covers the high-value case. Semantic dedup can be a pipeline role. | ML App Engineer |
| Multi-agent orchestration framework | Over-engineering. Agent-as-tool + pipelines + macros compose to cover every topology. | Epic 5 |
| Token-exact counting (tiktoken) | Only covers OpenAI tokenizers. Budget allocation needs order-of-magnitude, not exact precision. | Epic 2 |
| Knowledge graph with entity extraction | Requires LLM calls per chunk during indexing. Violates cost-conscious constraint. | Epic 4 |
| Visual pipeline designer GUI | Violates "no desktop UI" constraint. Roles are YAML files; text editor is the authoring tool. | Epic 3 |
| Event bus / message passing between agents | Wrong abstraction for single-shot CLI. Agent-as-tool IS the communication channel. | Epic 5 |
| Full-blown package registry for roles | Premature. `--fork-role` + git + `extends` covers sharing. Registry adds platform burden. | UX Designer |
| Real-time file watching daemon | CLI tools are invocation-based. Use git hooks, cron, or shell loops. | AI Architect |
| Confidence scoring on LLM output | Research problem, not engineering. No reliable way without another LLM call. | Epic 2 |

---

## Success Metrics

| Metric | Current State | Target | Epic |
|---|---|---|---|
| Schema failure rate with `output_schema` | Unknown | <5% (Phase 9A/B), <1% (Phase 9C) | 2 |
| Pipeline re-run cost after stage failure | 100% (full re-run) | Stage cost only (Phase 10B cache) | 2 |
| Time to understand a role before using it | Read YAML file | `--dry-run` shows everything in 0 tokens | 3 |
| Time to create a role variant | 5 min (copy + edit) | 5 sec (`--fork-role`) | 3 |
| Can compose roles across machines | Accidental (HTTP hack) | First-class (`remote:host/role`) | 6 |
| Pipeline topology | Sequential only | Fan-out, conditional, merge | 7 |
| Role quality tracking | None | Per-role metrics + regression tests | 8 |
| AIChat features accessible via HTTP | 3 (chat, embed, rerank) | 8+ (roles, pipelines, batch, cost) | 5 |
| Context utilization for `-f dir/` | 100% of files (wasteful) | BM25-ranked, budget-optimized | 2 |
| Pre-flight error prevention | 0 errors caught | All capability mismatches caught | 2 |
| Batch cost savings with mixed complexity | 0% (static model) | 40-60% (deterministic routing) | 2 |

---

## Phase Summary Table

| Phase | Epic | Scope | Key Deliverable |
|---|---|---|---|
| 0-8 | 1: Core Platform | Done | Foundation |
| 9 | 2: Runtime Intelligence | Schema Fidelity | Native structured output, schema retry |
| 10 | 2: Runtime Intelligence | Resilience | API retry, stage cache, stage retry, model fallback |
| 11 | 2: Runtime Intelligence | Context Budget | Budget allocator, BM25 ranking; pipeline budget propagation (11D) planned |
| 12 | 3: Composition UX | Discoverability | `--dry-run` resolved, port signatures, composition summaries |
| 13 | 3: Composition UX | Authoring | `--fork-role`, error teaching, guardrail examples |
| 14 | 4: Typed Ports | Capabilities | `capabilities:` field, capability resolver, `--find-role` |
| 15 | 4: Typed Ports | Contract Testing | Pipeline schema validation, `--check` flag |
| 16 | 5: Server Engine | Hardening | CORS, auth, health, cost headers |
| 17 | 5: Server Engine | Execution | Virtual models, role invoke, pipeline endpoint |
| 18 | 5: Server Engine | Discovery | Cost estimation, OpenAPI spec |
| 19 | 6: Universal Addressing | Resolution | `RoleResolver` trait, unified `-r`, agent-in-pipeline |
| 20 | 6: Universal Addressing | Federation | Remote roles, `remotes:` config, federated pipelines |
| 21 | 7: DAG Execution | Primitives | `parallel:`, `switch:`/`when:`, merge strategies |
| 22 | 7: DAG Execution | Observability | DAG trace, per-branch cost, budget-aware fan-out |
| 23 | 8: Feedback Loop | Evaluation | `metrics:` field, `--compare`, cost attribution |
| 24 | 8: Feedback Loop | Testing | Role regression, role-as-judge, prompt distillation |
| 25 | 9: Knowledge Evolution | Compilation | EDP data model, knowledge compiler, AEVS restore-check, KB storage |
| 26 | 9: Knowledge Evolution | Query | Tag-filter + BM25, graph walk, role `knowledge:`, multi-KB RRF |
| 27 | 9: Knowledge Evolution | Evolution | Mutation API, ACE reflect/curate, attributed output |
| 28 | 10: Entity Evolution | Composability | Agent-as-tool, configurable loop, macro chaining |
| 29 | 10: Entity Evolution | Dynamism | ReactPolicy trait, agent memory |
| 30 | 12: Developer Experience & Performance | Macro Compilation | Trait-based defaults, modular `register_client!`, slim `impl_client_trait!` |
| 31 | 11: Bridge Retirement | MCP Pool Hardening | `ToolCall::eval` MCP routing, multi-server pool fix, portable `mcp.json` loader |
