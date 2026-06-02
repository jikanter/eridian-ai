# Eridian Roadmap

**Last updated:** 2026-06-02 · **Horizon model:** Now / Next / Later · **Repos:** aichat · llm-functions · harness (pi)

## Vision

AIChat is **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and
humans compose multi-model pipelines, consume external tools via MCP, and expose roles as
callable infrastructure. The REPL is provided by [pi](https://github.com/earendil-works/pi)
(Epic 13); aichat owns inference, roles, agents, RAG, MCP, and macros.

Roles are the fundamental unit of composition. The roadmap evolves roles from static prompt
templates into **typed, addressable, evaluable building blocks** that compose across machines,
execution models, and cost budgets.

### Strategy pillars

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No desktop UI, no breaking argc/llm-functions** without explicit approval.
- **Runs as well on local models as on frontier models.**

---

## The three repositories

Eridian is one product spanning three repos. Roadmap items are tagged by the repo that owns
the work so readers can see *where it lands*.

| Tag | Repo | Owns |
|---|---|---|
| **aichat** | this repo | CLI / runtime / MCP server-and-client; inference, roles, agents, RAG, MCP, macros, caching, HTTP server. |
| **llm-functions** | [jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions) | Tool + agent declarations that aichat loads and runs. |
| **harness (pi)** | [earendil-works/pi](https://github.com/earendil-works/pi) | The REPL/harness surface that other clients consume aichat through. |
| **cross-repo** | [`docs/architecture/integrated-architecture/`](architecture/integrated-architecture/) | Requirements that only make sense across ≥2 repos. Adjacent peer repos **astrophage** (wire-level record/replay/cache substrate) and **brief** are specced there. |

---

## Horizons (the product view)

What is in flight, what is committed next, what is parked — independent of epic numbering.

### Now — in flight

| Theme | Work | Repo |
|---|---|---|
| Runtime Intelligence | **Phase 37 — transparent response caching** (37A–E). The active build focus on the [architecture diagram](architecture.svg); ports [LiteLLM caching](https://github.com/BerriAI/litellm/tree/main/litellm/caching) per [`EVAL-0004`](analysis/caching/EVAL-0004-litellm-cache-parity.md). 37D wires the cache into `serve.rs`, which every pi turn flows through. | aichat (37D touches harness) |
| Memory Surface | **Phase 35 — knowledge-MCP protocol** (35A–D). Exposes the auto-memory store (Phase 34) over MCP. | cross-repo (aichat ↔ harness) |
| Feedback Loop | **Phase 24 — regression & distillation** (24A–D). Builds on shipped Phase 23 role evaluation. | aichat |

### Next — committed, not started

| Theme | Work | Repo |
|---|---|---|
| Runtime Intelligence | **Phases 38 → 39 → 40 → 41** caching sub-track: backend trait + control protocol (38), cargo-gated remote backends (39), embedding/rerank caching (40), admin & observability parity (41). | aichat |
| Entity Evolution | **Phases 28–29** — agent composability & dynamism. Agents are defined in llm-functions, so this is inherently cross-repo. | cross-repo (aichat ↔ llm-functions) |

### Later — parked / deferred

| Theme | Work | Repo |
|---|---|---|
| Server Engine | **Phase 18 — discovery & estimation.** Deferred 2026-04-17 in favor of knowledge work; no active demand. | aichat |

### Shipped this cycle (Apr–Jun 2026)

Phases **13, 15, 16, 22, 23, 33, 34, 36** all reached **Done**. See the status ledger for the
per-phase record. Epic 1 (Phases 0–8) foundation shipped earlier — archived in
[completed-epics.md](roadmap/archive/completed-epics.md).

---

## Themes → epics

The 14 epics roll up into four strategic outcomes. Repo tag shows where each epic's work lands.

| Outcome | Epics | Repo |
|---|---|---|
| **A runtime that is cheap, correct, and resilient** | 1 Core Platform · 2 Runtime Intelligence (incl. caching 37–41) · 9 Knowledge Evolution | aichat |
| **Roles as typed, composable, evaluable infrastructure** | 3 Composition UX · 4 Typed Ports · 7 DAG Execution · 8 Feedback Loop | aichat |
| **Addressable & federated across machines and repos** | 5 Server Engine · 6 Universal Addressing · 11 Bridge Retirement | aichat / cross-repo |
| **Surfaces: REPL, memory, and agent evolution** | 13 Pi as REPL Surface · 14 Memory Surface · 10 Entity Evolution · 12 Developer Experience | cross-repo / aichat |

---

## Status ledger

One row per phase. **Owner** is the repo the work lands in. "Sub-phases" lists granular state;
"Last update" is the most recent dated change in the phase doc.

| Epic | Phase | Owner | Scope | Sub-phases | Last update | Detail |
|---|---|---|---|---|---|---|
| 1 Core Platform | 0–8 | aichat | Foundation | all **Done** | — | [completed-epics.md](roadmap/archive/completed-epics.md) |
| 2 Runtime Intelligence | 9 | aichat | Schema fidelity | 9A–D **Done** | 2026-03-16 | [phase-9-overview.md](roadmap/phase-9-overview.md) |
| 2 Runtime Intelligence | 10 | aichat | Resilience & retry | 10A–D **Done**; `model_policy` ruled out | 2026-05-11 | [phase-10-overview.md](roadmap/phase-10-overview.md) |
| 2 Runtime Intelligence | 11 | aichat | Context budget | 11A/B **Done**, 11C **Superseded**, 11D **Done** | 2026-05-28 | [phase-11-overview.md](roadmap/phase-11-overview.md) |
| 2 Runtime Intelligence | 37 | aichat (37D ↔ harness) | Transparent response caching | 37A–E **Planned** (Now); 37F deferred | 2026-05-29 | [phase-37-overview.md](roadmap/phase-37-overview.md) |
| 2 Runtime Intelligence | 38 | aichat | Cache backend abstraction & control protocol | 38A–E **Planned** (Next) | 2026-05-29 | [phase-38-overview.md](roadmap/phase-38-overview.md) |
| 2 Runtime Intelligence | 39 | aichat | Distributed & remote cache backends (cargo-gated) | 39A–D **Planned** (Next) | 2026-05-29 | [phase-39-overview.md](roadmap/phase-39-overview.md) |
| 2 Runtime Intelligence | 40 | aichat | Auxiliary-call caching (embeddings & rerank) | 40A–D **Planned** (Next) | 2026-05-29 | [phase-40-overview.md](roadmap/phase-40-overview.md) |
| 2 Runtime Intelligence | 41 | aichat | Cache observability & admin parity | 41A–D **Planned** (Next) | 2026-05-29 | [phase-41-overview.md](roadmap/phase-41-overview.md) |
| 3 Composition UX | 12 | aichat | Discoverability | 12A–D **Done** | 2026-05-04 | [phase-12-overview.md](roadmap/phase-12-overview.md) |
| 3 Composition UX | 13 | aichat | Authoring & teaching | 13A–D **Done** | 2026-05-29 | [phase-13-overview.md](roadmap/phase-13-overview.md) |
| 4 Typed Ports | 14 | aichat | Capability manifests | 14A–D **Done** | 2026-05-04 | [phase-14-overview.md](roadmap/phase-14-overview.md) |
| 4 Typed Ports | 15 | aichat | Contract testing | 15A–C **Done** | 2026-05-29 | [phase-15-overview.md](roadmap/phase-15-overview.md) |
| 4 Typed Ports | 33 | aichat | Typed input surface | 33A–E **Done** | 2026-05-30 | [phase-33-overview.md](roadmap/phase-33-overview.md) |
| 5 Server Engine | 16 | aichat | Server hardening | 16A–I **Done** | 2026-05-29 | [phase-16-overview.md](roadmap/phase-16-overview.md) |
| 5 Server Engine | 17 | aichat | Role & pipeline execution | 17A–E **Done** (un-deferred) | 2026-05-11 | [phase-17-overview.md](roadmap/phase-17-overview.md) |
| 5 Server Engine | 18 | aichat | Discovery & estimation | 18A–C **Deferred** (Later) | 2026-04-17 | [phase-18-overview.md](roadmap/phase-18-overview.md) |
| 6 Universal Addressing | 19 | aichat | RoleResolver | 19A–D **Done** | 2026-05-04 | [phase-19-overview.md](roadmap/phase-19-overview.md) |
| 6 Universal Addressing | 20 | aichat | Remote & federated | 20A–D **Done** | 2026-05-11 | [phase-20-overview.md](roadmap/phase-20-overview.md) |
| 7 DAG Execution | 21 | aichat | DAG primitives | 21A–D **Done** | 2026-05-11 | [phase-21-overview.md](roadmap/phase-21-overview.md) |
| 7 DAG Execution | 22 | aichat | DAG observability & budget | 22A–E **Done** | 2026-05-29 | [phase-22-overview.md](roadmap/phase-22-overview.md) |
| 7 DAG Execution | 36 | aichat | Pipeline stage config isolation | 36A–D **Done** | 2026-06-01 | [phase-36-overview.md](roadmap/phase-36-overview.md) |
| 8 Feedback Loop | 23 | aichat | Role evaluation | 23A–D **Done** | 2026-05-30 | [phase-23-overview.md](roadmap/phase-23-overview.md) |
| 8 Feedback Loop | 24 | aichat | Regression & distillation | 24A–D **Planned** (Now) | — | [phase-24-overview.md](roadmap/phase-24-overview.md) |
| 9 Knowledge Evolution | 25 | aichat | Knowledge compilation | **Done** (rewritten 2026-04-16) | — | [phase-25-knowledge-compilation.md](roadmap/phase-25-knowledge-compilation.md) |
| 9 Knowledge Evolution | 26 | aichat | Knowledge query & composability | **Done** | — | [phase-26-knowledge-query.md](roadmap/phase-26-knowledge-query.md) |
| 9 Knowledge Evolution | 27 | aichat | Evolution, attribution & trace | **Done** | — | [phase-27-knowledge-evolution.md](roadmap/phase-27-knowledge-evolution.md) |
| 10 Entity Evolution | 28 | aichat ↔ llm-functions | Agent composability | 28A–C **Planned** (Next) | — | [phase-28-overview.md](roadmap/phase-28-overview.md) |
| 10 Entity Evolution | 29 | aichat ↔ llm-functions | Agent dynamism | 29A/B **Planned** (Next) | — | [phase-29-overview.md](roadmap/phase-29-overview.md) |
| 11 Bridge Retirement | 31 | aichat ↔ llm-functions ↔ harness | MCP pool hardening | 31A–E **Done** | 2026-05-11 | [phase-31-overview.md](roadmap/phase-31-overview.md) |
| 12 Developer Experience | 30 | aichat | Macro compilation | 30A–D **Done** | — | [phase-30-macro-compilation.md](roadmap/phase-30-macro-compilation.md) |
| 13 Pi as REPL Surface | 32 | aichat ↔ harness | Pi cutover | 32A–D **Done** | 2026-05-11 | [repl-pi.md](features/repl-pi.md) |
| 14 Memory Surface | 34 | aichat (↔ harness reader) | Auto-memory wiring | 34A–D **Done** | 2026-05-30 | [phase-34-overview.md](roadmap/phase-34-overview.md) |
| 14 Memory Surface | 35 | cross-repo (aichat ↔ harness) | Knowledge-MCP protocol | 35A–D **Planned** (Now) | 2026-05-26 | [phase-35-overview.md](roadmap/phase-35-overview.md) |

> **Phase 8** (data processing & observability) is recorded as Done in the Epic 1 row above, but
> active follow-on work on that surface is **in progress in the main worktree** — treat its docs
> as live, not archived.

---

## Active track — sequencing detail

**Critical path is clear through Phase 36** (Done 2026-06-01). The shipped chain: Phase 11D
(budget propagation: `pipeline_budget_usd:` / `budget_weight:` + tail-truncation in
`run_stage_inner`, sub-allocated across fan-out branches by 22C) → Phase 13 (authoring &
teaching) → Phase 15B/C (cross-stage `schema_containment` + `--check`) → Phase 22 (DAG trace
tree, per-branch cost, budget-aware fan-out, stage-cache observability) → Phase 33 (typed input
surface: schema-as-source-of-truth, type-aware `{{slot}}` rendering, `-v`/stdin coercion,
adjacent-stage shape-check) → Phase 36 (pipeline stage config isolation: opt-in per-stage
`config_override:` via clone-and-merge, downward-only escalation guard, `config_overrides_applied`
telemetry). See [`docs/features/pipeline-isolation.md`](features/pipeline-isolation.md).

**Parallel independent tracks:**

- **Caching sub-track (Epic 2, Phases 37 → 38 → 39 → 40 → 41).** Strict ordering: 38 is blocked
  by 37A (`CallMetrics`) + 37E (trace); 39/40/41 are each blocked by 38A (the `CacheBackend`
  trait). 39's remote backends are cargo-gated → zero new default deps.
- **Memory Surface (Epic 14, Phase 34 → 35).** Phase 34 Done 2026-05-30 (read-only
  `memory/MEMORY.md` startup injection, `memory:<ref>` lazy load, session-exit Reflector with
  pre-Reflector secret redaction, Curator accept/skip/edit/reject gate). Phase 35 (knowledge-MCP)
  next.
- **Feedback Loop (Epic 8).** Phase 23 Done 2026-05-30 (`metrics:` scoring, `--compare`, per-role
  cost attribution + invocation ledger). Phase 24 (regression & distillation) next.
- **Entity Evolution (Epic 10, Phases 28–29).** Agent evolution; cross-repo with llm-functions.

**Deferred:** Phase 18 (server discovery / estimation).

---

## References

- **Roadmap directory index:** [roadmap/README.md](roadmap/README.md)
- **Architecture:** [architecture.md](architecture/architecture.md) &#183; **Future-state diagram:** [architecture.svg](architecture.svg)
- **Per-epic designs:** [analysis/](analysis/) (one `epic-N.md` per epic)
- **Dependency graph:** [roadmap/dependencies.md](roadmap/dependencies.md)
- **Success metrics:** [roadmap/success-metrics.md](roadmap/success-metrics.md)
- **Anti-roadmap (what NOT to build):** [roadmap/anti-roadmap.md](roadmap/anti-roadmap.md)
- **Integrated (cross-repo) requirements:** [architecture/integrated-architecture/](architecture/integrated-architecture/)
- **Completed epics:** [roadmap/archive/completed-epics.md](roadmap/archive/completed-epics.md)
- **This refresh's notes:** [roadmap/REFRESH-NOTES.md](roadmap/REFRESH-NOTES.md)
</content>
