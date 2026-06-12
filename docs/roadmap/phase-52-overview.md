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
| 52A | **`RoleLike` → `Entity` trait** — rename + a `facets()` capability-introspection method (which families are present, owned vs referenced). No behavior change; `to_role()` stays the bridge. | **Done** (aichat) |
| 52B | **Facet taxonomy in code + docs** — the closed family set (Know · Act · Shape · Govern · Compose · Judge); document the couplings (§7 of the design); surface facets in `--dry-run`. | **Done** (aichat) |
| 52C | **Backing-aware uniform resolution** — collapse the variant-specific `SessionEntity` / `EntityRef` branches (`pipe.rs:517`, `config/mod.rs:1030`) onto the `Entity` trait; the **backing-gates-ownership** rule is the single resolution invariant. | Planned (aichat) |
| 52D | **Trace entity attribution** — each invocation's keystone trace (Phase 42) carries `entity_id` + the *resolved facet set actually used*; the stable key Phase 49 attribution reads. | **Done** (aichat) |

### 52A implementation note (shipped)

- The `RoleLike` trait is renamed to `Entity` (`src/config/role.rs`); all call
  sites (`pipe.rs`, `main.rs`, `config/mod.rs`, `config/preflight.rs`,
  `repl/mod.rs`) follow. `to_role()` is untouched — still the bridge.
- New introspection surface: `Facet` (the six closed families), `FacetOwnership`
  (`Owned` / `Referenced`), and `FacetSet`, with `Entity::facets() -> FacetSet`.
- `facets()` is implemented for `Role`, `Agent`, and `Session` by **field
  presence**, applying the §5.2 backing-gates-ownership rule: a file-role only
  *references* `Act`/`Know`; a directory-agent *owns* its tools / RAG / dynamic
  instructions while still *referencing* MCP servers. `Session` delegates to the
  role it synthesizes via `to_role()`.
- **No behavior change**: nothing in production calls `facets()` yet — it is the
  foundation 52B (`--dry-run` listing), 52C (resolution), and 52D (trace)
  consume. The new API carries `#[allow(dead_code)]` until then. No CLI surface
  changed, so there is no showboat demo for 52A.

### 52B implementation note (shipped)

- `FacetSet::summary()` (`src/config/role.rs`) renders the facet families in
  closed-taxonomy order as a compact line — e.g. `Act(ref), Shape(owned)` — a
  family present under both ownerships collapsing to `(owned, ref)`. Empty when
  no facets are present.
- `emit_dry_run_preview` (`src/main.rs`) emits a `  facets: …` line after
  `capabilities:`, **omitted entirely** when the entity carries none, so the
  preview stays clean for bare prompts/roles. The line sits before the pipeline
  tree on stderr — stdout still carries only the assembled prompt.
- The six-family closed taxonomy and its advisory couplings are already in the
  design (`entity-model.md` §4 / §7); §7's "surfaced in docs and `--dry-run`"
  claim is now load-bearing. No authoring-format change.
- Coverage: `facetset_summary_*` unit tests (rendering, dual-ownership collapse,
  empty) + `tests/regression/phase-52b.sh` (the `--dry-run` line present for a
  mixed entity, omitted for a bare one).

### 52D implementation note (shipped)

- `FacetSet::trace_tokens() -> Vec<String>` (`src/config/role.rs`) is the
  machine-readable sibling of 52B's `summary()`: each `(family, ownership)` pair
  becomes one `Family:ownership` token (`owned` / `referenced`), in the `entries`
  BTreeSet's stable order. **Dual ownership is not collapsed** — a family present
  both ways yields two tokens — so the ownership bit survives as a downstream
  stratification key. This is the GROUP BY key Phase 49 attribution reads, chosen
  over a display string precisely so consumers group/diff/hash without re-parsing.
- The keystone `session.start` payload (`event.rs` `SessionStart`, threaded via
  `session.rs` `StartInfo`) gains two **additive optional** fields: `entity_id`
  (the resolved entity's stable id, distinct from the human `role` label) and
  `facets` (the `trace_tokens` set). Per SPEC-001 §5 these do **not** bump
  `schema_version` (stays `"0.1"`); consumers tolerate `entity_id: null` /
  `facets: []`.
- Wired in `call_react` (`src/client/common.rs`) from `input.role().name()` +
  `input.role().facets().trace_tokens()`. **Known limitation:** at the keystone
  the resolved entity is the synthesized `Role` (`to_role()`), so for an agent
  the facets reflect the resolved role, not the agent directory's *owned*
  facets. That mislabel is deliberately **visible** (the ownership bit is kept)
  and is closed by 52C's uniform resolution — 52D does not block on it.
- Coverage: `facetset_trace_tokens_*` unit tests (sorted pairs, dual-ownership
  kept distinct, empty) + `session_start_carries_entity_attribution` /
  `session_start_omits_entity_id_when_absent_*` (`session.rs`) asserting the
  field appears on / defaults cleanly off the emitted `session.start` line.
- SPEC-001 §3.1 `session.start` block documents both fields + the additive-no-bump
  versioning rule.

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
