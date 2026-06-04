# Phase 33: Typed Input Surface : Overview - Epic 4

**Status (2026-05-30):** **Done — 33A–E all shipped.** This phase unifies the fragmented input data space (`variables:`, `input_schema:`, `__INPUT__`, `-v`, stdin) into a single typed contract. Extends [Phase 14](phase-14-overview.md) (capability manifests) and [Phase 15](phase-15-overview.md) (contract testing); none of those phases need to land first, but 15B's cross-stage containment check becomes substantially more useful once 33B/33D ship.

| Item | Description | Status |
|---|---|---|
| 33A | `default:` (and `default: { shell: ... }`) per property inside `input_schema:` — schema becomes the source of truth for parameter declarations | **Done** |
| 33B | Type-aware `{{name}}` rendering — list/object/scalar substitution uses the declared schema shape instead of stringly-only `replace()` | **Done** |
| 33C | CLI / stdin coercion against the schema — `-v key=value` parses to the declared type; one schema property (default `body`) routes from stdin via `x-aichat.source` | **Done** |
| 33D | Preflight shape check between adjacent pipeline stages (extends Phase 15B containment) — strict when both stages declare schemas, soft-warn otherwise | **Done** |
| 33E | Deprecation handling for the `variables:` block — preserved as sugar with a single warning if mixed with `input_schema:` in the same role; no removal date | **Done** |

## Background

[architecture.md](../../architecture/architecture.md) and [`src/config/role.rs`](../../../src/config/role.rs) currently expose five distinct dialects that all feed a role at invocation time:

| Channel | Declaration | Type system | Surface |
|---|---|---|---|
| `variables:` | role frontmatter list | string-only | `{{name}}` substitution + `-v` CLI |
| `input_schema:` | role frontmatter | full JSON Schema | message validation, no substitution |
| `__INPUT__` | sentinel string in body | n/a | the user message / stdin content |
| `{{__sys__}}` / `{{$AICHAT_*}}` | hard-coded | string | runtime facts, env |
| `{{.field}}` | record-aware (`--each`) | JSON | batch templating only |

The author declares the contract twice (`variables:` plus `input_schema:`), keeps the names in sync by convention, and the CLI caller has to know which channel each value flows through. Non-string `default:` values are silently dropped at parse with a `warn!` ([`src/config/role.rs:1229-1237`](../../../src/config/role.rs)), so a malformed role looks healthy until invocation.

Phase 33 collapses the first two channels into one. The reserved namespaces (`__sys__`, `$AICHAT_*`, `.field`) are kept as-is — they're orthogonal, well-understood, and never collide with user-declared slots.

## Design tenets

1. **One declared contract per role.** The schema *is* the parameter list. Everything else derives.
2. **Type-aware rendering.** `{{x}}` knows whether `x` is a string, list, or object, and renders accordingly.
3. **One grammar at every layer.** CLI args, stdin, pipeline inputs, REPL state, and programmatic invocation all coerce against the same schema. The role doesn't care where the value came from.
4. **Reserved namespaces stay reserved.** `__sys__`, `$AICHAT_*`, `.field` keep working and are explicitly documented as orthogonal.
5. **Validation is free but optional.** No schema → free-form text input, exactly like today. Declared schema → validated inputs, typed substitution, contract-checkable composition.

## Target shape

```yaml
---
input_schema:
  type: object
  properties:
    target:
      type: string
      default: "main"
    depth:
      type: integer
      default: 3
      minimum: 1
      maximum: 10
    files:
      type: array
      items: { type: string }
    body:
      type: string
      x-aichat: { source: stdin }
  required: [files]
---
Review {{target}} at depth {{depth}}.
Files:
{{files}}

{{body}}
```

What this collapses, channel-by-channel:

| Before | After |
|---|---|
| `variables: [{name, default}]` | A schema property with `default:` |
| `-v name=value` (string-only) | `-v name=value` type-coerced against the property |
| `input_schema:` validates the message | The schema covers the message via a named slot (`body:` by convention) annotated with `x-aichat: { source: stdin }` |
| `__INPUT__` sentinel | `{{body}}` — uniform with every other slot |
| Stringly composition between pipeline stages | Preflight shape compatibility check between stage N's `output_schema` and stage N+1's `input_schema` |

## 33A Design — Defaults inside `input_schema`

Extend the `input_schema:` parse path to recognize `default:` per property. Two forms, mirroring the existing `VariableDefault` enum at [`src/config/role.rs:800-805`](../../../src/config/role.rs):

```yaml
properties:
  target:
    type: string
    default: "main"
  today:
    type: string
    default: { shell: "date +%F" }
```

Resolution precedence (highest first), matching today's `variables:` semantics so migration is mechanical:

1. CLI `-v name=value` (Phase 33C handles type coercion)
2. Pipeline stage input field of the same name
3. Property's `default:` (literal or shell)
4. `required: [name]` triggers an error if still unresolved

Schema validation runs **after** defaults are filled. A property with both `default:` and `required:` is allowed — the default satisfies the requirement.

**Files:** [`src/config/role.rs`](../../../src/config/role.rs) (parse defaults out of properties; reuse `VariableDefault::resolve`), [`src/config/mod.rs`](../../../src/config/mod.rs) (new `resolve_input_schema_defaults` analogous to `resolve_role_variables`).

## 33B Design — Type-aware rendering

Today's [`apply_variables`](../../../src/config/role.rs) does `prompt.replace("{{name}}", value)` with `value: String`. Phase 33B routes substitution through a renderer that consults the schema:

```rust
fn render_slot(value: &serde_json::Value, schema_type: &str) -> String {
    match (value, schema_type) {
        (Value::String(s), _) => s.clone(),
        (Value::Number(n), _) => n.to_string(),
        (Value::Bool(b), _) => b.to_string(),
        (Value::Null, _) => String::new(),
        (Value::Array(_) | Value::Object(_), _) => serde_json::to_string(value).unwrap(),
    }
}
```

Compact JSON is the default for arrays and objects; pretty-print is opt-in via `x-aichat: { render: pretty }` on the property. This keeps tokens cheap by default while leaving an escape hatch for human-readable injection.

**Files:** [`src/utils/variables.rs`](../../../src/utils/variables.rs) (extend `RE_VARIABLE` resolver to consult a typed value map), [`src/config/role.rs`](../../../src/config/role.rs) (`apply_variables` takes `IndexMap<String, Value>` instead of `IndexMap<String, String>`).

## 33C Design — CLI / stdin coercion

`-v key=value` today is stringly-typed and gets dropped into `variables:`. Phase 33C parses the RHS against the schema:

```
$ aichat -r review -v depth=5 -v files='["a.rs","b.rs"]' < changes.diff
```

Parse rules:

- If the property is `string` → keep as-is.
- If the property is `integer` / `number` / `boolean` → parse with `serde_json::from_str`; error on failure with a helpful message that quotes the schema.
- If the property is `array` / `object` → parse as JSON, validate against `items` / nested `properties`.
- For convenience: `-v files=@path/to/file.json` reads the file and parses it.

Stdin routing: exactly one property may carry `x-aichat: { source: stdin }`. When stdin is non-empty, its content populates that slot. Convention is to name it `body` for free-text roles, but any name works. Roles with no stdin-sourced slot ignore stdin (preserves today's behavior for prompt-only roles).

`--file path` and positional args also flow through this layer, all converging on the same schema-coerced value map.

**Files:** [`src/cli.rs`](../../../src/cli.rs) (extend `-v` parsing with schema awareness), [`src/config/input.rs`](../../../src/config/input.rs) (stdin routing).

## 33D Design — Pipeline shape-check

Extends Phase 15B's containment check from "is output N a subset of input N+1" to "does the value flow have somewhere to land." Two modes:

- **Strict** (both stages declare schemas): verify every required property of N+1's `input_schema` is satisfied by N's `output_schema` (structural subset). Fail preflight on mismatch with the [Phase 13B](phase-13-overview.md) side-by-side diff.
- **Soft-warn** (one side missing): emit a warning, continue. Preserves today's tolerant pipeline behavior for roles that haven't migrated.

When both stages declare schemas *and* explicit field mappings are supplied (future work, out of scope here), the mapping shape is checked instead of the bare subset.

**Files:** [`src/config/preflight.rs`](../../../src/config/preflight.rs) (extend `validate_pipeline_stages`), [`src/pipe.rs`](../../../src/pipe.rs) (route prior stage output into next stage's typed value map).

## 33E Design — `variables:` deprecation

`variables:` continues to parse and work, indefinitely, as syntactic sugar for a string-only `input_schema:` slice:

```yaml
# Legacy
variables:
  - name: target
    default: "main"

# Equivalent under Phase 33
input_schema:
  type: object
  properties:
    target: { type: string, default: "main" }
```

Internal model: at parse time, `variables:` is folded into the role's effective `input_schema:`. If the role declares both, emit a single one-time warning per role load:

```
warn: role 'foo' declares both `variables:` and `input_schema:`. The
      `variables:` block is preserved as sugar; new slots should be
      declared in `input_schema:` directly. See docs/features/typed-input.md.
```

No removal is planned. The block is documented as legacy and `--fork-role` ([Phase 13A](phase-13-overview.md)) emits the new shape.

**Files:** [`src/config/role.rs`](../../../src/config/role.rs) (fold `variables` into `input_schema` after parse), `docs/features/typed-input.md` (new user-facing doc).

## Shipped — unification core (2026-05-30)

33A + 33B + 33E landed together as the design's first PR. Demo:
[`docs/demos/phase-33-typed-input.md`](../../demos/phase-33-typed-input.md).

**33A — schema defaults.** `schema_slots()` (`src/config/role.rs`) flattens an
`input_schema`'s `properties` into `SchemaSlot`s carrying an optional
`SlotDefault` (`Literal(Value)` or `Shell(String)` — a `default:` object with a
single `shell:` key), plus `required`/`pretty` flags. A property's default is
resolved into the `{{slot}}` map; precedence is CLI `-v` > default. A `required:`
property with no value is **skipped, not errored** here — the schema's existing
message validation still enforces it, so roles that pass their payload as the
message keep working unchanged. (Full `-v` type coercion and stdin routing are
33C.)

**33B — type-aware rendering.** `render_slot(&Value, pretty)` renders scalars
bare, `null` empty, and arrays/objects as compact JSON (pretty opt-in via
`x-aichat: { render: pretty }`). Strings pass through unchanged, so existing
string-only roles render identically. Rendering happens at the resolution
boundary, so `Role::apply_variables` keeps its `IndexMap<String, String>`
signature (the typed values are rendered to strings before splicing) — a
smaller blast radius than changing the splice signature.

**33E — `variables:` folding.** `resolve_slots(variables, input_schema, cli)`
merges both channels into one rendered map; the schema property wins on a name
collision (the schema is the source of truth). `Config::resolve_role_variables`
delegates to it and emits one warning per load when a role declares both blocks.
`retrieve_role` now resolves slots when **either** channel is present.

**33C — CLI/stdin (2026-05-30, follow-on commit).** `coerce_cli_value(name, raw,
slot_type)` parses a `-v` value into the declared JSON type: `string` verbatim,
`integer`/`number`/`boolean` via parse (error names the property + expected
type), `array`/`object` via `serde_json` (must produce the matching container),
and `@path` reads a file (JSON for containers, verbatim for a string slot).
`resolve_slots` runs it for schema slots and propagates the error. `stdin_slot()`
finds the property annotated `x-aichat: { source: stdin }`; `Role::route_stdin_slot`
rewrites its `{{name}}` to the `INPUT_PLACEHOLDER` sentinel so the existing
embedded-prompt machinery splices the message there, and `Role::has_stdin_slot`
gates the raw-message schema validation off (in both `main.rs` and `pipe.rs`) for
those roles — a per-role opt-in, so plain `input_schema` roles still validate
their message exactly as before.

**33D — pipeline shape-check (2026-05-30, follow-on commit).** `enforce_pipeline_shape`
turns the Phase 15B containment reports into an execution-time policy:
`Fail` boundaries (both stages declare schemas and the downstream would provably
reject some upstream output) bail preflight with a teaching diff
("the value flow has nowhere to land"); `Warn` boundaries (a free-text upstream
feeding a structured downstream — one side undeclared) are surfaced as warnings
but do not block; `Ok`/`Unknown`/statically-unanalyzable (`skipped`) pass.
`validate_pipeline_shape` runs it over the sequential leaf list; the pipe.rs
`preflight_shape` helper gates it to purely-sequential pipelines (fan-out/switch
DAGs skip — cross-branch shape checking is out of scope). Wired into every
execution preflight: `run` (CLI `--pipe`), `run_pipeline_role` (tool dispatch),
`invoke_role` / `invoke_role_streaming` (server), and `run_inline_pipeline`.

**Tests:** 33 unit tests in `src/config/role.rs` + `src/config/preflight.rs`
(`render_slot` ×6, `schema_slots` ×4, `resolve_slots` ×8, `coerce_cli_value` ×7,
`stdin_slot` ×3, `enforce_pipeline_shape` ×5) plus 8 bats in
`tests/integration/typed-input.sh`. Full suite green (617 unit / 197 compat);
existing pipeline/role/authoring bats unchanged (no pre-existing pipeline trips
the new hard failure).

**Remaining nuance (not a sub-phase):** a `required:` slot supplied via
`-v`/default is not yet hard-enforced at resolve time — the schema's message
validation still covers the non-stdin case. Explicit cross-stage field mappings
and a `pipeline_strict:` opt-in remain future work (see Open questions #4).

## Open questions

### 1. Stdin routing convention

**Question:** What name should the conventional stdin-sourced slot have? `body`? `_message`? `input`? Per-role pick via annotation only?

**Recommendation: `body` as the default name, overridable by `x-aichat.source: stdin` on any string property.** Three reasons. (a) `input` would collide with the role-level concept of "the role's input" and confuse the surface. (b) `_message` leaks an implementation detail (the LLM "user message"). (c) `body` matches HTTP intuition for "the payload part," which is what the LLM actually receives. The annotation override exists so authors with strong opinions can name the slot whatever — but the default name is `body` and the docs lead with that.

### 2. Backward compatibility scope for `variables:`

**Question:** Keep `variables:` indefinitely as sugar, or set a deprecation horizon?

**Recommendation: keep indefinitely; never remove.** Removal violates the "no breaking changes to existing roles" implicit contract that's held since Epic 1. The `variables:` block costs roughly 30 lines of code to keep folding into the schema — orders of magnitude cheaper than auditing every role file in every user's config directory. The single one-time warning when both blocks coexist is sufficient nudge. Treat `variables:` like the way Rust treats `try!` after `?` shipped: not promoted, still works.

### 3. List/object rendering default

**Question:** Compact JSON, YAML-ish, or invoke a per-slot formatter?

**Recommendation: compact JSON by default; opt-in pretty-print via `x-aichat.render: pretty`.** Compact JSON is the only universally-parseable form across every LLM in the project's matrix, costs the fewest tokens (a critical project tenet — see CLAUDE.md "token cost conscious"), and is what `serde_json::to_string` produces with zero extra config. Pretty-print exists because a 50-item array dumped into a prompt is unreadable to humans debugging the role; the annotation is the escape hatch. YAML-ish is rejected because no model is trained on it as a structured-input convention (same reasoning as the TOON rejection in [architecture.md](../../architecture/architecture.md) "What Was Killed").

### 4. Pipeline shape-checking — strict or soft-warn?

**Question:** When pipeline stages declare schemas, should mismatches block preflight or just warn?

**Recommendation: strict when both adjacent stages declare schemas; soft-warn when either side is undeclared.** Strict matters because pipelines compose roles into mini-programs and the only way the "typed ports" promise (Epic 4) is real is if shape mismatches fail loud. Soft-warn matters because half the existing roles in `assets/roles/` and every user-authored role from before Epic 4 don't declare schemas; flipping those to strict overnight would break working pipelines for zero new safety (you cannot check what isn't declared). The behavior is deterministic and documented: declare both, get the contract; declare neither, get today's behavior; mixed, get a warning that points at the gap.

A future opt-in could promote soft-warn to strict via `pipeline_strict: true` at the role or config level. Out of scope here — ship the default policy first.

## Out of scope (anti-roadmap candidates)

- **A Jinja-class templating engine.** Keep `{{name}}` substitution; the addition is type-awareness, not control flow beyond what `{{#if}}` / `{{#unless}}` already cover.
- **Per-slot formatter functions.** `{{name | json}}` style filters create a templating sub-DSL. The schema-driven default + the `x-aichat.render: pretty` escape hatch covers the legitimate use cases without a parser.
- **A new schema language.** JSON Schema is the contract. `x-aichat` extensions are namespaced per [SPEC-mcp-json-artifact.md](../../architecture/integrated-architecture/SPEC-mcp-json-artifact.md) conventions.
- **Magic type inference for undeclared slots.** If the schema doesn't declare `foo`, then `{{foo}}` resolves exactly the way it does today — via `variables:` if present, else stays literal. No silent upgrades, no surprises.
- **Renaming `input_schema:` to `input:`.** Pure churn. The current name is descriptive and every existing role uses it.

## Sequencing

- **33A and 33B can land in either order**, but B without A is half a feature (no values to render) and A without B is a parser change with no user-visible effect. Land together in a single PR.
- **33C depends on 33A** (needs the schema-as-source-of-truth).
- **33D depends on 33A and 33B** for the strict-mode value flow.
- **33E depends on 33A** (folding `variables:` into `input_schema:` internally).

Suggested PR shape: one PR for 33A+33B+33E (the unification core), one for 33C (CLI surface), one for 33D (pipeline integration). Each ships independently usable.

## Files (consolidated)

- [`src/config/role.rs`](../../../src/config/role.rs) — schema-level defaults, `apply_variables` signature change, `variables:` folding
- [`src/utils/variables.rs`](../../../src/utils/variables.rs) — type-aware `{{name}}` resolver
- [`src/config/mod.rs`](../../../src/config/mod.rs) — `resolve_input_schema_defaults`
- [`src/config/input.rs`](../../../src/config/input.rs) — stdin slot routing
- [`src/cli.rs`](../../../src/cli.rs) — `-v` schema coercion, `@file.json` syntax
- [`src/config/preflight.rs`](../../../src/config/preflight.rs) — adjacent-stage shape check
- [`src/pipe.rs`](../../../src/pipe.rs) — prior-stage output → next-stage typed value map
- `docs/features/typed-input.md` — new user-facing doc, sibling to [docs/features/repl-pi.md](../../features/repl-pi.md)
