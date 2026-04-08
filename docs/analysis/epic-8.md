# Epic 8: Feedback Loop

**Created:** 2026-04-07
**Status:** Planning
**Depends on:** Epic 2 Phase 8A1 (run log infrastructure), Epic 4 Phase 14A (capabilities field)
**Phases:** 23-24
**Source:** Theme 2 — ML Engineer + ML App Engineer analyses

---

## Motivation

Roles have no metrics, no regression testing, no A/B comparison. Every role invocation should be a scored data point. This epic closes the gap between "prompt template" and "optimizable, testable, versionable component."

---

## Phases

### Phase 23: Role Evaluation

| Item | Description |
|---|---|
| 23A | `metrics:` field on roles (shell commands that score output, exit 0=pass / 1=fail) |
| 23B | `--compare` flag (run input through two roles, show results side-by-side with cost) |
| 23C | Cost attribution by role in run log (tag each pipeline stage in JSONL) |
| 23D | Role invocation history (append scored records to per-role ledger) |

### Phase 24: Regression Testing & Prompt Distillation

| Item | Description |
|---|---|
| 24A | Role regression testing (replay saved input/output pairs, check metrics) |
| 24B | Role-as-judge pattern (document + example roles) |
| 24C | Prompt distillation pipeline (expensive model → validate → append examples to cheap role) |
| 24D | `showboat validate-role` integration (replay test cases from trace log) |

---

## Key Designs

**23A — Metrics Field:**

```yaml
metrics:
  - name: valid_json
    shell: "jq . >/dev/null 2>&1"
  - name: under_500_words
    shell: "test $(wc -w < /dev/stdin) -lt 500"
  - name: has_required_fields
    shell: "jq -e '.summary and .key_points' >/dev/null 2>&1"
```

Each metric receives output on stdin. Results recorded in JSONL run log alongside cost/tokens.

**23B — Compare Flag:**

```bash
$ echo "Review this code" | aichat --compare summarizer-v1 summarizer-v2
```

Runs both roles on same input, shows side-by-side output, metrics, and cost ratio.

**24A — Regression Testing:**

```bash
$ aichat --test-role summarizer    # replay recorded invocations, check metrics
```

Test cases extracted from invocation history (23D). `--save-test` captures current invocation.

**24B — Role-as-Judge:** Pipe output through a `judge` role with structured `{score, reasoning, pass}` output. Two YAML files replace heavyweight eval frameworks.

**24C — Prompt Distillation:** Pipeline pattern: expensive model generates → validate → append passing examples as few-shot to cheap role. DSPy's BootstrapFinetune approximated without fine-tuning.

Files: `src/config/role.rs` (metrics), `src/main.rs` (evaluate, compare, test-role), `src/cli.rs` (flags), `src/utils/trace.rs` (metric events), `src/pipe.rs` (cost attribution).

Full designs: [ROADMAP.md, Epic 8 section](../ROADMAP.md#epic-8-feedback-loop-new)

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Built-in eval framework (DeepEval-style) | Shell-based metrics + role-as-judge covers needs. No new runtime dependency. |
| Automatic prompt optimization | Research problem. Distillation pipeline (24C) is the practical proxy. |
| A/B testing infrastructure with traffic splitting | Wrong layer for CLI tool. `--compare` is manual A/B. |
