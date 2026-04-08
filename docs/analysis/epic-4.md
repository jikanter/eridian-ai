# Epic 4: Typed Ports & Capabilities

**Created:** 2026-04-07
**Status:** Planning
**Depends on:** Epic 3 Phase 12C (port signatures groundwork)
**Phases:** 14-15
**Source:** Theme 1 — convergence across all four expert analyses

---

## Motivation

Roles should declare what they *can do*, not just what they *are*. Type-based wiring instead of name-based wiring is what makes systems evolvable. This is the single highest-leverage abstraction change in the roadmap.

---

## Phases

### Phase 14: Capability Manifests

| Item | Description |
|---|---|
| 14A | `capabilities:` field on roles (semantic intent tags) |
| 14B | Human-readable port type annotations (derived from schema) |
| 14C | Local capability resolver (`config.find_roles_by_capability(...)`) |
| 14D | `--find-role` CLI flag (search by capability, input/output type) |

### Phase 15: Contract Testing

| Item | Description |
|---|---|
| 15A | Pipeline schema compatibility check at authoring time (`showboat validate-pipeline`) |
| 15B | Cross-stage schema containment validation (output N satisfies input N+1) |
| 15C | `--check` flag for validating role/pipeline definitions without execution |

---

## Key Designs

**14A — Capabilities Field:**

```yaml
capabilities: [code-review, security-audit, rust, python]
```

Free-form string tags enabling discovery without formal ontology. Mirrors MCP Server Cards' approach.

**14C/14D — Capability Resolver + CLI:**

```bash
$ aichat --find-role --capability code-review
  code-reviewer    in: {code, language}  out: {issues, severity}
$ aichat --find-role --accepts json --produces json
  classifier       in: json{text}        out: json{label, confidence}
```

Files: `src/config/role.rs` (capabilities, port_accepts, port_produces), `src/config/mod.rs` (resolver), `src/cli.rs` (--find-role).

**15A/15B — Pipeline Schema Validation:**

JSON Schema containment check: verify output of stage N satisfies input of stage N+1. Deterministic, zero LLM cost. Files: `src/config/preflight.rs`.

Full designs with YAML examples: [ROADMAP.md, Epic 4 section](../ROADMAP.md#epic-4-typed-ports--capabilities-new)

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Formal type system or ontology | Over-engineering. Free-form tags + schema matching covers the need. |
| Automatic pipeline wiring from types | Ambiguous without user intent. `--find-role` suggests; user composes. |

---

## Relationship to Existing Roadmap

| Feature | Existing Phase | Relationship |
|---|---|---|
| 14A (capabilities) | Phase 1B (role description) | **Extension** — description is prose; capabilities are structured tags |
| 15A (pipeline validation) | Phase 4E (pipeline tracebacks) | **Extension** — 4E reports failures; 15A prevents them at authoring time |
| 15C (--check flag) | Phase 7B (pre-flight checks) | **Extension** — 7B checks tool binaries; 15C checks schema compatibility |
