# Roadmap Refresh — Notes

**Branch:** `roadmap-refresh-tri-repo` · **Scope:** documentation only · **Date of refresh:** 2026-06-02

This file records the roadmap refresh: the file/timestamp inventory it was based on, the
proposed new structure, what was changed, what was archived (and why), what was deliberately
left untouched, and open questions for the human reviewer.

> **Status of this document:** Sections 1–3 are the *proposal* written before editing.
> Sections 4–7 are the *post-edit summary*. If you are reviewing the diff, read §3 (proposed
> structure) first, then §4 (what changed).

---

## 1. Inventory + timestamp map

All working-tree mtimes are identical (`Jun 2 14:45`, the worktree checkout time), so they
carry no freshness signal. Authority is therefore decided by **last committed timestamp**
(`git log -1 --format=%cI`). Newer = more authoritative when two docs conflict.

### 1.1 Current-scheme phase overviews (the live roadmap — keep)

| File | Last commit | Phase status per ROADMAP.md |
|---|---|---|
| `phase-37-overview.md` / `phase-37-response-caching.md` | 2026-06-01 | 37 Planned (active build focus) |
| `phase-38..41-overview.md` | 2026-06-01 | 38–41 Planned (caching sub-track) |
| `phase-36-overview.md` / `phase-36-implementation-plan.md` | 2026-06-01 | 36 Done |
| `phase-34-overview.md` / `phase-34-auto-memory.md` | 2026-05-30 / 05-26 | 34 Done |
| `phase-33-overview.md` | 2026-05-30 | 33 Done |
| `phase-23-overview.md` | 2026-05-30 | 23 Done |
| `phase-22-overview.md` | 2026-05-29 | 22 Done |
| `phase-16-overview.md` / `phase-16-server-hardening.md` | 2026-05-29 | 16 Done |
| `phase-15-overview.md` | 2026-05-29 | 15 Done |
| `phase-13-overview.md` | 2026-05-29 | 13 Done |
| `phase-11-overview.md` / `phase-11-context-budget.md` | 2026-05-29 / 05-03 | 11 Done |
| `phase-35-overview.md` / `phase-35-knowledge-mcp.md` | 2026-05-26 | 35 Planned |
| `phase-9/10/12/14/17/18/19/20/21/24/25/26/27/28/29/30/31-overview.md` | 2026-04→05 | per ROADMAP table |
| `phase-9-schema-fidelity.md`, `phase-10-resilience.md`, `phase-17-server-execution.md`, `phase-18-server-discovery.md`, `phase-28-agent-composability.md`, `phase-29-agent-dynamism.md`, `phase-31-bridge-retirement.md` | 2026-04→05 | companion design detail — keep |

### 1.2 Meta / index docs (refresh in place)

| File | Last commit | Verdict |
|---|---|---|
| `../ROADMAP.md` | (tree) | Authoritative index — **refresh**: add themes, Now/Next/Later horizons, owning-repo tags, tri-repo framing |
| `dependencies.md` | 2026-06-01 | **Stale graph**: shows "Phase 13 planned / 15 partial / 22 planned" — all now Done; omits Phases 33–36, Memory 34–35, tri-repo. Refresh. |
| `success-metrics.md` | 2026-05-11 | Mildly stale: phrases shipped phases (20, 21, 22) as future targets. Refresh status column. |
| `anti-roadmap.md` | 2026-06-01 | Mostly evergreen. Light touch: tag the "forked-out tooling" row as astrophage (peer repo). |

### 1.3 Superseded / legacy (archive candidates)

| File | Last commit | Why superseded |
|---|---|---|
| `initial-phased-roadmap.md` | 2026-03-11 | The original 2026-03-10 flat plan (old numbering, pre-renumber). Fully replaced by `ROADMAP.md` + per-phase docs. Historical. |
| `phase-0-prerequisites.md` | 2026-03-30 | Epic 1 foundation, **Done**; summarized in `roadmap/archive/completed-epics.md`. |
| `phase-1-token-efficiency.md` | 2026-03-30 | Epic 1 foundation, **Done**. |
| `phase-2-pipeline-output.md` | 2026-03-30 | Epic 1 foundation, **Done**. |
| `phase-3-mcp-consumption.md` | 2026-03-11 | Epic 1 foundation, **Done**. |
| `phase-4-error-handling.md` | 2026-03-30 | Epic 1 foundation, **Done**. |
| `phase-5-remote-mcp.md` | 2026-03-30 | Epic 1 foundation, **Done**. |
| `phase-6-metadata-framework.md` | 2026-03-30 | Epic 1 foundation, **Done**. |
| `phase-7-error-messages.md` | 2026-03-30 | Epic 1 foundation, **Done**. |
| `phase-31.md` | 2026-05-03 | 3-line redirect stub; superseded by `phase-31-overview.md` + `phase-31-bridge-retirement.md`. |

**Explicitly NOT archived:** `phase-8-data-observability.md` — Epic-1-numbered but treated as
**active in-progress work** owned by another agent in the main worktree (see §5).

### 1.4 Structural drift noted (not all fixable in a docs-only pass)

- **integrated-architecture moved.** Git history shows `docs/roadmap/integrated-architecture/`
  (May 1–3 commits); current tree has it at `docs/architecture/integrated-architecture/`.
  `CLAUDE.md` still points readers to the old `docs/roadmap/integrated-architecture/README.md`
  path (stale). See open questions (§6).

---

## 2. Tri-repo model (framing the refresh)

The integrated system spans three repositories. Every roadmap item now carries an
**owning-repo tag** so readers can see *where the work lands*:

| Tag | Repo | Role |
|---|---|---|
| **`aichat`** | this repo | CLI / runtime / MCP server-and-client. Owns inference, roles, agents, RAG, MCP, macros, caching, server. |
| **`llm-functions`** | [jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions) | Tool + agent declarations consumed by aichat. |
| **`harness (pi)`** | [earendil-works/pi](https://github.com/earendil-works/pi) | The REPL/harness surface other clients consume aichat through. |
| **`cross-repo`** | [`docs/architecture/integrated-architecture/`](../architecture/integrated-architecture/) | Requirements that only make sense across ≥2 repos. Adjacent peer repos **astrophage** (record/replay/cache substrate) and **brief** are referenced here. |

Most phases are `aichat`-internal. The cross-repo seams are: Phase 31 (bridge retirement,
aichat ↔ llm-functions ↔ harness), Phase 32 (pi REPL cutover, aichat ↔ harness), Phase 35
(knowledge-MCP protocol, cross-repo), Memory Surface 34/35 (pi-extension reader), and the
caching sub-track's 37D pi integration.

---

## 3. Proposed structure for `docs/ROADMAP.md`

Reframe from a flat phase table into a PM-standard **outcome → theme → horizon** model,
keeping the existing per-phase detail table as the canonical status ledger.

1. **Vision** (keep, tighten) — "make for AI workflows."
2. **Strategy pillars** (was "Governing Constraints") — cost-conscious, one-tool-per-job,
   no-UI/no-breaking-argc.
3. **Now / Next / Later horizons** *(new)* — the product view:
   - **Now (in flight):** Caching sub-track Phase 37 (the architecture.svg "active build
     focus"); Memory Surface Phase 35; Epic 8 Phase 24 (regression & distillation).
   - **Next (committed, unstarted):** Caching 38 → 39 → 40 → 41; Epic 10 Phases 28–29
     (agent evolution).
   - **Later / Deferred:** Phase 18 (server discovery/estimation, deferred).
   - **Shipped (this cycle):** Phases 13, 15, 16, 22, 23, 33, 34, 36.
4. **Themes (epics) with owning-repo tags** *(new tagging)* — the 14 epics, each tagged by
   owning repo, grouped under the four strategic outcomes they serve.
5. **Status ledger** (keep the existing one-row-per-phase table, corrected + repo-tagged).
6. **References** (keep; fix the cross-repo link to the moved integrated-architecture dir).

Also add a thin **`docs/roadmap/README.md`** as the directory's navigational index (maps every
file to its status and points at the authoritative `ROADMAP.md`), since the dir has 50+ files
and no entry point.

Archive location: all archived roadmap material lives under **`docs/roadmap/archive/`** — a
single archive dir colocated with the roadmap it serves. The pre-existing `completed-epics.md`
(formerly at `docs/archive/roadmap/`) was moved here too, retiring the split `docs/archive/`
tree entirely. Rationale in §4.

---

## 4. What changed (post-edit summary)

### 4.1 `docs/ROADMAP.md` — reframed (PM structure)

- Added **The three repositories** section (repo tags + links to llm-functions and pi).
- Added **Horizons (Now / Next / Later)** — the product view, decoupled from epic numbering:
  - **Now:** Phase 37 caching, Phase 35 knowledge-MCP, Phase 24 regression/distillation.
  - **Next:** caching 38–41, Entity Evolution 28–29.
  - **Later:** Phase 18 (deferred).
  - **Shipped this cycle:** 13, 15, 16, 22, 23, 33, 34, 36.
- Added **Themes → epics** roll-up (4 strategic outcomes, repo-tagged).
- Added an **Owner** column to the status ledger; every phase now carries its owning repo.
- Replaced the dense, timestamp-heavy "Active Track" prose with a cleaner **sequencing detail**
  section (same facts, less noise).
- Added a **Phase 8 note** in the ledger flagging it as active (not archived).
- Renamed "Governing Constraints" → "Strategy pillars" and added the local-vs-frontier pillar
  from `CLAUDE.md`.
- Added References entries for the new roadmap-dir index and these notes.

### 4.2 New file: `docs/roadmap/README.md`

Directory navigational index. Maps every phase doc (overview + companion detail) to its
location, points at `../ROADMAP.md` as authoritative, and carries a tombstone table for the
archived docs.

### 4.3 Meta-docs refreshed

- **`dependencies.md`** — graph statuses corrected (13/15/22 were shown "planned/partial" but are
  **Done**); added Phases 33–36 and the Memory Surface (34–35) track; added repo tags to every
  epic; updated the critical path to "shipped through Phase 36"; clarified 37D/37E touch the
  harness.
- **`success-metrics.md`** — marked shipped targets **achieved** (Phase 11D, 22, 23, 36); added a
  cache-hit cost-savings metric for the Phase 37 sub-track; removed a now-duplicated row.
- **`anti-roadmap.md`** — named the "forked-out tooling" row as the **astrophage** peer repo with
  a link to its cross-repo spec.

### 4.4 Archived (moved to `docs/roadmap/archive/`, nothing deleted)

`initial-phased-roadmap.md`, `phase-0-prerequisites.md` … `phase-7-error-messages.md`,
`phase-31.md`. Reasons in §1.3. Link fixes:
- `completed-epics.md` detail links rewritten `./roadmap/phase-N.md` → `./phase-N.md` (they were
  already broken; the moves *repair* them since the targets are now siblings).
- The Phase 8 link in `completed-epics.md` points to `../phase-8-data-observability.md`
  (phase-8 stays in the live roadmap) and is tagged *active*.
- The Phase 7.5 dead link (detail doc was never committed) replaced with an inline note.
- The archived `phase-31.md` stub's links repointed at the live docs under `../`.

> **Archive location decision.** The task asked for `docs/roadmap/archive/`. An earlier draft of
> this refresh staged the foundation docs under a separate `docs/archive/roadmap/` (where
> `completed-epics.md` historically lived). That split was reversed: everything — including
> `completed-epics.md` — now lives under the single `docs/roadmap/archive/`, and the old
> `docs/archive/` tree was removed. Inbound links from `ROADMAP.md`, `README.md`, and
> `completed-epics.md` were updated accordingly.

## 5. What was left alone

- **Phase 8** (`phase-8-data-observability.md`) — per the hard constraint, treated as **active
  in-progress work** owned by another agent in the main worktree. Not moved, not rewritten; only
  the *inbound link* from the archived `completed-epics.md` was repointed and tagged active.
- All **current per-phase overview + companion design docs** (Phases 9–41) — content untouched;
  they remain the canonical design record. Only the index/ledger framing around them changed.
- **Source code, Cargo files** — untouched (docs-only task).
- **`docs/architecture/*`, `integrated-architecture/*`** — content untouched. (`CLAUDE.md` got a
  single stale-path fix in the "Integrated requirements" section — see §6.1.)
- Internal links *inside* the archived foundation docs (e.g. `../analysis/...`) were not chased;
  those are frozen historical artifacts.

## 6. Open questions for the human

1. **Stale path in `CLAUDE.md`.** ✅ RESOLVED — §"Integrated requirements" pointed at
   `docs/integrated-architecture/` and `docs/roadmap/integrated-architecture/README.md`; both
   repointed to the real `docs/architecture/integrated-architecture/`. (Line 15 was already
   correct.)
2. **Archive dir name.** ✅ RESOLVED — all archived roadmap material consolidated under
   `docs/roadmap/archive/` (§4.4); the split `docs/archive/` tree was removed and all inbound
   links updated.
3. **Phase numbering gap.** ✅ RESOLVED — no stub added. The new `docs/roadmap/README.md`
   already cross-refs Phase 32 (Pi as REPL Surface) to `../features/repl-pi.md`, closing the
   navigational gap without a redundant placeholder doc.
4. **Phase 7.5 detail doc** was never committed; `completed-epics.md` referenced a missing file.
   I replaced the dead link with a note — confirm the summary there is canonical.

## 7. Verification

- `grep` for every archived filename across `docs/` — all remaining references resolve to
  siblings within `docs/roadmap/archive/` or are intentional tombstones in the new index/notes.
- `phase-8-data-observability.md` confirmed still present in `docs/roadmap/`.
- No `src/`, Cargo, or Phase 8 doc content modified.
- No timestamps/random values introduced into any demo-style output (none added).
- Nothing committed or pushed; all changes left in the worktree for review.
