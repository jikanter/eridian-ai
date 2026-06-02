# Cross-Epic Dependency Graph

How the epics relate, and which repo each lands in. Linked from [`../ROADMAP.md`](../ROADMAP.md).
Repo tags: **aichat** · **llm-functions** · **harness (pi)** · **cross-repo**.

```
Epic 1 (Core Platform, aichat)  ──── DONE (Phases 0–8) ───────────────────────────
  │
  ├── Epic 2 (Runtime Intelligence, aichat) ─ Phases 9-11 DONE; caching 37→41 outstanding
  │     │
  │     ├── Epic 3 (Composition UX, aichat) ─── Phases 12, 13 ──── DONE
  │     │     │
  │     │     └── Epic 4 (Typed Ports, aichat) ─── Phases 14, 15, 33 ──── DONE
  │     │           │
  │     │           ├── Epic 5 (Server Engine, aichat) ─── Phases 16, 17 DONE; 18 DEFERRED
  │     │           │     │
  │     │           │     └── Epic 6 (Universal Addressing, aichat) ─ Phases 19, 20 ── DONE
  │     │           │           │
  │     │           │           └── Epic 7 (DAG Execution, aichat) ─ Phases 21, 22, 36 ─ DONE
  │     │           │
  │     │           └── Epic 8 (Feedback Loop, aichat) ─ Phase 23 DONE; 24 PLANNED (indep.)
  │     │
  │     └── Epic 9 (Knowledge Evolution, aichat) ─── Phases 25-27 ──── DONE
  │
  ├── Epic 10 (Entity Evolution, aichat ↔ llm-functions) ─ Phases 28-29 ─ PLANNED
  │
  ├── Epic 11 (Bridge Retirement, aichat ↔ llm-functions ↔ harness) ─ Phase 31 ─ DONE
  │
  ├── Epic 12 (Developer Experience, aichat) ─── Phase 30 ──── DONE
  │
  ├── Epic 13 (Pi as REPL Surface, aichat ↔ harness) ─── Phase 32 ──── DONE
  │
  └── Epic 14 (Memory Surface, aichat ↔ harness) ─ Phase 34 DONE; 35 PLANNED
```

**Critical path (shipped through Phase 36):**
Phase 11D → Phase 13 → Phase 15B/C → Phase 22 → Phase 33 → Phase 36 — all **Done**.
The active critical path is now the **caching sub-track** plus the parallel **Memory Surface**,
**Feedback Loop**, and **Entity Evolution** tracks (see below).

## Parallel tracks (active)

The **caching sub-track** (Epic 2, Phases 37 → 38 → 39 → 40 → 41) ports
[LiteLLM's caching subsystem](https://github.com/BerriAI/litellm/tree/main/litellm/caching)
feature-for-feature ([`EVAL-0004`](../analysis/caching/EVAL-0004-litellm-cache-parity.md)):

```
Phase 37 (layers: L1/L2/L3, accounting, trace, pi)   37A → 37B → 37C → 37D → 37E   (37F deferred)
   └─ 37E couples to the open-harness trace workstream (schema_version bump for cache.lookup)
   └─ 37D wires the cache into serve.rs — the gateway every pi (harness) turn flows through
Phase 38 (CacheBackend trait + control protocol)     blocked by 37A (CallMetrics) + 37E (trace)
   ├─ Phase 39 (remote backends, cargo-gated)         blocked by 38A
   ├─ Phase 40 (embedding/rerank caching)             blocked by 38A, 38E
   └─ Phase 41 (admin & observability surface)         blocked by 38A, extends 37D
```

38A's trait is the single hard gate: 39, 40, and 41 are mutually independent but each needs the
backend trait to exist. 39's remote backends are cargo-gated, so they add zero dependencies to
the default build.

Other independent tracks that can proceed in parallel:

- **Memory Surface** (Epic 14): Phase 34 **Done** → Phase 35 (knowledge-MCP) **Planned** — cross-repo with the harness.
- **Feedback Loop** (Epic 8): Phase 23 **Done** → Phase 24 (regression & distillation) **Planned**.
- **Entity Evolution** (Epic 10): Phases 28-29 **Planned** — cross-repo, since agents are defined in llm-functions.

**Deferred:** Phase 18 (server discovery / estimation).
