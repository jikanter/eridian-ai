# Roadmap Refresh — Notes (next-year + comprehensive archive)

**Branch:** `roadmap-next-year-2026-06` · **Scope:** documentation only · **Date:** 2026-06-04

This refresh (1) authors the **next year of phases (42–51)** organized around the four-repo
integrated architecture, (2) **physically archives** every shipped per-phase doc behind one
comprehensive ledger, and (3) condenses/refactors the remaining unimplemented items into a clean
Now/Next/Later forward view. It supersedes the 2026-06-02 tri-repo refresh (which framed the
horizons and tagged repos but did not add forward phases).

---

## 1. What was added — the next year (Epics 15–17, Phases 42–51)

Framed around the four-repo split (**aichat ↔ llm-functions ↔ brief ↔ astrophage**, + the pi
harness). A **fifth strategic outcome** was added: *"An ecosystem that is observable, replayable,
and evaluable."* Historical phase numbers were **not** renumbered (would break links/history);
new work continues from 42.

| New epic | Phases | Owner | Realizes |
|---|---|---|---|
| **15 Observability Keystone** | 42 trace emission · 43 test harness · 44 projections + training | aichat | `SPEC-001`, `SPEC-002`, `PLAN-trace-emission`, `PLAN-test-harness`, `ADR-0001` |
| **16 Astrophage Substrate** | 45 cache-policy gateway + `replay-core` · 46 cassette/eval-replay · 47 mock/fault · 48 brief companion | astrophage / aichat / brief | `SPEC-003`, `SPEC-004`, `ADR-0005`, `EVAL-0005`, `SPEC-astrophage` |
| **17 Federation & Scale** | 50 knowledge-as-cassette / federated KB · 51 vendor model extensions | aichat / cross-repo | `SPEC-mcp-json-artifact`, `2026-04-23-model-extensions` |
| **10 Entity Evolution** (extended) | 49 agent memory federation (new) | aichat ↔ llm-functions ↔ harness | builds on 29B + 35 + 42 |

Each new phase has a full house-style overview (`phase-42-overview.md` … `phase-51-overview.md`):
goal, owner + horizon, sub-phase table, cross-repo seams, dependencies, acceptance criteria, and
the grounding SPEC/ADR/PLAN it realizes. The **trace keystone (Phase 42)** is the new hard gate —
pulled into **Now** because astrophage correlation (45D), tool-replay (46C), the test harness
(43), and training extraction (44) all read the trace.

The next-year plan **resolves** one standing `STOP-AND-ASK`: tool-replay key stability
(`SPEC-astrophage §9.2`) is owned by Phase 46C.

## 2. What was archived (physical move + comprehensive index)

All **Done** per-phase docs moved `docs/roadmap/ → docs/roadmap/archive/` (via `git mv`); nothing
deleted. Moved: `phase-9-*`, `phase-10-*`, `phase-11-*`, `phase-12/13/14/15-overview`,
`phase-16-*`, `phase-17-*`, `phase-19/20/21/22/23-overview`, `phase-25/26/27-knowledge-*`,
`phase-30-macro-compilation`, `phase-31-*`, `phase-33-overview`, `phase-34-*`, `phase-36-*`.

[`archive/completed-epics.md`](archive/completed-epics.md) was expanded from an Epic-1-only record
into the **comprehensive ledger of every shipped phase** (Epics 1–14), one row per phase with
sub-phase end-state and a link to the archived design doc.

**Link integrity.** Two classes of links were repaired:
- **Inbound** (live docs → moved docs): `features/`, `demos/`, `analysis/`, and the live roadmap
  docs (35, 37) now point into `archive/`.
- **Internal** (inside the moved docs): physically moving down one level broke every `../` link
  (to `features/`, `analysis/`, `src/`, `ROADMAP.md`, …). All were re-calibrated **+1 level**;
  sibling links to still-live phases got `../`, and live meta-doc links got `../`. A full
  broken-link scan confirms none of the 30 moved docs is a source of a broken link.

The older foundation docs (`phase-0-7`, `initial-phased-roadmap.md`) had pre-existing broken
`../analysis/…` links (left frozen by the 2026-06-02 refresh); since their targets still exist,
those were recalibrated `+1` too so the **entire archive is link-clean**. The `phase-31.md`
tombstone stub's two links were repointed to its now-sibling archived docs. A full scan confirms
**zero broken links sourced anywhere under `docs/roadmap/`** (the 19 remaining in `docs/` are
unrelated pre-existing issues outside the roadmap tree: the `2026-03-16-simple-planning.md`
double-path bug and demo-fixture placeholders).

## 3. What was refactored (unimplemented items)

- **Thin stubs normalized.** `phase-28-overview.md` and `phase-29-overview.md` (≈400-byte stubs)
  rewritten to house style (owner, horizon, cross-repo seams), keeping their companion-doc links.
- **Astrophage-boundary note** added to the in-aichat caching overviews (37–41): a consistent
  admonition clarifying *structure-aware in-process cache* (37–41) vs *wire-level runtime-agnostic
  cache* (astrophage, 45–47), and that the two never share a key.

## 4. Meta-docs rewritten

- **`../ROADMAP.md`** — five-repo intro, fifth strategic outcome, coming-year Now/Next/Later,
  Epics 15/16/17 in Themes → epics, corrected status ledger (Done → `Done · archived` with links;
  new 42–51 rows), trace-keystone sequencing detail, "Last updated 2026-06-04".
- **`README.md`** — file map split into Active / Planned-committed / Planned-frontier / Deferred,
  plus the archived table.
- **`dependencies.md`** — redrawn graph with Epics 15/16/17, the trace-keystone gate, the
  caching→astrophage chain (38A trait gate), the aichat→`replay-core` build-coupling arrow, and
  the federation chain.
- **`success-metrics.md`** — shipped targets marked achieved; next-year targets added (trace
  coverage, control-flow determinism, eval-replay byte-identity, astrophage savings, training-pair
  yield, tool-replay key stability, agent-memory federation, KB portability, local-model reach).
- **`anti-roadmap.md`** — added a **Cross-repo boundaries** section (no astrophage reverse-dep, no
  keys across the seam, brief stays runtime-free, no tool-execution caching, no parallel telemetry
  model, no editing peer repos from aichat).

## 5. What was left alone

- **Phase 8** (`phase-8-data-observability.md`) — active in the main worktree. Not moved; only
  inbound links repointed and tagged active.
- **Phase 18** — stays live as **Deferred** (Later horizon).
- **`docs/analysis/caching/*`, `architecture/integrated-architecture/*`** — content untouched;
  the new phases reference them as grounding.
- **Source code, Cargo files** — untouched (docs-only).

## 6. Open questions / residual

1. **Tool-replay key stability** (`SPEC-astrophage §9.2`) — now *owned* by Phase 46C, but the
   schema decision (is `(tool_name, args_hash)` a stable lookup key?) is made when 46 is built.
2. **Canonical-key field edges** (`SPEC-003 §2`) — any in-key/out-of-key field not settled by
   37C's `transparent_key` remains a stop-and-ask for Phase 45A.
3. **brief companion (Phase 48)** — documented here, built in the
   [brief repo](https://github.com/jikanter/brief); optional (the direct-promptfoo path works
   without it).
4. **Non-roadmap pre-existing broken links** (the `2026-03-16-simple-planning.md` double-`analysis/`
   path and demo-fixture placeholders) were left as-is — outside this refresh's scope.

## 7. Verification

- `git mv` used for all moves (history preserved). Broken-link scan: **zero broken links sourced
  anywhere under `docs/roadmap/`** (live + archive). The only broken links left in `docs/` are
  unrelated pre-existing issues outside the roadmap tree — **zero** introduced by the new phases
  or the archive move.
- `phase-8-data-observability.md` confirmed still in `docs/roadmap/`; no `src/`/Cargo changes.
- Every Done ledger row links to a file under `archive/`; every 42–51 row links to a new live
  overview; horizons reconcile with the ledger.
- Nothing committed/pushed beyond the working branch; left for review.
