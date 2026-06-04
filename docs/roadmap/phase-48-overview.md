# Phase 48 — brief Companion: Cassette Bindings : Overview — Epic 16 (Astrophage Substrate)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** brief repo (cross-repo seam) · **Horizon:** Next (optional/deferred)

> **Goal.** Let an intent author **bind a role to a cassette set next to the role's intent** in
> `.brief.md`, with **brief never running anything**. brief grows a *format field*; astrophage
> (invoked by the eval harness, with aichat as the client) is the *runtime consumer*. This is the
> **F6 pattern**: brief emits a string, astrophage does the work, and the two never share a
> process.
>
> **Documented here, built in brief.** Per the cross-repo rule, this phase is **specced in the
> aichat repo and implemented in [brief](https://github.com/jikanter/brief)** — the aichat repo
> never edits brief. The committed eval-replay story (Phase 46) **works without it**; the field
> is a convenience for teams already authoring intent in `.brief.md`.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 48A | brief **`## Fixtures` section + `cassettes:` frontmatter** field + parser (brief-repo task, spec'd here) | Planned (brief repo) |
| 48B | **`brief emit`** → promptfoo provider-config snippet selecting astrophage replay (`--cache-mode cassette --cassette … --cache-replay`) | Planned (brief repo) |
| 48C | **Round-trip demo** — brief-bound role → committed cassette → CI replay; drift on prompt edit (demoable from the aichat side) | Planned (cross) |

## Cross-repo seams

- brief stays **format-first, synchronous, no `tokio`, no `reqwest`** — it declares and emits, it
  does not execute (brief `CLAUDE.md` decisions 2–4). The field is **extensible frontmatter**;
  the runtime work is entirely astrophage + aichat.
- **Honesty flag** ([`SPEC-astrophage §4`](../architecture/integrated-architecture/SPEC-astrophage.md)):
  a cassette can always be named directly in `promptfooconfig.yaml`. The brief field is optional
  sugar — if it is never built, brief stays untouched and the direct-promptfoo path still works.

## Dependencies

- **Upstream:** Phase 46 (cassette policy). Realizes [`SPEC-astrophage §4`](../architecture/integrated-architecture/SPEC-astrophage.md) + [`SPEC-004 §brief`](../analysis/caching/SPEC-004-ecosystem-surfaces.md).
- **Optional/deferred** per the §4 honesty flag.

## Acceptance criteria

1. A `cassettes:`-bound `.brief.md` emits a promptfoo provider config selecting astrophage replay.
2. The emitted config drives a **token-free CI replay** against a committed cassette set.
3. **brief gains zero runtime/network code** — the seam is format-only, by construction.

## Grounding docs

[`SPEC-astrophage.md`](../architecture/integrated-architecture/SPEC-astrophage.md) (§4) ·
[`SPEC-004`](../analysis/caching/SPEC-004-ecosystem-surfaces.md) (§brief) ·
[brief repo](https://github.com/jikanter/brief) (companion change lives there, not here)
