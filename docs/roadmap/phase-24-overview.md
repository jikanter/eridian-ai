# Phase 24: Regression Testing & Prompt Distillation : Overview - Epic 8

| Item | Description | Status |
|---|---|---|
| 24A | Role regression testing (replay saved input/output pairs, check metrics) | -- |
| 24B | Role-as-judge pattern (document + example roles) | -- |
| 24C | Prompt distillation pipeline (expensive model -> validate -> append examples to cheap role) | -- |
| 24D | `showboat validate-role` integration (replay test cases from trace log) | -- |

**24A Design — Role Regression Testing:**

When you edit a role's prompt, you have no way to verify it still produces acceptable output. Regression testing replays saved input/output pairs from the trace log:

```bash
$ aichat --test-role summarizer

Replaying 5 recorded invocations for 'summarizer':
  [1/5] input: "The auth flow..."     metrics: 3/3 PASS   cost: $0.0004
  [2/5] input: "OAuth2 requires..."   metrics: 3/3 PASS   cost: $0.0004
  [3/5] input: "Session tokens..."    metrics: 2/3 FAIL   cost: $0.0004
    FAIL: under_500_words (output was 623 words)
  [4/5] input: "API gateway..."       metrics: 3/3 PASS   cost: $0.0004
  [5/5] input: "Rate limiting..."     metrics: 3/3 PASS   cost: $0.0004

Result: 4/5 passed (80%)  Total cost: $0.002
```

Test cases are extracted from the role's invocation history (Phase 23D). The `--save-test` flag captures the current invocation as a test case.

**24B Design — Role-as-Judge:**

```bash
$ aichat -r writer "Explain OAuth2" | aichat -r judge
```

A `judge` role with structured output:

```yaml
---
name: judge
description: Evaluate LLM output quality
output_schema:
  type: object
  properties:
    score: { type: integer, minimum: 1, maximum: 5 }
    reasoning: { type: string }
    pass: { type: boolean }
  required: [score, reasoning, pass]
---
Evaluate the following text for clarity, accuracy, and completeness.
Score 1-5. Pass if score >= 3.
__INPUT__
```

Two YAML files replace heavyweight eval frameworks (DeepEval, Confident AI).

**24C Design — Prompt Distillation Pipeline:**

Use an expensive model to generate high-quality outputs, then use those as few-shot examples for a cheap model:

```yaml
# roles/distill.md
---
name: distill
pipeline:
  - role: generate-with-expensive   # stage 1: expensive model generates
  - role: validate-output           # stage 2: check metrics
  - role: append-example            # stage 3: add passing examples to target role
---
```

This is what DSPy's BootstrapFinetune does, but approximated without fine-tuning — just example curation through existing pipeline mechanics.
