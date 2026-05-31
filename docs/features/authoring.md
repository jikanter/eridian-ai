# Authoring & Teaching Roles

Roles are the unit of composition in aichat. Phase 13 makes them cheaper to
*author* and the system more willing to *teach* you as you go. Four small,
zero-token features:

| Command / behavior | What it does |
|---|---|
| `--fork-role <source> <new-name>` | Scaffold a new role that `extends:` an existing one |
| `--explain-role <name>` | Print what a role does and how it composes |
| Built-in `guardrail-*` roles | Worked examples of the guardrail-in-a-pipeline pattern |
| Schema-mismatch teaching errors | Pipeline shape errors show the field delta and a fix |

See also: [Macros](./macros.md), [Typed Input](./typed-input.md),
[architecture.md](../architecture/architecture.md),
[Phase 13 design](../roadmap/phase-13-overview.md).

---

## `--fork-role` — scaffold an `extends:` child

Forking turns "copy the frontmatter, remember that `extends:` exists, comment
out the fields I might override" into one command:

```bash
$ aichat --fork-role base-analyst my-analyst
Created <config>/roles/my-analyst.md
  extends: base-analyst
  Uncomment the fields you want to override, then edit the prompt body.
```

The written file carries the parent's declarations as commented-out hints, so
the fork starts as a pure `extends:` (a no-op override) that you can toggle on
field by field:

```yaml
---
extends: base-analyst
# model: claude:claude-sonnet-4-6
# temperature: 0.3
# top_p: 1.0
# use_tools: web_search,fs_cat
# input_schema: { type: object, properties: { ... } }
# output_schema: { type: object, properties: { ... } }
---
# Add your prompt additions here. The parent prompt is inherited.
```

- Each hint line shows the **parent's current value** (a placeholder like
  `claude:claude-sonnet-4-6` when the parent doesn't declare that field), so you
  can see what you're inheriting before you override it.
- The parent prompt body is **not** duplicated — it's inherited through the
  `extends:` chain and your additions are appended after it.
- The source must resolve (its own `extends:`/`include:` chain is walked too),
  so forking a role with a broken parent fails immediately rather than at run
  time.
- Forking never clobbers: if `roles/<new-name>.md` already exists, the command
  errors and changes nothing.
- `-o json` emits `{ "source": …, "new_name": …, "path": … }` for scripting.

---

## `--explain-role` — read a role's composition

`--explain-role` answers "what will this do, and what is it built from?"
without spending a token or running anything:

```bash
$ aichat --explain-role base-analyst
Role: base-analyst
  description: Analyze input carefully.
  source: <config>/roles/base-analyst.md
  model: <default>
  temperature: 0.3
  in: any  out: text
  capabilities: [analysis]
  tools: [web_search, fs_cat]
  prompt: 36 chars (embeds __INPUT__)
```

It surfaces only the fields a role actually declares — `extends:`/`include:`
(shown even though resolution flattens them), model and sampling, port
signatures (`in:`/`out:`), capabilities, tools, knowledge bindings, pipeline
shape (sequential vs DAG, with the stage chain), and lifecycle hooks. `-o json`
returns the same `RoleExplanation` as a machine-readable object:

```bash
$ aichat --explain-role guardrail-pii -o json
{
  "name": "guardrail-pii",
  "description": "Detect and redact personally identifiable information (PII) from text.",
  "source_path": "<builtin asset: guardrail-pii.md>",
  "builtin": true,
  "capabilities": ["guardrail", "pii", "safety"],
  "input": "any",
  "output": "json{safe, redacted, findings}",
  "has_pipeline": false,
  ...
}
```

This complements `--dry-run` (which resolves a role *for a specific input* and
previews the assembled prompt on stderr) and `--list-roles --verbose` (which
shows a one-line port signature per role). `--explain-role` is the
single-role, prose-y middle ground.

---

## Guardrail role examples

Guardrails aren't a new runtime feature — they're a **role authoring pattern**.
aichat ships three worked examples as built-in roles so the pattern is
discoverable (`--list-roles`, `--find-role --capability guardrail`):

| Role | Purpose | Output port |
|---|---|---|
| `guardrail-pii` | Detect & redact personally identifiable information | `json{safe, redacted, findings}` |
| `guardrail-injection` | Flag prompt-injection / jailbreak attempts | `json{safe, attack_type, reason}` |
| `guardrail-topic` | Restrict input to allowed topics (`-v allowed_topics=...`) | `json{safe, topic, reason}` |

Each one:

- declares an `output_schema:` so its verdict is structured and machine-checkable,
- tags itself with `capabilities: [guardrail, …]` for discovery,
- leaves `model:` commented out (a comment notes a cheap/local model suffices),
  so the example runs on your default model out of the box.

Compose them into a pipeline using the existing `pipeline:` mechanism — a
guardrail stage in front of (or behind) your real task:

```yaml
# roles/safe-summary.md
---
pipeline:
  - role: guardrail-pii        # redact before the task ever sees the text
  - role: summarize
  - role: guardrail-topic      # confirm the summary stayed on-topic
---
```

Run a guardrail standalone to see its verdict:

```bash
echo "Email me at jane@example.com" | aichat -r guardrail-pii
# {"safe": false, "redacted": "Email me at [REDACTED:email]", "findings": ["email address"]}
```

Inspect any of them with `--explain-role guardrail-pii`.

---

## Schema-mismatch errors that teach

When one pipeline stage produces a shape the next stage can't accept, the error
shows the **field-level delta** and points at the fix instead of stopping at a
raw validator message:

```
$ aichat --pipe --stage extract --stage review '{"text":"…","summary":"…"}'
Pipeline stage 2/2 (role 'review', …) failed: pipeline stage 2 input schema validation failed (role 'review'):
  Schema input validation failed:
  - "content" is a required property
  - "language" is a required property

  Stage 1 produced: { text, summary }
  Stage 2 expects: { content, language }
  Missing fields: content, language
  Extra fields: text, summary

  hint: shape mismatches between adjacent stages are usually fixed by a
        transform role between them. To start one:
        aichat --fork-role <parent> my-adapter
```

`Stage N produced` lists the JSON keys the upstream stage emitted (or a
`<non-JSON>` preview when it wasn't an object); `Stage N+1 expects` lists the
consumer schema's required (or declared) properties. The missing/extra delta is
only computed when the upstream output parsed as a JSON object — otherwise the
underlying validator message carries the detail.
