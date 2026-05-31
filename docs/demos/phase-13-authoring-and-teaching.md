# Phase 13: Authoring & Teaching

*2026-05-29T17:33:40Z by Showboat 0.6.1*
<!-- showboat-id: 556f8f48-b169-46c4-8cde-5238c83a7097 -->

Phase 13 (Epic 3, Composition UX) makes roles cheaper to author and the system more willing to teach. Four zero-token features, demonstrated against a hermetic config directory so the demo reproduces offline.

- **13A** `--fork-role <source> <new-name>` — scaffold a role that `extends:` an existing one, with the parent's declarations seeded as commented-out override hints.
- **13B** Schema-mismatch teaching errors — a pipeline shape error shows what the upstream stage produced vs. what the consumer expects, the missing/extra field delta, and a `--fork-role` hint.
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

One command replaces "copy the frontmatter, remember `extends`, comment out the overrides." The written file carries the parent's declarations as commented hints (the parent has no `model:`, so a placeholder is shown); the parent prompt body is inherited, not duplicated.

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
./target/debug/aichat --fork-role base-analyst my-analyst </dev/null
echo "--- written file ---"
cat /tmp/aichat-phase13-demo/roles/my-analyst.md
```

```output
Created /tmp/aichat-phase13-demo/roles/my-analyst.md
  extends: base-analyst
  Uncomment the fields you want to override, then edit the prompt body.
--- written file ---
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
Error: --fork-role: target file /tmp/aichat-phase13-demo/roles/my-analyst.md already exists — pick a different NEW_NAME or remove the existing file
(refused; exit 1)
--- json form (new name) ---
{
  "source": "base-analyst",
  "new_name": "my-analyst-json",
  "path": "/tmp/aichat-phase13-demo/roles/my-analyst-json.md"
}
```

## 13D — `--explain-role` reads a role's composition

Zero tokens, nothing executed. It surfaces only the fields a role declares — model, sampling, port signatures, capabilities, tools, and prompt size.

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
./target/debug/aichat --explain-role base-analyst </dev/null
```

```output
Role: base-analyst
  description: Analyze input carefully.
  source: /tmp/aichat-phase13-demo/roles/base-analyst.md
  model: <default>
  temperature: 0.3
  in: any  out: text
  capabilities: [analysis]
  tools: [web_search, fs_cat]
  prompt: 36 chars (embeds __INPUT__)
```

## 13C — built-in guardrail examples

Three guardrail roles ship embedded in the binary, so they appear in any config. They are discoverable by capability and each declares a structured `output_schema` (a guardrail's verdict should be machine-checkable). They demonstrate a *pattern*, not a new runtime feature — compose them into a `pipeline:` in front of or behind your real task.

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
echo "--- discover by capability ---"
./target/debug/aichat --find-role --capability guardrail </dev/null
echo "--- explain one (json) ---"
./target/debug/aichat --explain-role guardrail-pii -o json </dev/null
```

```output
--- discover by capability ---
guardrail-injection
guardrail-pii
guardrail-topic
--- explain one (json) ---
{
  "name": "guardrail-pii",
  "description": "Detect and redact personally identifiable information (PII) from text.",
  "source_path": "<builtin asset: guardrail-pii.md>",
  "builtin": true,
  "capabilities": [
    "guardrail",
    "pii",
    "safety"
  ],
  "input": "any",
  "output": "json{safe, redacted, findings}",
  "has_pipeline": false,
  "pipeline_stage_count": 0,
  "pipeline_has_dag": false,
  "embedded_input": true,
  "prompt_length": 551
}
```

## 13B — schema-mismatch errors that teach

Feeding a JSON object with the wrong shape into a stage whose `input_schema` expects `{ content, language }` produces a teaching error: it shows what the upstream stage produced, what the consumer expects, the missing/extra delta, and a `--fork-role` hint for authoring a reshaping adapter. (No model is called — validation fails first.)

```bash
export AICHAT_CONFIG_DIR=/tmp/aichat-phase13-demo AICHAT_ROLES_DIR=/tmp/aichat-phase13-demo/roles
./target/debug/aichat --pipe --stage needs-shape "{\"text\":\"x\",\"summary\":\"y\"}" 2>&1
echo "(pipeline exit $?)"
```

```output
Error: Pipeline stage 1/1 (role 'needs-shape', model 'openai:gpt-4o-mini') failed: pipeline stage 1 input schema validation failed (role 'needs-shape'):
  Schema input validation failed:
  - "content" is a required property
  - "language" is a required property

  Stage 0 produced: { text, summary }
  Stage 1 expects: { content, language }
  Missing fields: content, language
  Extra fields: text, summary

  hint: shape mismatches between adjacent stages are usually fixed by a
        transform role between them. To start one:
        aichat --fork-role <parent> my-adapter

(pipeline exit 10)
```

## Verification & docs

- Code lives in `src/config/role.rs` (`render_forked_role`, `build_role_explanation`/`format_role_explanation`, `format_pipeline_input_schema_error`), dispatched from `run_fork_role`/`run_explain_role` in `src/main.rs`, with the 13B teaching error wired into `run_stage_inner` in `src/pipe.rs`.
- Integration: `tests/integration/authoring.sh` — 12 bats cases covering all four items, hermetic and cleaned up.
- User docs: `docs/features/authoring.md`.
- The guardrail examples (`assets/roles/guardrail-*.md`) leave `model:` commented out so they run on the user's default model.
