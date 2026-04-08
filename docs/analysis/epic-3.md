# Epic 3: Composition UX

**Created:** 2026-04-07
**Status:** Planning
**Depends on:** Epic 2 (runtime intelligence infrastructure)
**Phases:** 12-13
**Source:** Theme 6 — UX Designer analysis

---

## Motivation

Apply token-consciousness to human attention, not just LLM calls. Every role invocation should make the user slightly more aware of what the system can do. Reduces the cost of *understanding* before the cost of *execution*.

---

## Phases

### Phase 12: Discoverability & Previews

| Item | Description |
|---|---|
| 12A | Resolved prompt preview (`--dry-run` with extends/include expanded, variables interpolated) |
| 12B | Pipeline visualization in `--dry-run` (text diagram: `extract -> validate -> summarize`) |
| 12C | Port signatures in `--list-roles --verbose` (input/output type summaries) |
| 12D | Composition summary after `.role <name>` in REPL |

### Phase 13: Authoring & Teaching

| Item | Description |
|---|---|
| 13A | `--fork-role <source> <new-name>` (creates pre-populated `extends:` file) |
| 13B | Schema mismatch errors as teaching moments (side-by-side diff with suggestion) |
| 13C | Built-in guardrail role examples (PII detection, prompt injection, topic restriction) |
| 13D | `--explain-role <name>` (human-readable description of role composition) |

---

## Key Designs

**12A/12B — Resolved Preview:** Zero-token "terraform plan" for roles. `--dry-run` shows resolved state (extends/include/variables expanded), pipeline diagram, assembled prompt with token count, and estimated cost. Files: `src/main.rs`, `src/config/role.rs`.

**13A — Fork Role:** `aichat --fork-role base-analyst my-analyst` creates a pre-populated `extends:` file with commented-out overridable fields. Files: `src/cli.rs`, `src/main.rs`.

**13B — Error Teaching:** Schema mismatch errors show side-by-side diffs of what stage N produced vs what stage N+1 expects, with actionable hints. Files: `src/config/role.rs`, `src/pipe.rs`.

**13C — Guardrail Examples:** 3 example roles in `assets/roles/` (PII, injection, topic) demonstrating the guardrail-in-pipeline pattern.

Full designs with YAML examples: [ROADMAP.md, Epic 3 section](../ROADMAP.md#epic-3-composition-ux-new)

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Visual pipeline designer GUI | Violates "no desktop UI" constraint. Roles are YAML files. |
| Full-blown package registry for roles | Premature. `--fork-role` + git + `extends` covers sharing. |
| Automated role recommendation | Requires LLM call for routing. Deterministic `--find-role` (Epic 4) is sufficient. |

---

## Relationship to Existing Roadmap

| Feature | Existing Phase | Relationship |
|---|---|---|
| 12A (resolved preview) | Phase 1A (`-o json` for `--info`) | **Extension** — 1A added machine output; 12A adds human-readable resolved view |
| 13A (fork-role) | None | **New** |
| 13B (error teaching) | Phase 4C (structured errors) | **Extension** — 4C structured the error; 13B makes it teach |
| 13C (guardrails) | None | **New** — pattern documentation, not new runtime feature |
