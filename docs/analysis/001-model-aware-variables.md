# Spec 001: Model-Aware Variables

**Status:** Draft
**Branch:** `spec/model-aware-variables`
**Affects:** `src/utils/variables.rs`, `src/config/role.rs`, `src/config/agent.rs`

## Problem

Role prompts are static text. The same `%shell%` prompt is sent verbatim whether the
backing model is `claude-sonnet-4-20250514` (200k context, vision, tool use) or
`gpt-3.5-turbo` (16k context, no vision). Prompt authors either write for the
lowest common denominator or accept breakage when users swap models.

In a multi-agent world this gets worse: an orchestrating agent may delegate to
sub-agents backed by different models. The prompts need to adapt without requiring
a separate role file per model.

## Proposal

Extend the existing `{{variable}}` interpolation system with model-derived
variables, and add lightweight conditional blocks to role/agent markdown.

### Part A: New Variables

These are resolved at the same time as the existing system variables (role load /
agent instruction build), after the `Model` is known.

| Variable | Type | Source | Example Value |
|---|---|---|---|
| `__model_id__` | string | `Model::id()` | `openai:gpt-4o` |
| `__model_name__` | string | `Model::name()` | `gpt-4o` |
| `__model_client__` | string | `Model::client_name()` | `openai` |
| `__max_input_tokens__` | int\|`unknown` | `ModelData::max_input_tokens` | `128000` |
| `__max_output_tokens__` | int\|`unknown` | `ModelData::max_output_tokens` | `16384` |
| `__supports_vision__` | bool | `ModelData::supports_vision` | `true` |
| `__supports_function_calling__` | bool | `ModelData::supports_function_calling` | `true` |
| `__supports_stream__` | bool | `!ModelData::no_stream` | `true` |

**Design note:** Values come straight from `ModelData` fields that already exist
and are populated by `models.yaml`. No new metadata to maintain.

### Part B: Conditional Blocks

A minimal conditional syntax inside `{{...}}` double-brace markers, reusing the
same regex boundary the interpolator already owns.

#### Syntax

```
{{#if VAR}}
...content when VAR is truthy...
{{/if}}

{{#unless VAR}}
...content when VAR is falsy...
{{/unless}}
```

**Truthiness rules:**
- `false`, `0`, empty string, `unknown` => falsy
- Everything else => truthy

#### Numeric Comparisons

```
{{#if __max_input_tokens__ >= 64000}}
You may include full file contents in your responses.
{{/if}}

{{#if __max_output_tokens__ < 4096}}
Keep responses under 2000 words.
{{/if}}
```

Operators: `>`, `>=`, `<`, `<=`, `==`, `!=`

Left side must be a variable name. Right side must be a literal integer.

#### String Equality

```
{{#if __model_client__ == openai}}
Use markdown code fences with language tags.
{{/if}}
```

Right side is an unquoted literal. Comparison is exact match.

#### No Nesting

Conditionals do not nest. This is a deliberate constraint to keep role files
readable. If you need complex branching, use `dynamic_instructions` or a macro.

### Part C: Call-Site Change

Currently `interpolate_variables` is a pure function of environment state -- it
takes `&mut String` and reads `env::consts`, `os_info`, etc. It has no access to
the `Model`.

The minimal change:

```rust
// Before
pub fn interpolate_variables(text: &mut String) { ... }

// After
pub fn interpolate_variables(text: &mut String, model: Option<&Model>) { ... }
```

When `model` is `Some`, the model variables resolve to real values.
When `model` is `None` (e.g., during early init before model selection), they
pass through unresolved (current behavior for unknown keys).

Call-sites that already have the model in scope:

| Location | Has Model? | Notes |
|---|---|---|
| `Role::new()` at `role.rs:70` | No -- model assigned later | Pass `None`; re-interpolate at bind time |
| `Agent::interpolated_instructions()` at `agent.rs:240` | Yes -- `self.model` | Pass `Some(&self.model)` |
| `Config::set_role()` / `Config::set_agent()` | Yes -- config holds model | Second pass with `Some` |

This means role prompts get two interpolation passes:

1. **Parse time** (`Role::new`): system variables resolve (`__os__`, `__shell__`, etc.)
2. **Bind time** (when role is activated with a model): model variables and conditionals resolve

This is clean because system variables never change mid-process, but the model
can change if the user switches models in a session.

## Worked Example

A model-aware `%shell%` role:

```markdown
Provide only {{__shell__}} commands for {{__os_distro__}} without any description.
Ensure the output is a valid {{__shell__}} command.
If there is a lack of details, provide most logical solution.
If multiple steps are required, try to combine them using '&&' (For PowerShell, use ';' instead).
Output only plain text without any markdown formatting.

{{#if __max_output_tokens__ < 4096}}
Provide only the single most relevant command.
{{/if}}
```

A model-aware agent instruction:

```markdown
You are a code review assistant.

{{#if __supports_function_calling__}}
Use the provided tools to read files and run linters before giving feedback.
{{/if}}

{{#unless __supports_function_calling__}}
Ask the user to paste the code directly. You do not have access to tools.
{{/unless}}

{{#if __supports_vision__}}
If the user shares a screenshot, analyze it directly.
{{/if}}

{{#if __max_input_tokens__ >= 100000}}
You may request entire files for review. Context is plentiful.
{{/if}}

{{#if __max_input_tokens__ < 32000}}
Ask the user to share only the relevant functions. Be conservative with context.
{{/if}}
```

## Implementation Plan

### Phase 1: Variables Only (no conditionals)

1. Add `interpolate_variables_with_model(text: &mut String, model: Option<&Model>)` to `variables.rs`
2. Rename existing `interpolate_variables` to call the new function with `None`
3. Wire up the second pass in `Config::set_role()` and `Agent::interpolated_instructions()`
4. Add variables to the match arms: `__model_id__`, `__model_name__`, etc.

**Estimated diff:** ~60 lines in `variables.rs`, ~10 lines each in `role.rs`, `agent.rs`, `mod.rs`

### Phase 2: Conditional Blocks

1. Add a pre-pass in `interpolate_variables_with_model` that processes `{{#if ...}}` / `{{/if}}` blocks *before* simple variable substitution
2. Parse the condition: variable name, optional operator, optional literal
3. Evaluate against resolved variable values
4. Strip or retain the block contents
5. Then run normal `{{var}}` replacement on the result

**Estimated diff:** ~120 lines in `variables.rs`, new tests

### Phase 3: User-Defined Model Tags (future, out of scope)

Allow `models.yaml` entries to carry arbitrary tags:

```yaml
- name: gpt-4o
  tags: [fast, cheap, good-at-code]
```

Expose as `__model_has_tag_fast__` or similar. This is deferred because the
built-in `ModelData` fields cover the critical cases today.

## Compatibility

- **Fully backward compatible.** Existing roles contain no `{{#if` blocks and
  no `__model_*__` variables, so they interpolate identically.
- Unknown variables already pass through as `{{key}}` -- the model variables
  when `model` is `None` follow this exact behavior.
- No config file format changes. No new CLI flags.
- The `RoleLike` trait is unchanged.

## Alternatives Considered

### Full template engine (Tera, Handlebars)

Overkill. Brings a dependency, a learning curve, and an attack surface for
prompt injection via template directives. The `{{var}}` system is intentionally
minimal and this spec keeps it that way.

### Per-model role files (`%shell%.gpt-4o.md`)

Combinatorial explosion. 20+ providers x N models = unmaintainable. Conditionals
inside a single file are strictly better.

### Runtime prompt rewriting by another LLM

Clever but slow, expensive, and unpredictable. Deterministic conditionals are
the right tool here.

## Open Questions

1. **Should `{{#else}}` be supported?** It makes blocks shorter but adds parser
   complexity. The `{{#unless}}` alternative is more explicit. Leaning no for v1.
2. **Should conditionals work in macro steps?** Probably yes, since macro
   interpolation already uses a similar `{{var}}` pattern, but the model context
   may not be available in all macro execution paths.
3. **Should there be a `--dump-variables` CLI flag?** Useful for debugging what
   a role resolves to for a given model. Low effort, high utility.
