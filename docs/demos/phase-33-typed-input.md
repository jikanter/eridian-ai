# Phase 33: Typed Input (33A–E)

*2026-05-30T01:35:30Z by Showboat 0.6.1*
<!-- showboat-id: 3320b554-8ed8-4e9b-86d5-a3c14c90b24b -->

Phase 33 makes `input_schema:` the single source of truth for a role's parameters. This first increment lands the unification core:

- **33A** — per-property `default:` inside `input_schema:` (literal JSON or `{ shell: "…" }`), resolved into the `{{slot}}` map with precedence `-v` > default.
- **33B** — type-aware `{{slot}}` rendering: scalars bare, arrays/objects as compact JSON (pretty opt-in via `x-aichat: { render: pretty }`); strings unchanged.
- **33E** — the legacy `variables:` block folds into the same slot space; declaring both emits one warning and the schema wins on a name collision.

Phase 33 is now complete: **33C** (`-v` type coercion + `@file` + stdin routing) and **33D** (strict adjacent-stage pipeline shape-check) landed in follow-on commits, shown below.

## New surface (src/config/role.rs)

```bash
grep -E "^pub\(crate\) fn (render_slot|schema_slots|resolve_slots)\(|^pub\(crate\) (enum|struct) (SlotDefault|SchemaSlot)" src/config/role.rs
```

```output
pub(crate) fn render_slot(value: &Value, pretty: bool) -> String {
pub(crate) enum SlotDefault {
pub(crate) struct SchemaSlot {
pub(crate) fn resolve_slots(
pub(crate) fn schema_slots(schema: &Value) -> Vec<SchemaSlot> {
```

## 33A + 33B end-to-end (offline, --dry-run)

A role declares its parameters in `input_schema:` with typed defaults — no `variables:` block. The defaults fill the `{{slots}}`; the array renders as compact JSON. `-v` overrides a default.

```bash
set -e
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
mkdir -p "$ROLES_DIR"
cat > "$ROLES_DIR/p33-demo.md" <<EOF
---
input_schema:
  type: object
  properties:
    target: { type: string, default: "main" }
    depth: { type: integer, default: 3 }
    tags: { type: array, default: ["security", "perf"] }
---
Review {{target}} at depth {{depth}}. Tags: {{tags}}.
EOF
echo "# defaults filled:"
./target/debug/aichat -r p33-demo --dry-run "{\"x\":1}" </dev/null 2>/dev/null | grep "^Review"
echo "# -v overrides target and depth:"
./target/debug/aichat -r p33-demo -v target=release -v depth=9 --dry-run "{\"x\":1}" </dev/null 2>/dev/null | grep "^Review"
rm -f "$ROLES_DIR/p33-demo.md"
```

```output
# defaults filled:
Review main at depth 3. Tags: ["security","perf"].
# -v overrides target and depth:
Review release at depth 9. Tags: ["security","perf"].
```

## 33E — variables: still works as sugar

A role may keep using `variables:`. It folds into the same slot space; if a role declares both `variables:` and `input_schema:`, aichat warns once and the schema property wins on a name clash. The legacy form renders identically to before (strings pass through unchanged).

```bash
set -e
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/p33-legacy.md" <<EOF
---
variables:
  - name: target
    default: main
---
Legacy review of {{target}}.
EOF
./target/debug/aichat -r p33-legacy --dry-run "x" </dev/null 2>/dev/null | grep "^Legacy"
rm -f "$ROLES_DIR/p33-legacy.md"
```

```output
Legacy review of main.
```

## Unit coverage

The resolver, renderer, and schema-slot extraction are pure functions, unit-tested without a model: `render_slot` (type-aware rendering), `schema_slots` (default/required/pretty extraction), and `resolve_slots` (the merge + precedence + 33E collision rule).

```bash
cargo test --bin aichat config::role::tests 2>&1 | grep -oE "config::role::tests::(render_slot|schema_slots|resolve_slots)_[a-z_]+" | sort -u
```

```output
config::role::tests::render_slot_array_is_compact_json_by_default
config::role::tests::render_slot_null_is_empty
config::role::tests::render_slot_object_is_compact_json_by_default
config::role::tests::render_slot_pretty_expands_arrays
config::role::tests::render_slot_scalars_render_bare
config::role::tests::render_slot_string_passes_through_unquoted
config::role::tests::resolve_slots_cli_overrides_schema_default
config::role::tests::resolve_slots_fills_schema_literal_default
config::role::tests::resolve_slots_renders_typed_defaults
config::role::tests::resolve_slots_schema_wins_on_name_collision
config::role::tests::resolve_slots_shell_default_runs
config::role::tests::resolve_slots_skips_required_property_without_value
config::role::tests::resolve_slots_variable_default_and_cli
config::role::tests::resolve_slots_variable_required_without_value_errors
config::role::tests::schema_slots_empty_without_properties
config::role::tests::schema_slots_marks_required_and_pretty
config::role::tests::schema_slots_reads_literal_defaults_by_type
config::role::tests::schema_slots_reads_shell_default
```

```bash
cargo test --bin aichat config::role::tests 2>&1 | grep -E "^test result:" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ passed/N passed/; s/[0-9]+ filtered out/N filtered out/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## 33C — CLI coercion + stdin routing

`-v key=value` is now coerced against the declared property type (with `@file.json` to load a value from disk); a bad value errors with the property and expected type named. A property annotated `x-aichat: { source: stdin }` receives the message as free text — routed into its `{{slot}}` via the existing embedded-input machinery — so its raw message is not validated against the object schema.

```bash
set -e
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/p33c.md" <<EOF
---
input_schema:
  type: object
  properties:
    target: { type: string, default: "main" }
    depth: { type: integer, default: 3 }
    body: { type: string, x-aichat: { source: stdin } }
---
Review {{target}} at depth {{depth}}. Input:
{{body}}
EOF
echo "# free-text stdin fills {{body}}; defaults fill the rest:"
printf "the diff" | ./target/debug/aichat -r p33c --dry-run 2>/dev/null | grep -A1 "^Review"
echo "# bad -v against an integer slot errors:"
./target/debug/aichat -r p33c -v depth=deep --dry-run </dev/null 2>&1 | grep -i "depth"
rm -f "$ROLES_DIR/p33c.md"
```

```output
# free-text stdin fills {{body}}; defaults fill the rest:
Review main at depth 3. Input:
the diff
# bad -v against an integer slot errors:
Error: -v depth=deep: value is not a valid integer
```

## 33D — adjacent-stage shape check (completes Phase 33)

When two adjacent pipeline stages both declare schemas, preflight verifies the upstream `output_schema` satisfies the downstream `input_schema` — *before* any model call. A provable mismatch fails fast with a teaching diff; a free-text upstream (no `output_schema`) is a soft warning, not a failure.

```bash
set -e
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/p33-producer.md" <<EOF
---
output_schema:
  type: object
  properties: { summary: { type: string } }
  required: [summary]
---
Summarize.
EOF
cat > "$ROLES_DIR/p33-consumer.md" <<EOF
---
input_schema:
  type: object
  properties: { content: { type: string } }
  required: [content]
---
Use {{content}}.
EOF
echo "# producer emits {summary}; consumer requires {content} -> preflight fails:"
./target/debug/aichat --pipe --stage p33-producer --stage p33-consumer --dry-run "x" </dev/null 2>&1 | head -5
rm -f "$ROLES_DIR/p33-producer.md" "$ROLES_DIR/p33-consumer.md"
```

```output
# producer emits {summary}; consumer requires {content} -> preflight fails:
Error: Preflight: pipeline shape mismatch — the value flow has nowhere to land:
  stage 1 (p33-producer) → stage 2 (p33-consumer)
    Missing (downstream requires, upstream does not provide): content
    Fix: add a transform stage, or align the schemas so the upstream output satisfies the downstream input.
```

## End-to-end (offline)

The integration suite drives the real binary over typed roles under `--dry-run` — schema defaults, `-v` override, coercion error, stdin routing, the control that a plain `input_schema` role still validates its message, and the 33D shape check (fail / pass / soft).

```bash
AICHAT_BIN=./target/debug/aichat bats tests/integration/typed-input.sh 2>&1 | grep -E "^(ok|not ok)"
```

```output
ok 1 typed-input: schema defaults fill slots and arrays render compact (33A/33B)
ok 2 typed-input: -v overrides a schema default (33A)
ok 3 typed-input: bad -v for an integer slot errors (33C coercion)
ok 4 typed-input: stdin routes into a source:stdin slot, no message validation (33C)
ok 5 typed-input: a plain input_schema role still validates the message
ok 6 typed-input: 33D shape check fails an incompatible sequential pipeline
ok 7 typed-input: 33D shape check passes a compatible sequential pipeline
ok 8 typed-input: 33D is soft when the upstream declares no output_schema
```
