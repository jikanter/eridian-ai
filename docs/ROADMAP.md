# Eridian Roadmap

**Last updated:** 2026-05-26

## Vision

AIChat is **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and humans compose multi-model pipelines, consume external tools via MCP, and expose roles as callable infrastructure. The REPL is provided by [pi](https://github.com/earendil-works/pi) (Epic 13); aichat owns inference, roles, agents, RAG, MCP, and macros.

Roles are the fundamental unit of composition. The roadmap evolves roles from static prompt templates into **typed, addressable, evaluable building blocks** that compose across machines, execution models, and cost budgets.

### Governing Constraints

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No desktop UI, no breaking argc/llm-functions** without explicit approval.

---

## Status

One row per phase. "Sub-phases" lists the granular state; "Last update" is the most recent dated change in the phase doc.

| Epic | Phase | Scope | Sub-phases | Last update | Detail |
|---|---|---|---|---|---|
| 1 Core Platform | 0–8 | Foundation | all **Done** | — | [completed-epics.md](archive/roadmap/completed-epics.md) |
| 2 Runtime Intelligence | 9 | Schema fidelity | 9A–D **Done** | 2026-03-16 | [phase-9-overview.md](roadmap/phase-9-overview.md) |
| 2 Runtime Intelligence | 10 | Resilience & retry | 10A–D **Done**; `model_policy` ruled out | 2026-05-11 | [phase-10-overview.md](roadmap/phase-10-overview.md) |
| 2 Runtime Intelligence | 11 | Context budget | 11A/B **Done**, 11C **Superseded**, 11D **Planned** | 2026-05-11 | [phase-11-overview.md](roadmap/phase-11-overview.md) |
| 3 Composition UX | 12 | Discoverability | 12A–D **Done** | 2026-05-04 | [phase-12-overview.md](roadmap/phase-12-overview.md) |
| 3 Composition UX | 13 | Authoring & teaching | 13A–D **Done** | 2026-05-29 | [phase-13-overview.md](roadmap/phase-13-overview.md) |
| 4 Typed Ports | 14 | Capability manifests | 14A–D **Done** | 2026-05-04 | [phase-14-overview.md](roadmap/phase-14-overview.md) |
| 4 Typed Ports | 15 | Contract testing | 15A **Partial**, 15B/C **Planned** | 2026-05-11 | [phase-15-overview.md](roadmap/phase-15-overview.md) |
| 4 Typed Ports | 33 | Typed input surface | 33A–E **Planned** | 2026-05-23 | [phase-33-overview.md](roadmap/phase-33-overview.md) |
| 5 Server Engine | 16 | Server hardening | 16F/G/H **Done**; 16A–E **Deferred** | 2026-05-11 | [phase-16-overview.md](roadmap/phase-16-overview.md) |
| 5 Server Engine | 17 | Role & pipeline execution | 17A–E **Done** (un-deferred) | 2026-05-11 | [phase-17-overview.md](roadmap/phase-17-overview.md) |
| 5 Server Engine | 18 | Discovery & estimation | 18A–C **Deferred** | 2026-04-17 | [phase-18-overview.md](roadmap/phase-18-overview.md) |
| 6 Universal Addressing | 19 | RoleResolver | 19A–D **Done** | 2026-05-04 | [phase-19-overview.md](roadmap/phase-19-overview.md) |
| 6 Universal Addressing | 20 | Remote & federated | 20A–D **Done** | 2026-05-11 | [phase-20-overview.md](roadmap/phase-20-overview.md) |
| 7 DAG Execution | 21 | DAG primitives | 21A–D **Done** | 2026-05-11 | [phase-21-overview.md](roadmap/phase-21-overview.md) |
| 7 DAG Execution | 22 | DAG observability & budget | 22A–E **Planned** | — | [phase-22-overview.md](roadmap/phase-22-overview.md) |
| 7 DAG Execution | 36 | Pipeline stage config isolation | 36A–D **Planned** | 2026-05-26 | [phase-36-overview.md](roadmap/phase-36-overview.md) |
| 8 Feedback Loop | 23 | Role evaluation | 23A–D **Planned** | — | [phase-23-overview.md](roadmap/phase-23-overview.md) |
| 8 Feedback Loop | 24 | Regression & distillation | 24A–D **Planned** | — | [phase-24-overview.md](roadmap/phase-24-overview.md) |
| 9 Knowledge Evolution | 25 | Knowledge compilation | **Done** (rewritten 2026-04-16) | — | [phase-25-knowledge-compilation.md](roadmap/phase-25-knowledge-compilation.md) |
| 9 Knowledge Evolution | 26 | Knowledge query & composability | **Done** | — | [phase-26-knowledge-query.md](roadmap/phase-26-knowledge-query.md) |
| 9 Knowledge Evolution | 27 | Evolution, attribution & trace | **Done** | — | [phase-27-knowledge-evolution.md](roadmap/phase-27-knowledge-evolution.md) |
| 10 Entity Evolution | 28 | Agent composability | 28A–C **Planned** | — | [phase-28-overview.md](roadmap/phase-28-overview.md) |
| 10 Entity Evolution | 29 | Agent dynamism | 29A/B **Planned** | — | [phase-29-overview.md](roadmap/phase-29-overview.md) |
| 11 Bridge Retirement | 31 | MCP pool hardening | 31A–E **Done** | 2026-05-11 | [phase-31-overview.md](roadmap/phase-31-overview.md) |
| 12 Developer Experience | 30 | Macro compilation | 30A–D **Done** | — | [phase-30-macro-compilation.md](roadmap/phase-30-macro-compilation.md) |
| 13 Pi as REPL Surface | 32 | Pi cutover | 32A–D **Done** | 2026-05-11 | [repl-pi.md](features/repl-pi.md) |
| 14 Memory Surface | 34 | Auto-memory wiring | 34A–D **Planned** | 2026-05-26 | [phase-34-overview.md](roadmap/phase-34-overview.md) |
| 14 Memory Surface | 35 | Knowledge-MCP protocol | 35A–D **Planned** | 2026-05-26 | [phase-35-overview.md](roadmap/phase-35-overview.md) |

---

## Active Track

Sequential critical path: **Phase 11D → Phase 15B/C → Phase 33 → Phase 22** (Phase 13 **Done** 2026-05-29). Phase 33 (typed input surface) slots after 15B/C because 33D extends the same containment-check logic into adjacent-stage shape validation — and now also extends Phase 13B's `schema_field_diff` field-delta reporting.
Parallel independent tracks: **Epic 8** (Phases 23–24, role evaluation), **Epic 10** (Phases 28–29, agent evolution), and **Epic 14 Memory Surface** (Phase 34 → Phase 35, Posture-C dual-store wiring from the 2026-05-24 divergence playbook). **Pipeline isolation** (Phase 36) slots after Phase 22 inside Epic 7 and extends the existing model-restore pattern in `run_stage`.
Deferred: **Phase 18** (server discovery/estimation), **Phase 16A–E** (server hardening beyond what Phase 20 federation required).

---

## References

- **Architecture:** [architecture.md](architecture/architecture.md)
- **Per-epic designs:** [analysis/](analysis/) (one `epic-N.md` per epic)
- **Dependency graph:** [roadmap/dependencies.md](roadmap/dependencies.md)
- **Success metrics:** [roadmap/success-metrics.md](roadmap/success-metrics.md)
- **Anti-roadmap (what NOT to build):** [roadmap/anti-roadmap.md](roadmap/anti-roadmap.md)
- **Integrated (cross-repo) requirements:** [architecture/integrated-architecture/](architecture/integrated-architecture/)
- **Completed epics:** [archive/roadmap/completed-epics.md](archive/roadmap/completed-epics.md)
