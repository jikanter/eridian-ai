# Phase 14: Capability Manifests : Overview - Epic 4

**Status (2026-05-04):** Shipped. 7 new role unit tests + 10 regression tests (`tests/regression/phase-14-12.sh`).

| Item | Description | Status |
|---|---|---|
| 14A | `capabilities:` field on roles (semantic intent tags) | **Done** |
| 14B | Human-readable port type annotations (derived from schema) | **Done** |
| 14C | Local capability resolver (`Config::find_roles_by_capability` / `find_roles_by_port`) | **Done** |
| 14D | `--find-role` CLI flag (search by capability, input/output type) | **Done** |

**Files touched:**
- `src/config/role.rs` — `capabilities` field + getter; `port_input_summary`,
  `port_output_summary`, `port_accepts`, `port_produces`; helper renderers
  `port_summary_from_schema*` and `port_signature_matches`. `extends()` /
  `include()` getters added for the Phase 12 preview.
- `src/config/mod.rs` — `Config::find_roles_by_capability` and
  `Config::find_roles_by_port` (sort by name; capabilities matching is
  case-insensitive substring; port matching reuses Phase 14B).
- `src/cli.rs` — `--find-role`, `--capability`, `--accepts`, `--produces`,
  `--verbose` flags.
- `src/main.rs` — `--find-role` dispatch, `render_role_list` helper shared
  with `--list-roles`, `--list-roles --capability` filter.

**Limitations carried forward (intentional):**
- `capabilities:` uses override semantics across `extends:` (child wins
  entirely), matching how `tags:` already inherits. A future change can
  switch to union semantics if user feedback demands it; the simpler
  override is consistent with the rest of the YAML schema today.
- `--find-role` always re-resolves the full role universe. Acceptable for
  the current corpus size; a cached index can come if `Config::all_roles`
  starts showing up in profiles.
- Port signatures only inspect the top-level `type` and `properties` of a
  schema. Nested shape detail is not surfaced; querying with `--accepts
  json{a,b,c}` works because the matcher does an exact-string compare on
  the human form.

**14A Design — Capabilities Field:**

```yaml
---
name: code-reviewer
description: Reviews code for bugs and security issues
capabilities: [code-review, security-audit, rust, python]
input_schema:
  type: object
  properties:
    code: { type: string }
    language: { type: string }
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string, enum: [low, medium, high, critical] }
---
```

Capabilities are free-form string tags. They enable discovery ("find me a role that can do code-review") without requiring formal ontology. This mirrors MCP Server Cards' approach to tool discovery.

**14C Design — Capability Resolver:**

```rust
// New method on Config
pub fn find_roles_by_capability(&self, capability: &str) -> Vec<&Role> {
    self.roles.iter()
        .filter(|r| r.capabilities().iter().any(|c| c.contains(capability)))
        .collect()
}

pub fn find_roles_by_port(&self, input_type: Option<&str>, output_type: Option<&str>) -> Vec<&Role> {
    self.roles.iter()
        .filter(|r| {
            let input_ok = input_type.map_or(true, |t| r.port_accepts(t));
            let output_ok = output_type.map_or(true, |t| r.port_produces(t));
            input_ok && output_ok
        })
        .collect()
}
```

**14D Design — Find Role CLI:**

```bash
$ aichat --find-role --capability code-review
  code-reviewer    in: {code, language}  out: {issues, severity}  capabilities: [code-review, security-audit]
  lint-checker     in: text              out: {errors}            capabilities: [code-review, linting]

$ aichat --find-role --accepts json --produces json
  classifier       in: json{text}        out: json{label, confidence}
  transformer      in: json{...}         out: json{...}
```

**Files:** `src/config/role.rs` (add `capabilities: Vec<String>`, `port_accepts()`, `port_produces()`), `src/config/mod.rs` (add resolver methods), `src/cli.rs` (add `--find-role` flag), `src/main.rs` (render results).
