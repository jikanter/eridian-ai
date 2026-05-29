# Phase 13: Authoring & Teaching : Overview - Epic 3

**Status (2026-05-29):** **Done.** All four items shipped. User docs:
[features/authoring.md](../features/authoring.md). Tests:
`tests/integration/authoring.sh` (12 bats cases) plus unit tests in
`src/config/role.rs` (`schema_field_diff`, `format_field_set`) and `src/pipe.rs`
(`format_stage_input_mismatch`).

| Item | Description | Status |
|---|---|---|
| 13A | `--fork-role <source> <new-name>` (creates pre-populated `extends:` file) | **Done** |
| 13B | Schema mismatch errors as teaching moments (field delta + fork-role hint) | **Done** |
| 13C | Built-in guardrail role examples (PII detection, prompt injection, topic restriction) | **Done** |
| 13D | `--explain-role <name>` (human-readable description of what a role does and how it composes) | **Done** |

**Files touched:**
- `src/cli.rs` — `--fork-role` (2-arg) and `--explain-role` flags.
- `src/main.rs` — `run_fork_role` / `build_fork_role_content`,
  `run_explain_role` / `print_role_explanation` / `print_pipeline_node`;
  both wired into the `info_flag` short-circuit set and dispatched in `run()`.
- `src/config/role.rs` — `schema_field_diff` + `format_field_set` (13B field diff).
- `src/pipe.rs` — `format_stage_input_mismatch`, called from `run_stage_inner`
  when a stage's input schema rejects the prior stage's output (`stage_index`
  threaded through `run_stage` → `run_stage_inner`).
- `assets/roles/guardrail-{pii,injection,topic}.md` — embedded example roles.

**Notes for future work:**
- 13B's cross-stage "Stage N produced / Stage N+1 expects" framing fires on the
  *input-schema* boundary (the genuine producer→consumer mismatch). A stage's
  own *output-schema* failure still uses the terse validator message; enriching
  that path is a natural follow-on if it proves useful.
- The teaching error keeps the literal phrase `Schema input validation failed`
  so `classify_error` still maps it to the schema-validation family before the
  pipeline wrapper re-tags it as a `PipelineStage` error.

**13A Design — Fork Role:**

```bash
$ aichat --fork-role base-analyst my-analyst

Created roles/my-analyst.md:
  ---
  extends: base-analyst
  # model: claude:claude-sonnet-4-6     # override parent's model
  # temperature: 0.7                     # override parent's temperature
  # output_schema:                       # override parent's schema
  ---
  # Add your prompt additions here. Parent prompt is inherited.
```

This is the pattern that made Terraform modules composable in practice. The fork command turns a 5-minute file-editing task into a 5-second command, and teaches the user that `extends` exists.

**Files:** `src/cli.rs` (add `--fork-role` flag), `src/main.rs` (generate the file with commented-out parent fields).

**13B Design — Error Teaching:**

Current schema mismatch on pipeline failure:
```
error: pipeline stage 2 output schema validation failed
  /required/language: missing required property
```

After Phase 13B:
```
error: pipeline stage 2 output schema validation failed

  Stage 1 produced:     { "text": "...", "summary": "..." }
  Stage 2 expects:      { "content": "...", "language": "..." }

  Missing fields: content, language
  Extra fields: text, summary

  hint: Did you mean to add a transform role between stages 1 and 2?
        Try: aichat --fork-role json-transform my-adapter
```

**Files:** `src/config/role.rs` (enhance `validate_schema` error formatting), `src/pipe.rs` (pass both schemas to error formatter).

**13C Design — Guardrail Role Examples:**

Guardrails are not a new feature — they are a role authoring pattern. Ship 3 example roles in `assets/roles/` that demonstrate the pattern:

```yaml
# assets/roles/guardrail-pii.md
---
name: guardrail-pii
description: Detect and redact PII from text
model: deepseek:deepseek-chat    # cheap model sufficient
output_schema:
  type: object
  properties:
    safe: { type: boolean }
    redacted: { type: string }
    findings: { type: array, items: { type: string } }
  required: [safe, redacted]
---
Scan the following text for personally identifiable information (PII).
If PII is found, set safe=false and return the redacted version.
__INPUT__
```

Users compose guardrails into pipelines via existing mechanisms:
```yaml
pipeline:
  - role: guardrail-pii
  - role: my-actual-task
  - role: guardrail-topic
```

**Files:** `assets/roles/guardrail-pii.md`, `assets/roles/guardrail-injection.md`, `assets/roles/guardrail-topic.md`.
