# Typed Input

A role's `input_schema:` is the single declaration of what the role takes. CLI args, stdin, pipeline inputs, and REPL state all coerce against it; the schema's properties are also referenceable as `{{name}}` slots inside the prompt body, with type-aware rendering. The legacy `variables:` block continues to work and is documented as sugar for a string-only `input_schema:`.

> **Status (2026-05-30):** **Shipped — 33A** (per-property `default:`, literal + shell), **33B** (type-aware `{{slot}}` rendering), **33E** (`variables:` folded into the schema slot space with a one-time warning), and **33C** (`-v` type coercion against the declared type incl. the `@file.json` form; stdin routing via `x-aichat: { source: stdin }`). So: schema defaults fill `{{name}}` slots, `-v depth=5` is coerced to the integer `5` (bad values error, naming the property and type), arrays/objects render as compact JSON (pretty opt-in), and a `source: stdin` slot receives the free-text message (its raw message is then not validated against the object schema). **Not yet shipped:** **33D** — strict adjacent-stage pipeline shape-checking; until then pipeline composition stays soft (Phase 15B). One nuance still deferred from 33C: a `required:` slot supplied via `-v`/default is not yet hard-enforced at resolve time (the schema's message validation covers the non-stdin case). The reserved namespaces (`{{__sys__}}`, `{{$AICHAT_*}}`, `{{.field}}`) work identically everywhere.

See also: [Phase 33 design](../roadmap/phase-33-overview.md), [architecture.md](../architecture/architecture.md), [Macros](./macros.md), [Knowledge](./knowledge.md).

## At a glance

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

Invoke:

```bash
aichat -r review -v files='["src/a.rs","src/b.rs"]' -v depth=5 < changes.diff
```

What happens:

1. CLI `-v` values parse against the schema. `depth=5` becomes an integer; `files='[...]'` parses as a JSON array.
2. Stdin populates `body` because `body` is annotated `x-aichat: { source: stdin }`.
3. `target` is not provided, so its `default: "main"` resolves.
4. The full value map validates against `input_schema:` before any model call.
5. The prompt body renders: `{{target}}` → `main`, `{{depth}}` → `5`, `{{files}}` → compact JSON `["src/a.rs","src/b.rs"]`, `{{body}}` → the stdin content as-is.

## Declaring inputs

`input_schema:` is a JSON Schema object describing the inputs the role accepts. Two things are new beyond standard JSON Schema:

- Each property may carry a `default:` — either a literal or a `{ shell: "command" }` mapping. Shell defaults run on role load and capture trimmed stdout.
- An `x-aichat` extension block carries aichat-specific annotations (currently `source` and `render`).

```yaml
input_schema:
  type: object
  properties:
    project:
      type: string
      default: "Eridian"
    today:
      type: string
      default: { shell: "date +%F" }
    severity:
      type: string
      enum: [low, medium, high]
      default: "medium"
    options:
      type: object
      properties:
        verbose: { type: boolean, default: false }
        max_iters: { type: integer, default: 5 }
  required: [project]
```

Resolution precedence per property, highest first:

1. CLI `-v name=value`
2. Pipeline input field of the same name from the prior stage
3. Property's `default:` (literal or `{ shell: ... }`)
4. If still unresolved and the property is in `required:`, the role errors out before any model call.

Validation runs after defaults are filled. A property that's both required and has a default is fine — the default satisfies the requirement.

## Substituting into the prompt

Every schema property becomes available as `{{name}}` inside the prompt body. The substitution is type-aware:

| Property type | Rendered as |
|---|---|
| `string` | The string verbatim |
| `integer` / `number` | `value.to_string()` |
| `boolean` | `"true"` / `"false"` |
| `null` | empty string |
| `array` / `object` | compact JSON (e.g. `[1,2,3]`, `{"a":1}`) |

If you want a pretty-printed array or object instead, opt in per property:

```yaml
options:
  type: object
  x-aichat: { render: pretty }
  properties: { ... }
```

Compact JSON is the default because it minimizes tokens (Eridian is token-cost conscious) and parses unambiguously across every model. Reach for `pretty` only when the human reading the prompt during debugging matters more than the model's token budget.

## Routing stdin into a slot

Exactly one property may carry `x-aichat: { source: stdin }`. When stdin is non-empty, its content populates that slot. By convention, name it `body`:

```yaml
input_schema:
  type: object
  properties:
    body:
      type: string
      x-aichat: { source: stdin }
  required: [body]
```

Then the role body references it with `{{body}}` like any other slot. Roles with no stdin-sourced slot ignore stdin entirely (preserving the prompt-only role pattern).

## CLI surface

```bash
aichat -r myrole -v key=value          # type-coerced against the schema
aichat -r myrole -v files='[...]'      # JSON literal for arrays / objects
aichat -r myrole -v config=@cfg.json   # read JSON from a file
aichat -r myrole < payload             # populates the `x-aichat.source: stdin` slot
echo "..." | aichat -r myrole          # same
```

If a `-v` value fails to parse against the declared type, the error names the property, the value, the expected type, and the relevant schema fragment. No silent fallback to "string."

## Reserved namespaces (unchanged)

Three substitution families are independent of `input_schema:` and continue to work:

- `{{__model_id__}}`, `{{__os__}}`, `{{__cwd__}}`, `{{__now__}}`, etc. — runtime facts. Full list in [`src/utils/variables.rs`](../../src/utils/variables.rs).
- `{{$AICHAT_FOO}}` — environment variables, gated to the `AICHAT_` prefix.
- `{{.field}}` — record field in batch (`--each`) mode.

These cannot be shadowed by `input_schema:` properties because they live in a separate name namespace (double-underscore, `$`, and `.` prefixes respectively).

Conditional blocks (`{{#if ... }} ... {{/if}}`, `{{#unless ...}}`) also work and can reference both schema slots and reserved variables:

```yaml
---
input_schema:
  type: object
  properties:
    verbose: { type: boolean, default: false }
---
{{#if verbose}}
Be exhaustive.
{{/if}}
{{#if __supports_vision__}}
Inspect any images in the input.
{{/if}}
```

## Pipeline composition

When a role appears as a stage in another role's `pipeline:`, the prior stage's output flows into this stage's typed value map. If both stages declare schemas, the shape is checked at preflight (see [Phase 15B](../roadmap/phase-15-overview.md) for the containment-check logic and [Phase 33D](../roadmap/phase-33-overview.md) for the adjacent-stage extension): every required property of the downstream `input_schema:` must be satisfied by something in the upstream `output_schema:`.

If either side doesn't declare a schema, the pipeline emits a warning and falls back to today's behavior of treating the prior stage's text output as the next stage's message body.

## Migrating from `variables:`

The legacy `variables:` block is preserved as sugar. Both forms are equivalent:

```yaml
# Legacy — still works
variables:
  - name: target
    default: "main"
  - name: today
    default: { shell: "date +%F" }
```

```yaml
# Modern — same semantics
input_schema:
  type: object
  properties:
    target:
      type: string
      default: "main"
    today:
      type: string
      default: { shell: "date +%F" }
```

There is no removal date for `variables:`. If a role declares both `variables:` and `input_schema:`, aichat emits a single warning per role load and proceeds — the schema takes precedence, and any `variables:` entries with names not already in the schema are merged in as string-typed properties.

If you want to migrate manually, [`--fork-role`](../roadmap/phase-13-overview.md) ([Phase 13A](../roadmap/phase-13-overview.md)) emits the modern form. A `variables:` block stays string-only forever; the upgrade is purely a question of whether you want types, validation, and pipeline shape-checking on those slots.

## Why types matter here

Three concrete payoffs:

1. **Pipelines compose without surprises.** When stages share typed contracts, preflight catches shape mismatches before any model call. No more 30-second pipeline that fails on stage 4 because stage 3 emitted `"summary"` instead of `"content"`.
2. **CLI args mean what they say.** `-v depth=5` is the integer five, not the string `"5"`. Cheaper for the model to reason about, and validates against `minimum:` / `maximum:` / `enum:` before token one.
3. **Roles become callable as typed functions.** Agents consuming roles as tools, federated `/v1/roles` callers, and pipelines composing roles — all see the same schema and don't need a side-channel description of the parameter shape.

## Anti-patterns

- **Don't stringify structured data unless you have to.** `-v files='[...]'` works, but if the schema declares `files` as an array, prefer `-v files=@files.json` for anything non-trivial.
- **Don't use reserved-namespace prefixes for your own slots.** `__foo__`, `$FOO`, and `.foo` are reserved. The schema lets you, but downstream tooling may not.
- **Don't lean on `default: { shell: ... }` for slow commands.** The shell runs on every role load (including REPL `.role` switches). Keep it under a second or move it to a `pipe_to`/`save_to` hook in a pre-stage role.
- **Don't mix `variables:` and `input_schema:` slot names.** It works, but the warning is there because the merge ordering can be subtle. Pick one block per slot name.
