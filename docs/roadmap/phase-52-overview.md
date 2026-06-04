# Phase 52 — Entity Model Formalization : Overview — Epic 10 (Entity Evolution)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** aichat · **Horizon:** Next (Epic 10 foundation)

> **Goal.** Make the **Entity** the named, foundational building block it already implicitly is —
> formalizing the latent `RoleLike` trait into an `Entity` trait with **capability (facet)
> introspection**, so that Prompt / Role / Agent / Macro are understood and handled as *presets*
> over one substrate. This is a **rename + widening, not a struct merge**: it clarifies and unifies
> dispatch without touching the file-vs-directory authoring contracts. It is the conceptual
> foundation the rest of Epic 10 (28 → 29 → 49) builds on. Full design:
> [`architecture/entity-model.md`](../architecture/entity-model.md).

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 52A | **`RoleLike` → `Entity` trait** — rename + a `facets()` capability-introspection method (which families are present, owned vs referenced). No behavior change; `to_role()` stays the bridge. | Planned (aichat) |
| 52B | **Facet taxonomy in code + docs** — the closed family set (Know · Act · Shape · Govern · Compose · Judge); document the couplings (§7 of the design); surface facets in `--dry-run`. | Planned (aichat) |
| 52C | **Backing-aware uniform resolution** — collapse the variant-specific `SessionEntity` / `EntityRef` branches (`pipe.rs:517`, `config/mod.rs:1030`) onto the `Entity` trait; the **backing-gates-ownership** rule is the single resolution invariant. | Planned (aichat) |
| 52D | **Trace entity attribution** — each invocation's keystone trace (Phase 42) carries `entity_id` + the *resolved facet set actually used*; the stable key Phase 49 attribution reads. | Planned (aichat) |

## Cross-repo seams

- **No struct merge, no new authoring format.** Roles stay files in **aichat**; agents stay
  directories in **llm-functions**. The Entity trait is the in-process spine; the two authoring
  contracts remain independent and cross-repo (see [`anti-roadmap.md`](anti-roadmap.md)).
- **Pairs with the trace keystone (Phase 42).** Entity = the *authoring* keystone; the trace = the
  *runtime* keystone. 52D is where they meet (`resolve Entity → execute → emit Trace`).

## Dependencies

- **Upstream:** none hard — 52A–C are clarifying refactors of existing code. 52D needs Phase 42
  (trace emission).
- **Feeds:** Phases **28 / 29 / 49** (their new capabilities are *facets* on the formalized
  Entity); MCP capability negotiation; uniform `--dry-run` introspection.
- **Builds on:** [`archive/phase-14-overview.md`](archive/phase-14-overview.md) (capability
  manifests) · [`archive/phase-19-overview.md`](archive/phase-19-overview.md) (RoleResolver).

## Acceptance criteria

1. The runtime dispatches via the **`Entity` trait**; no caller branches on the concrete
   Role/Agent/Prompt/Macro variant for resolution.
2. `facets()` reports the correct facet families for each preset, distinguishing **owned vs
   referenced**, consistent with the [field-mapping appendix](../architecture/entity-model.md#11-field-mapping-appendix).
3. `--dry-run` lists an entity's facets; the **backing-gates-ownership** rule is enforced at
   resolution (a file-role cannot *own* a stateful/executable facet).
4. A traced invocation carries `entity_id` + resolved facet set (gated on Phase 42).
5. **No change** to role-file or agent-directory authoring formats; `to_role()` semantics
   preserved.

## Grounding docs

[`architecture/entity-model.md`](../architecture/entity-model.md) (the design) ·
[`architecture/architecture.md`](../architecture/architecture.md#entity-types) (Entity Types) ·
[`../analysis/epic-10.md`](../analysis/epic-10.md) ·
[`phase-42-overview.md`](phase-42-overview.md) (trace keystone) ·
[`phase-28-overview.md`](phase-28-overview.md) · [`phase-29-overview.md`](phase-29-overview.md)
</content>
