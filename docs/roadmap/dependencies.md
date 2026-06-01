# Cross-Epic Dependency Graph

How the epics relate. Linked from [`ROADMAP.md`](../ROADMAP.md).

```
Epic 1 (Core Platform)         ──── DONE ──────────────────────────────────────────
  │
  ├── Epic 2 (Runtime Intelligence) ─── Phases 9-11, 37-41 ─ caching sub-track 37→41 outstanding
  │     │
  │     ├── Epic 3 (Composition UX) ─── Phase 12 DONE, Phase 13 planned
  │     │     │
  │     │     └── Epic 4 (Typed Ports) ─── Phase 14 DONE, Phase 15 partial
  │     │           │
  │     │           ├── Epic 5 (Server Engine) ─── Phase 16 (F/G/H) + 17 DONE; 18 deferred
  │     │           │     │
  │     │           │     └── Epic 6 (Universal Addressing) ─── Phases 19-20 ──── DONE
  │     │           │           │
  │     │           │           └── Epic 7 (DAG Execution) ─── Phase 21 DONE; 22 planned
  │     │           │
  │     │           └── Epic 8 (Feedback Loop) ─── Phases 23-24 ─── Independent track
  │     │
  │     └── Epic 9 (Knowledge Evolution) ─── Phases 25-27 ──── DONE
  │
  ├── Epic 10 (Entity Evolution) ─── Phases 28-29 ─── Planned
  │
  ├── Epic 11 (Bridge Retirement) ─── Phase 31 ──── DONE
  │
  ├── Epic 12 (Macro Compilation) ─── Phase 30 ──── DONE
  │
  └── Epic 13 (Pi as REPL Surface) ─── Phase 32 ──── DONE
```

**Critical path (active):** Phase 11D → Phase 13 → Phase 15 (B/C) → Phase 22.
Epic 8 (23-24) and Epic 10 (28-29) are independent tracks that can proceed in parallel.
The **caching sub-track** (Epic 2, Phases 37 → 38 → 39 → 40 → 41) is an independent parallel track that ports [LiteLLM's caching subsystem](https://github.com/BerriAI/litellm/tree/main/litellm/caching) feature-for-feature ([`EVAL-0004`](../analysis/caching/EVAL-0004-litellm-cache-parity.md)):

```
Phase 37 (layers: L1/L2/L3, accounting, trace, pi)   37A → 37B → 37C → 37D → 37E   (37F deferred)
   └─ 37E couples to the open-harness trace workstream (schema_version bump for cache.lookup)
Phase 38 (CacheBackend trait + control protocol)     blocked by 37A (CallMetrics) + 37E (trace)
   ├─ Phase 39 (remote backends, cargo-gated)         blocked by 38A
   ├─ Phase 40 (embedding/rerank caching)             blocked by 38A, 38E
   └─ Phase 41 (admin & observability surface)         blocked by 38A, extends 37D
```

The pi integration is 37D — every pi turn already flows through the in-process `serve.rs`, so wiring the cache there is transparent. 38A's trait is the single hard gate: 39, 40, and 41 are mutually independent but each needs the backend trait to exist. 39's remote backends are cargo-gated, so they add zero dependencies to the default build.
Phase 18 (server discovery/estimation) remains deferred.
