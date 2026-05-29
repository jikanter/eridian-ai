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
Created <config>/roles/my-analyst.md:

  ---
  extends: base-analyst
  # model: openai:gpt-4o-mini        # override the parent's model
  # temperature: 0.3        # override the parent's temperature
  # output_schema:           # override the parent's output schema
  ---
  # Add your prompt additions here. The parent prompt is inherited.

Edit the file, then run:  aichat -r my-analyst "your input"
```

- The commented-out hints are seeded with the **parent's actual values**, so
  you can see what you're inheriting before you override it.
- The source must resolve (its own `extends:`/`include:` chain is walked too),
  so forking a role with a broken parent fails immediately rather than at run
  time.
- Forking never clobbers: if `roles/<new-name>.md` already exists, the command
  errors and changes nothing.
- `-o json` emits `{ "created": <path>, "name": <new-name>, "extends": <source> }`
  for scripting.

The child inherits the parent's prompt and settings via the existing
`extends:` machinery — see [Macros](./macros.md) and the role composition rules
in [architecture.md](../architecture/architecture.md).

---

## `--explain-role` — read a role's composition

`--explain-role` answers "what will this do, and what is it built from?"
without spending a token or running anything:

```bash
$ aichat --explain-role code-reviewer
Role: code-reviewer
  Reviews code for correctness and style issues.

Composition:
  • extends `base-analyst` — inherits its prompt and settings
  • includes: json-output — their prompts are prepended
  • model: claude:claude-sonnet-4-6
  • tools: web_search, fs_cat, execute_command (3)
  • input port:  text
  • output port: json{issues, severity}
  • capabilities: code-review, security

Pipeline (3 stages):
  1. extract (deepseek:deepseek-chat)
  2. review (claude:claude-sonnet-4-6)
  3. format (deepseek:deepseek-chat)

Invoke:
  aichat -r code-reviewer "your input"
```

`-o json` returns the same composition as a machine-readable object
(`name`, `description`, `extends`, `includes`, `model`, `tools`, `input`,
`output`, `capabilities`, `pipe_to`, `save_to`, `is_pipeline`, `pipeline`).

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
shows the **field-level delta** and points at the fix instead of dumping a raw
validator message:

```
$ aichat --pipe --stage extract --stage review "..."
Pipeline stage 2/2 (role 'review', model '…') failed: Schema input validation failed.

  Stage 1 produced:  { text, summary }
  Stage 2 expects:   { content, language }  (role 'review')

  Missing fields: content, language
  Extra fields:   text, summary

  hint: stage 2 (role 'review') expects fields stage 1 didn't produce.
        Insert a transform role between stages 1 and 2 to reshape the data:
          aichat --fork-role <source-role> my-adapter
```

The same diff appears when the *first* stage's input doesn't match its
`input_schema:` — there the framing is "Input provided" vs "Stage 1 expects",
and the hint drops the cross-stage transform suggestion.

Non-object schemas (a stage expecting plain `text` or an `array`) fall back to
the terse validator message, since a property-name diff would be meaningless
there.
