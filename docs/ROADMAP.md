# Eridian Roadmap

**Last updated:** 2026-06-04 · **Horizon model:** Now / Next / Later · **Coverage:** the coming year (2026-06 → 2027-06)
**Repos:** aichat · llm-functions · brief · astrophage · harness (pi)
**Next Step Implementation Note (2026-06-09) **:  For the implementation thread: the load-bearing decision is §9.4 (the off-diagonal (backing × facet) presets). Phases 28/29 will harden the RoleLike → Entity trait shape by accident if built before 52. 52A (trait rename + `facets()`) and 52B (facet taxonomy + `--dry-run` surfacing) are **shipped**; 52C (collapse the variant-specific `SessionEntity` / `EntityRef` resolution branches onto the `Entity` trait, **backing-gates-ownership** as the single resolution invariant) is in flight on its own branch. **Phase 42A–B are now shipped** — the SPEC-001 trace emitter keystone (`src/utils/trace_spec/`: ULID session ids, the 17-variant event envelope, the `LineSink`/`TraceSender` dedicated OS writer thread with bounded `sync_channel` + in-band `trace.dropped` accounting, the `env_subset` redaction gate, and now the **content-addressed blob store** — `blob.rs`: SHA-256 + two-level sharded `blobs/ab/cd/<hex>` + write-once `create_new`/O_EXCL — plus **record-time auth-header stripping** so `messages_hash` is key-independent), all built with **zero new dependencies**. 52D (trace entity attribution: `entity_id` + resolved facet set per keystone trace) is gated on Phase 42 *event coverage* — the schema and blob store now exist, so the remaining gate is **42C** (lifecycle event coverage + per-parent `manifest.jsonl`) and **42D** (`--trace`/`AICHAT_TRACE` surface unification + session-ULID correlation). The natural next concrete step is therefore **42C** → **42D**, which together unblock 52D, the test harness (43), and astrophage correlation (45D/46). The parallel unblocked Epic 10 successor remains **Phase 28** (agent composability).

## Vision

AIChat is **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and
humans compose multi-model pipelines, consume external tools via MCP, and expose roles as
callable infrastructure. The REPL is provided by [pi](https://github.com/earendil-works/pi); aichat owns all batch interfaces and the underlying implementation 
of almost all functionality.

The **Entity** is the fundamental unit of composition — a named, addressable, invocable,
traceable configuration that produces LLM calls. **Prompt, Role, Agent, and Macro are presets
over it**, not four unrelated types (see [`architecture/entity-model.md`](architecture/entity-model.md)).
The roadmap evolves entities from static prompt templates into **typed, addressable, evaluable
building blocks** that compose across machines, execution models, and cost budgets — and, in the
coming year, into a four-repo ecosystem that can **record, replay, and evaluate itself**.

### Strategy pillars

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No desktop UI, no breaking argc/llm-functions** without explicit approval.
- **Runs as well on local models as on frontier models.**
- **The trace is the keystone.** Testing, evaluation, training extraction, and observability all
  read one structured artifact — never a per-tool data model.
- **The Entity is the authoring counterpart.** Prompt / Role / Agent / Macro are *presets* over one
  `Entity` substrate; the runtime speaks one trait. `resolve Entity → execute → emit Trace`.

---

## The integrated system — five repos

Eridian is one product spanning a small constellation of repos. Every roadmap item carries an
**owning-repo tag** so readers can see *where the work lands*.

| Tag | Repo | Owns |
|---|---|---|
| **aichat** | this repo | CLI / runtime / MCP server-and-client; inference, roles, agents, RAG, MCP, macros, caching, HTTP server, **trace emission**, deterministic tool-replay. |
| **llm-functions** | [jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions) | Tool + agent declarations aichat loads and runs; the `(tool_name, args_hash)` tool-replay contract. |
| **brief** | [jikanter/brief](https://github.com/jikanter/brief) | Format-first intent authoring (`.brief.md`). **Declares and emits; never executes** (no `tokio`/`reqwest`). Grows the `cassettes:` / `## Fixtures` binding field. |
| **astrophage** | [jikanter/astrophage](https://github.com/jikanter/astrophage) | Runtime-agnostic **wire-level record/replay/cache/mock** substrate + the shared `replay-core` crate. Reached over `base_url`. |
| **harness (pi)** | [earendil-works/pi](https://github.com/earendil-works/pi) | The REPL/harness surface other clients consume aichat through. Inherits caching/replay **by topology**. |
| **cross-repo** | [`docs/architecture/integrated-architecture/`](architecture/integrated-architecture/) | Requirements that only make sense across ≥2 of the above. |

The cross-repo seams the coming year builds out: the **trace keystone** (Phase 42, the artifact
every consumer reads), the **astrophage substrate** (Phases 45–48, still a draft —
[`SPEC-003`](analysis/caching/SPEC-003-cache-substrate.md)/[`SPEC-astrophage`](architecture/integrated-architecture/SPEC-astrophage.md)),
the **eval/replay loop** (brief ↔ astrophage ↔ llm-functions), and **agent/knowledge
federation** (aichat ↔ llm-functions ↔ harness).

---

## Horizons — the coming year (the product view)

What is in flight, what is committed next, what is parked — independent of epic numbering.

### Now — in flight

| Theme | Work | Repo |
|---|---|---|
| Runtime Intelligence | **Phase 37 → 38** caching: transparent response caching (37A–E) then the `CacheBackend` trait + control protocol (38). The active build focus on the [architecture diagram](architecture.svg). | aichat (37D touches harness) |
| Observability Keystone | **Phase 42 — trace emission** ([`SPEC-001`](analysis/caching/SPEC-001-trace-format.md)). Pulled forward: the trace is the upstream gate for astrophage, the test harness, and training. | aichat |
| Memory Surface | **Phase 35 — knowledge-MCP protocol** (35A–D). Exposes the auto-memory store (Phase 34) over MCP. | cross-repo (aichat ↔ harness) |
| Feedback Loop | **Phase 24 — regression & distillation** (24A–D). Builds on shipped Phase 23. | aichat |

### Next — committed, not started

| Theme | Work | Repo |
|---|---|---|
| Runtime Intelligence | **Phases 39 → 40 → 41** caching tail: cargo-gated remote backends (39), embedding/rerank caching (40), admin & observability parity (41). | aichat |
| Observability Keystone | **Phases 43 → 44** — test harness ([`SPEC-002`](analysis/caching/SPEC-002-test-harness.md)) then trace projections + training extraction. | aichat |
| Astrophage Substrate | **Phases 45 → 46 → 47 → 48** — cache-policy gateway + `replay-core` (45), cassette/eval-replay loop (46), mock/fault injection (47), brief companion (48). | cross-repo |
| Entity Evolution | **Phase 52 → 28 → 29 → 49** — formalize the Entity model (the foundation), then agent composability, dynamism, memory federation. | cross-repo (aichat ↔ llm-functions) |

### Later — parked / deferred

| Theme | Work | Repo |
|---|---|---|
| Federation & Scale | **Phase 50** (knowledge-as-cassette / federated KB), **Phase 51** (vendor model extensions). | aichat / cross-repo |
| Runtime Intelligence | **37F** semantic cache (deferred within Phase 37 until L1/L3 are measured). | aichat |
| Server Engine | **Phase 18 — discovery & estimation.** Deferred 2026-04-17; no active demand. | aichat |

### Shipped (through this cycle)

Phases **0–17, 19–23, 25–27, 30–34, 36** are **Done** and archived in
[`roadmap/archive/completed-epics.md`](roadmap/archive/completed-epics.md). The Apr–Jun 2026
cycle landed 13, 15, 16, 22, 23, 33, 34, 36. Phase 8 is Done but stays live as active work.

---

## Themes → epics

The 17 epics roll up into **five** strategic outcomes. Repo tag shows where each epic's work lands.

| Outcome | Epics | Repo |
|---|---|---|
| **A runtime that is cheap, correct, and resilient** | 1 Core Platform · 2 Runtime Intelligence (incl. caching 37–41) · 9 Knowledge Evolution | aichat |
| **Roles as typed, composable, evaluable infrastructure** | 3 Composition UX · 4 Typed Ports · 7 DAG Execution · 8 Feedback Loop | aichat |
| **Addressable & federated across machines and repos** | 5 Server Engine · 6 Universal Addressing · 11 Bridge Retirement · **17 Federation & Scale** | aichat / cross-repo |
| **Surfaces: REPL, memory, and agent evolution** | 13 Pi as REPL Surface · 14 Memory Surface · 10 Entity Evolution · 12 Developer Experience | cross-repo / aichat |
| **An ecosystem that is observable, replayable, and evaluable** *(new)* | **15 Observability Keystone** · **16 Astrophage Substrate** | aichat / cross-repo |

The three **new** epics (15, 16, 17) are the next-year frontier; the rest are shipped or
finishing committed work.

---

## Status ledger

One row per phase. **Owner** is the repo the work lands in. Done phases link their archived
design doc; planned/new phases link the live doc.

| Epic | Phase | Owner | Scope | Sub-phases | Detail |
|---|---|---|---|---|---|
| 1 Core Platform | 0–8 | aichat | Foundation | all **Done** (8 active) | [completed-epics.md](roadmap/archive/completed-epics.md) |
| 2 Runtime Intelligence | 9 | aichat | Schema fidelity | 9A–D **Done · archived** | [archive/phase-9-overview.md](roadmap/archive/phase-9-overview.md) |
| 2 Runtime Intelligence | 10 | aichat | Resilience & retry | 10A–D **Done · archived** | [archive/phase-10-overview.md](roadmap/archive/phase-10-overview.md) |
| 2 Runtime Intelligence | 11 | aichat | Context budget | 11A/B/D **Done**, 11C superseded · archived | [archive/phase-11-overview.md](roadmap/archive/phase-11-overview.md) |
| 2 Runtime Intelligence | 37 | aichat (↔ harness) | Transparent response caching | 37A–E **Planned** (Now); 37F deferred | [phase-37-overview.md](roadmap/phase-37-overview.md) |
| 2 Runtime Intelligence | 38 | aichat | Cache backend trait & control protocol | 38A–E **Planned** (Now) | [phase-38-overview.md](roadmap/phase-38-overview.md) |
| 2 Runtime Intelligence | 39 | aichat | Remote cache backends (cargo-gated) | 39A–D **Planned** (Next) | [phase-39-overview.md](roadmap/phase-39-overview.md) |
| 2 Runtime Intelligence | 40 | aichat | Embedding & rerank caching | 40A–D **Planned** (Next) | [phase-40-overview.md](roadmap/phase-40-overview.md) |
| 2 Runtime Intelligence | 41 | aichat | Cache observability & admin parity | 41A–D **Planned** (Next) | [phase-41-overview.md](roadmap/phase-41-overview.md) |
| 3 Composition UX | 12 | aichat | Discoverability | 12A–D **Done · archived** | [archive/phase-12-overview.md](roadmap/archive/phase-12-overview.md) |
| 3 Composition UX | 13 | aichat | Authoring & teaching | 13A–D **Done · archived** | [archive/phase-13-overview.md](roadmap/archive/phase-13-overview.md) |
| 4 Typed Ports | 14 | aichat | Capability manifests | 14A–D **Done · archived** | [archive/phase-14-overview.md](roadmap/archive/phase-14-overview.md) |
| 4 Typed Ports | 15 | aichat | Contract testing | 15A–C **Done · archived** | [archive/phase-15-overview.md](roadmap/archive/phase-15-overview.md) |
| 4 Typed Ports | 33 | aichat | Typed input surface | 33A–E **Done · archived** | [archive/phase-33-overview.md](roadmap/archive/phase-33-overview.md) |
| 5 Server Engine | 16 | aichat | Server hardening | 16A–I **Done · archived** | [archive/phase-16-overview.md](roadmap/archive/phase-16-overview.md) |
| 5 Server Engine | 17 | aichat | Role & pipeline execution | 17A–E **Done · archived** | [archive/phase-17-overview.md](roadmap/archive/phase-17-overview.md) |
| 5 Server Engine | 18 | aichat | Discovery & estimation | 18A–C **Deferred** (Later) | [phase-18-overview.md](roadmap/phase-18-overview.md) |
| 6 Universal Addressing | 19 | aichat | RoleResolver | 19A–D **Done · archived** | [archive/phase-19-overview.md](roadmap/archive/phase-19-overview.md) |
| 6 Universal Addressing | 20 | aichat | Remote & federated | 20A–D **Done · archived** | [archive/phase-20-overview.md](roadmap/archive/phase-20-overview.md) |
| 7 DAG Execution | 21 | aichat | DAG primitives | 21A–D **Done · archived** | [archive/phase-21-overview.md](roadmap/archive/phase-21-overview.md) |
| 7 DAG Execution | 22 | aichat | DAG observability & budget | 22A–E **Done · archived** | [archive/phase-22-overview.md](roadmap/archive/phase-22-overview.md) |
| 7 DAG Execution | 36 | aichat | Pipeline stage config isolation | 36A–D **Done · archived** | [archive/phase-36-overview.md](roadmap/archive/phase-36-overview.md) |
| 8 Feedback Loop | 23 | aichat | Role evaluation | 23A–D **Done · archived** | [archive/phase-23-overview.md](roadmap/archive/phase-23-overview.md) |
| 8 Feedback Loop | 24 | aichat | Regression & distillation | 24A–D **Planned** (Now) | [phase-24-overview.md](roadmap/phase-24-overview.md) |
| 9 Knowledge Evolution | 25 | aichat | Knowledge compilation | **Done · archived** | [archive/phase-25-knowledge-compilation.md](roadmap/archive/phase-25-knowledge-compilation.md) |
| 9 Knowledge Evolution | 26 | aichat | Knowledge query | **Done · archived** | [archive/phase-26-knowledge-query.md](roadmap/archive/phase-26-knowledge-query.md) |
| 9 Knowledge Evolution | 27 | aichat | Evolution, attribution & trace | **Done · archived** | [archive/phase-27-knowledge-evolution.md](roadmap/archive/phase-27-knowledge-evolution.md) |
| 10 Entity Evolution | 52 | aichat | Entity model formalization (Epic 10 foundation) | 52A–B **Done** · 52C–D **Planned** (Next) | [phase-52-overview.md](roadmap/phase-52-overview.md) |
| 10 Entity Evolution | 28 | aichat ↔ llm-functions | Agent composability | 28A–C **Planned** (Next) | [phase-28-overview.md](roadmap/phase-28-overview.md) |
| 10 Entity Evolution | 29 | aichat ↔ llm-functions | Agent dynamism | 29A/B **Planned** (Next) | [phase-29-overview.md](roadmap/phase-29-overview.md) |
| 10 Entity Evolution | 49 | aichat ↔ llm-functions ↔ harness | Agent memory federation | 49A–C **Planned** (Next) | [phase-49-overview.md](roadmap/phase-49-overview.md) |
| 11 Bridge Retirement | 31 | cross-repo | MCP pool hardening | 31A–E **Done · archived** | [archive/phase-31-overview.md](roadmap/archive/phase-31-overview.md) |
| 12 Developer Experience | 30 | aichat | Macro compilation | 30A–D **Done · archived** | [archive/phase-30-macro-compilation.md](roadmap/archive/phase-30-macro-compilation.md) |
| 13 Pi as REPL Surface | 32 | aichat ↔ harness | Pi cutover | 32A–D **Done** | [features/repl-pi.md](features/repl-pi.md) |
| 13 Pi as REPL Surface | 53 | aichat ↔ harness | Discovery surface (`/aichat-flags`, `/aichat-docs`) | **Done** | [features/discovery.md](features/discovery.md) |
| 14 Memory Surface | 34 | aichat (↔ harness) | Auto-memory wiring | 34A–D **Done · archived** | [archive/phase-34-overview.md](roadmap/archive/phase-34-overview.md) |
| 14 Memory Surface | 35 | cross-repo | Knowledge-MCP protocol | 35A–D **Planned** (Now) | [phase-35-overview.md](roadmap/phase-35-overview.md) |
| **15 Observability Keystone** | 42 | aichat | Trace emission (SPEC-001) | 42A–B **Done** · 42C–D **Planned** (Now) | [phase-42-overview.md](roadmap/phase-42-overview.md) |
| **15 Observability Keystone** | 43 | aichat | Test harness (SPEC-002) | 43A–D **Planned** (Next) | [phase-43-overview.md](roadmap/phase-43-overview.md) |
| **15 Observability Keystone** | 44 | aichat | Trace projections & training extraction | 44A–D **Planned** (Next) | [phase-44-overview.md](roadmap/phase-44-overview.md) |
| **16 Astrophage Substrate** | 45 | astrophage (aichat seam) | MVP: replay-core + cache gateway | 45A–D **Planned** (Next) | [phase-45-overview.md](roadmap/phase-45-overview.md) |
| **16 Astrophage Substrate** | 46 | astrophage + aichat | Cassette policy & eval-replay loop | 46A–D **Planned** (Next) | [phase-46-overview.md](roadmap/phase-46-overview.md) |
| **16 Astrophage Substrate** | 47 | astrophage (aichat seam) | Mock policy & fault injection | 47A–C **Planned** (Next) | [phase-47-overview.md](roadmap/phase-47-overview.md) |
| **16 Astrophage Substrate** | 48 | brief (cross-repo) | brief companion: cassette bindings | 48A–C **Planned** (Next, optional) | [phase-48-overview.md](roadmap/phase-48-overview.md) |
| **17 Federation & Scale** | 50 | aichat (↔ harness) | Knowledge-as-cassette / federated KB | 50A–C **Planned** (Later) | [phase-50-overview.md](roadmap/phase-50-overview.md) |
| **17 Federation & Scale** | 51 | aichat | Vendor model extensions | 51A–C **Planned** (Later) | [phase-51-overview.md](roadmap/phase-51-overview.md) |

> **Phase 8** is recorded Done in the Epic 1 row but its observability surface remains active
> in the main worktree — treat [`roadmap/phase-8-data-observability.md`](roadmap/phase-8-data-observability.md)
> as live. Its ad-hoc trace is superseded by Phase 42.

---

## Active track — sequencing detail

**The shipped chain is complete through Phase 36** (2026-06-01). The next year runs four parallel
tracks; the one new hard gate is the **trace keystone**.

- **Observability Keystone (Epic 15, the new gate).** Phase 42 (SPEC-001 trace) is **upstream of
  almost everything new**: astrophage's `cache.lookup` correlation (45D), aichat tool-replay
  (46C), the test harness (43), and training extraction (44) all read the trace. It is therefore
  pulled into **Now**. 43 and 44 follow once the emitter exists.

- **Caching sub-track (Epic 2, Phases 37 → 38 → 39 → 40 → 41).** Strict ordering: 38 is blocked
  by 37A (`CallMetrics`) + 37E (trace); 39/40/41 each need 38A (the `CacheBackend` trait). 39's
  remote backends are cargo-gated → zero new default deps. **37F** (semantic cache) is deferred
  until L1/L3 are measured.

- **Astrophage Substrate (Epic 16, Phases 45 → 46 → 47 → 48).** 45 needs 38A (`CacheBackend`
  trait, for the `Remote` variant) + 42 (trace correlation). 46 (cassette/eval-replay) needs 45 +
  42 (tool-replay blob store) and resolves the
  [`SPEC-astrophage §9.2`](architecture/integrated-architecture/SPEC-astrophage.md) tool-replay
  key-stability stop-and-ask. 47 (mock) needs 45. 48 (brief companion) is built **in the brief
  repo**, documented here, and optional (the direct-promptfoo path works without it).

- **Entity Evolution (Epic 10, Phases 52 → 28 → 29 → 49).** **Phase 52 formalizes the Entity
  model** (the `RoleLike → Entity` trait + facet taxonomy; see
  [`architecture/entity-model.md`](architecture/entity-model.md)) as the conceptual foundation —
  28/29's new capabilities are *facets* on it. Then agent composability → dynamism → memory
  federation; cross-repo with llm-functions. 49 needs 29B (agent memory) + 35 (knowledge-MCP) +
  42 (trace attribution); 52D needs 42.

- **Memory Surface (Epic 14, 35) & Feedback Loop (Epic 8, 24)** proceed independently in **Now**.

**Deferred:** Phase 18 (server discovery / estimation); Phase 37F (semantic cache).

---

## References

- **Roadmap directory index:** [roadmap/README.md](roadmap/README.md)
- **Comprehensive archive (all shipped phases):** [roadmap/archive/completed-epics.md](roadmap/archive/completed-epics.md)
- **Architecture:** [architecture.md](architecture/architecture.md) &#183; **Future-state diagram:** [architecture.svg](architecture.svg)
- **The Entity model (foundational building block):** [architecture/entity-model.md](architecture/entity-model.md)
- **Caching / substrate analysis:** [analysis/caching/](analysis/caching/) (SPEC-001…004, ADR-0001…0005, EVAL/PLAN docs)
- **Integrated (cross-repo) requirements:** [architecture/integrated-architecture/](architecture/integrated-architecture/) (incl. [SPEC-astrophage.md](architecture/integrated-architecture/SPEC-astrophage.md))
- **Dependency graph:** [roadmap/dependencies.md](roadmap/dependencies.md)
- **Success metrics:** [roadmap/success-metrics.md](roadmap/success-metrics.md)
- **Anti-roadmap (what NOT to build):** [roadmap/anti-roadmap.md](roadmap/anti-roadmap.md)
- **This refresh's notes:** [roadmap/REFRESH-NOTES.md](roadmap/REFRESH-NOTES.md)
