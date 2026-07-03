# Cross-Epic Dependency Graph

How the epics relate, and which repo each lands in. Linked from [`../ROADMAP.md`](../ROADMAP.md).
Repo tags: **aichat** · **llm-functions** · **brief** · **astrophage** · **harness (pi)** · **cross-repo**.

```
Epic 1 (Core Platform, aichat)  ──── DONE (Phases 0–8; 8 active) ───────────────────
  │
  ├── Epic 2 (Runtime Intelligence, aichat) ─ Phases 9-11 DONE; caching 37→41 in flight/next
  │     │
  │     ├── Epic 3 (Composition UX, aichat) ─── Phases 12, 13 ──── DONE
  │     │     └── Epic 4 (Typed Ports, aichat) ─── Phases 14, 15, 33 ──── DONE
  │     │           ├── Epic 5 (Server Engine, aichat) ─── 16, 17 DONE; 18 DEFERRED
  │     │           │     └── Epic 6 (Universal Addressing) ─ 19, 20 ── DONE
  │     │           │           └── Epic 7 (DAG Execution) ─ 21, 22, 36 ─ DONE
  │     │           └── Epic 8 (Feedback Loop) ─ 23 DONE; 24 PLANNED (indep.)
  │     └── Epic 9 (Knowledge Evolution, aichat) ─── Phases 25-27 ──── DONE
  │
  ├── Epic 15 (Observability Keystone, aichat) ─ 42 → 43 → 44 ─ PLANNED ★ new gate
  │
  ├── Epic 16 (Astrophage Substrate, cross-repo) ─ 45 → 46 → 47 → 48 ─ PLANNED ★ new
  │
  ├── Epic 17 (Federation & Scale, aichat/cross-repo) ─ 50, 51 ─ PLANNED ★ new
  │
  ├── Epic 10 (Entity Evolution, aichat ↔ llm-functions) ─ 52 → 28 → 29 → 49 ─ PLANNED ★ 52 = Entity foundation
  ├── Epic 11 (Bridge Retirement, cross-repo) ─ Phase 31 ─ DONE
  ├── Epic 12 (Developer Experience, aichat) ─ 30 DONE; 54 PLANNED (CLI UX hardening, indep.; 54F Ask-First)
  ├── Epic 13 (Pi as REPL Surface, aichat ↔ harness) ─── Phase 32 ──── DONE
  └── Epic 14 (Memory Surface, aichat ↔ harness) ─ 34 DONE; 35 PLANNED
```

**Critical path, shipped through Phase 36** (Phase 11D → 13 → 15B/C → 22 → 33 → 36, all Done).
The **active** critical path for the coming year is the new **trace-keystone gate**.

## The trace keystone is the new gate (Epic 15)

```
Phase 42 (SPEC-001 trace emission)  ──┬─→ Phase 43 (test harness, SPEC-002)
   schema + async writer + blobs +    │
   redaction + cache.lookup slot      ├─→ Phase 44 (OTel projection + training extraction)
                                      │       └─ 44C contamination guard reads cache_hit / cache.lookup
                                      ├─→ Phase 45D (astrophage cache.lookup correlation)
                                      └─→ Phase 46C (aichat deterministic tool-replay from trace blobs)
```

42 is upstream of almost everything new — it is therefore pulled into **Now**. It supersedes the
ad-hoc trace of Phase 8F/8G.

## Caching → astrophage (Epics 2 → 16)

```
Phase 37 (L1/L3, accounting, trace, pi)   37A → 37B → 37C → 37D → 37E   (37F deferred)
   └─ 37A (CallMetrics) + 37E (trace) ─→ Phase 38 (CacheBackend trait + control protocol)
                                            ├─ Phase 39 (remote backends, cargo-gated)   ← 38A
                                            ├─ Phase 40 (embedding/rerank caching)        ← 38A, 38E
                                            ├─ Phase 41 (admin & observability)           ← 38A, 37D
                                            └─ 38A (CacheBackend trait) ─→ Phase 45C (astrophage Remote backend)

Phase 45 (astrophage MVP: replay-core + cache gateway)   ← needs 38A + 42
   └─→ Phase 46 (cassette / eval-replay; resolves SPEC-astrophage §9.2 tool-replay key)   ← needs 45 + 42
   └─→ Phase 47 (mock / fault injection)                                                   ← needs 45
   └─→ Phase 48 (brief companion: cassette bindings — built in brief, optional)            ← needs 46
```

38A's trait is the single hard gate for the in-aichat caching tail **and** for astrophage's
`CacheBackend::Remote` variant: in-process vs separate-process becomes one trait, one deployment
choice. The in-aichat caches (37–41, structure-aware `(role,model,input)` + L3 `cache_control`)
and astrophage (wire-level, canonicalized-request key) **never share a key** — see
[`SPEC-astrophage §0/§3`](../architecture/integrated-architecture/SPEC-astrophage.md).

**Build coupling (not runtime).** aichat build-depends on astrophage's `replay-core` crate by a
cross-repo git dep (decision A, SPEC-astrophage §2.1): the arrow points **aichat → astrophage** at
build time, while `base_url` stays the only *runtime* coupling. Removing astrophage leaves aichat
fully functional.

## Federation (Epics 10 → 14 → 17)

```
Phase 52 (Entity model formalization: RoleLike→Entity trait + facet taxonomy)
   └─ foundation for ─→ 28 (composability) · 29 (dynamism) · 49 (federation)   ← 52D needs 42
Phase 29B (agent memory, JSONL) ─→ Phase 49 (agent memory federation)   ← needs 35 + 42
Phase 35 (knowledge-MCP) ─┬─→ Phase 49 (federated agent memory over MCP)
                          └─→ Phase 50 (knowledge-as-cassette / federated KB)   ← needs 46 drift pattern
Phase 51 (vendor model extensions) ── independent (no upstream)
```

## Parallel tracks (active, coming year)

- **Observability Keystone** (Epic 15): 42 **Now** → 43, 44 **Next**. The new gate.
- **Caching** (Epic 2): 37→38 **Now**; 39, 40, 41 **Next**; 37F deferred.
- **Astrophage Substrate** (Epic 16): 45→46→47→48 **Next**; cross-repo, blocked by 38A + 42.
- **Entity Evolution** (Epic 10): 52 (Entity formalization, foundation) → 28→29→49 **Next**; cross-repo with llm-functions.
- **Memory Surface** (Epic 14): 35 **Now**. **Feedback Loop** (Epic 8): 24 **Now**.
- **Federation & Scale** (Epic 17): 50, 51 **Later**.

**Deferred:** Phase 18 (server discovery / estimation); Phase 37F (semantic cache).
