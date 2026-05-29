# Phase 13: Authoring & Teaching

*2026-05-29T17:11:38Z by Showboat 0.6.1*
<!-- showboat-id: 1ac63e80-c079-4e19-8918-fb41e9bef160 -->

Phase 13 (Epic 3, Composition UX) makes roles cheaper to author and the system more willing to teach. Four zero-token features, demonstrated here against a hermetic config directory so the demo reproduces offline.

- **13A** `--fork-role <source> <new-name>` — scaffold a role that `extends:` an existing one, with the parent's values seeded as commented-out override hints.
- **13B** Schema-mismatch teaching errors — a pipeline shape error shows the field-level delta (missing/extra) and points at `--fork-role` to author a reshaping adapter.
- **13C** Built-in `guardrail-*` roles — worked examples (PII, prompt-injection, topic) of the guardrail-in-a-pipeline pattern.
- **13D** `--explain-role <name>` — print what a role does and how it composes, with `-o json` for machines.

## Setup — a hermetic config directory

Every block re-exports `AICHAT_CONFIG_DIR` / `AICHAT_ROLES_DIR` so the demo runs against an isolated config that is wiped and recreated here. The config pins a model with a placeholder key — no network is touched, because the 13B path bails at input-schema validation before any model request.

```bash
DIR=/tmp/aichat-phase13-demo
rm -rf "$DIR"; mkdir -p "$DIR/roles"
cat > "$DIR/config.yaml" <<EOF
model: openai:gpt-4o-mini
clients:
  - type: openai
    api_key: sk-placeholder
EOF
cat > "$DIR/roles/base-analyst.md" <<EOF
---
description: Analyze input carefully.
temperature: 0.3
capabilities: [analysis]
use_tools: web_search,fs_cat
---
You are a careful analyst. __INPUT__
EOF
cat > "$DIR/roles/needs-shape.md" <<EOF
---
description: Needs a structured object.
input_schema:
  type: object
  properties:
    content: { type: string }
    language: { type: string }
  required: [content, language]
---
Format __INPUT__
EOF
echo "wrote config + base-analyst + needs-shape into $DIR"
```

```output
wrote config + base-analyst + needs-shape into /tmp/aichat-phase13-demo
```

## 13A — `--fork-role` scaffolds an `extends:` child

One command replaces "copy the frontmatter, remember `extends`, comment out the overrides." The hints are seeded with the parent's *actual* values (`temperature: 0.3`), and forking never clobbers an existing role.

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
./target/debug/aichat --fork-role base-analyst my-analyst </dev/null
```

```output
Created /tmp/aichat-phase13-demo/roles/my-analyst.md:

  ---
  extends: base-analyst
  # model: provider:model-id        # override the parent's model
  # temperature: 0.3        # override the parent's temperature
  # output_schema:           # override the parent's output schema
  ---
  # Add your prompt additions here. The parent prompt is inherited.

Edit the file, then run:  aichat -r my-analyst "your input"
```

Forking is idempotent-safe (refuses to overwrite) and scriptable (`-o json`):

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
echo "--- clobber attempt ---"
./target/debug/aichat --fork-role base-analyst my-analyst </dev/null 2>&1 || echo "(refused; exit $?)"
echo "--- json form (new name) ---"
./target/debug/aichat --fork-role base-analyst my-analyst-json -o json </dev/null
```

```output
--- clobber attempt ---
Error: Role 'my-analyst' already exists at /tmp/aichat-phase13-demo/roles/my-analyst.md — pick another name
(refused; exit 1)
--- json form (new name) ---
{
  "created": "/tmp/aichat-phase13-demo/roles/my-analyst-json.md",
  "name": "my-analyst-json",
  "extends": "base-analyst"
}
```

## 13D — `--explain-role` reads a role's composition

Zero tokens, nothing executed. The forked `my-analyst` resolves through `extends`, so explain shows the inherited model, tools, ports, and capabilities. `-o json` returns the same composition for machines.

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
./target/debug/aichat --explain-role my-analyst </dev/null
```

```output
Role: my-analyst
  Analyze input carefully.

Composition:
  • extends `base-analyst` — inherits its prompt and settings
  • model: (inherits the session/global default)
  • tools: web_search, fs_cat (2)
  • input port:  any
  • output port: text
  • capabilities: analysis

Invoke:
  aichat -r my-analyst "your input"
```

## 13C — built-in guardrail examples

Three guardrail roles ship embedded in the binary, so they appear in any config. They are discoverable by capability and each declares a structured `output_schema` (a guardrail's verdict should be machine-checkable). They demonstrate a *pattern*, not a new runtime feature — compose them into a `pipeline:` in front of or behind your real task.

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
echo "--- discover by capability ---"
./target/debug/aichat --find-role --capability guardrail </dev/null
echo "--- explain one ---"
./target/debug/aichat --explain-role guardrail-pii </dev/null
```

```output
--- discover by capability ---
guardrail-injection
guardrail-pii
guardrail-topic
--- explain one ---
Role: guardrail-pii
  Detect and redact personally identifiable information (PII) from text.

Composition:
  • model: (inherits the session/global default)
  • input port:  any
  • output port: json{safe, redacted, findings}
  • capabilities: guardrail, pii, safety

Invoke:
  aichat -r guardrail-pii "your input"
```

## 13B — schema-mismatch errors that teach

Feeding plain prose into a stage whose `input_schema` expects `{ content, language }` produces a teaching error: it names the missing fields and points at `--fork-role` to build a reshaping adapter. (No model is called — validation fails first.) The same diff appears on the adjacent-stage boundary, framed as "Stage N produced / Stage N+1 expects."

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
./target/debug/aichat --pipe --stage needs-shape "just plain prose, not json" 2>&1
echo "(pipeline exit $?)"
```

```output
Error: Pipeline stage 1/1 (role 'needs-shape', model 'openai:gpt-4o-mini') failed: Schema input validation failed.

  Input provided:    (non-JSON text)
  Stage 1 expects:   { content, language }  (role 'needs-shape')

  Missing fields: content, language

  hint: stage 1 expects fields the input doesn't provide.
(pipeline exit 10)
```

## Verification & docs

- Unit tests: `src/config/role.rs` (`schema_field_diff`, `format_field_set`) and `src/pipe.rs` (`format_stage_input_mismatch`).
- Integration: `tests/integration/authoring.sh` — 12 bats cases covering all four items, hermetic and cleaned up.
- User docs: `docs/features/authoring.md`.

The teaching error keeps the literal phrase `Schema input validation failed` so error classification still maps it to the schema family before the pipeline wrapper re-tags it. Non-object schemas fall back to the terse validator message, where a property-name diff would be meaningless.
