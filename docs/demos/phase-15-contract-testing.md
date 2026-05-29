# Phase 15: Contract Testing

*2026-05-29T17:47:18Z by Showboat 0.6.1*
<!-- showboat-id: 8348a447-64a3-423b-a29b-575ad605320e -->

Phase 15 makes pipeline schema contracts checkable before any model call. A pipeline chains roles; each role declares an `input_schema` and `output_schema`. Stage N's output feeds stage N+1's input, so the two schemas must line up. `aichat --check` validates that statically — deterministic, zero-token.

Three items:
- 15A — stage existence + model/tool capability checks (these already run implicitly before every pipeline; `--check` exposes them without executing).
- 15B — cross-stage JSON Schema containment: a document valid under stage N's `output_schema` must also be valid under stage N+1's `input_schema`. Implemented in `src/config/preflight.rs::schema_containment`.
- 15C — the `--check` flag: validate a role or pipeline definition and exit 0 (valid), 3 (invalid), or 2 (usage), with optional `-o json`.

## Setup — fixture roles with structured ports

We sandbox the fixtures in a temp roles dir via `AICHAT_ROLES_DIR` so the demo never touches your real config. `p15-extract` emits `{text, metadata}`; `p15-review` requires `{content, language}` (deliberately incompatible); `p15-producer`/`p15-consumer` are a compatible pair.

```bash
ROLES=/tmp/aichat-phase15-roles
mkdir -p "$ROLES"

cat > "$ROLES/p15-extract.md" <<EOF
---
output_schema:
  type: object
  properties:
    text: { type: string }
    metadata: { type: object }
  required: [text, metadata]
---
Extract structured fields from the input.
EOF

cat > "$ROLES/p15-review.md" <<EOF
---
input_schema:
  type: object
  properties:
    content: { type: string }
    language: { type: string }
  required: [content, language]
---
Review the content.
EOF

cat > "$ROLES/p15-format.md" <<EOF
---
input_schema:
  type: object
  properties:
    issues: { type: array }
  required: [issues]
---
Format the issues.
EOF

cat > "$ROLES/p15-producer.md" <<EOF
---
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string }
  required: [issues, severity]
---
Produce issues with a severity.
EOF

cat > "$ROLES/p15-consumer.md" <<EOF
---
input_schema:
  type: object
  properties:
    issues: { type: array }
  required: [issues]
---
Consume the issues.
EOF

cat > "$ROLES/p15-bad-pipe.md" <<EOF
---
pipeline:
  - role: p15-extract
  - role: p15-review
  - role: p15-format
---
EOF

cat > "$ROLES/p15-good-pipe.md" <<EOF
---
pipeline:
  - role: p15-producer
  - role: p15-consumer
---
EOF

ls "$ROLES"
```

```output
p15-bad-pipe.md
p15-consumer.md
p15-extract.md
p15-format.md
p15-good-pipe.md
p15-producer.md
p15-review.md
```

## 15B/15C — an incompatible pipeline fails the check

`p15-bad-pipe` chains extract → review → format. Stage 1 emits `{text, metadata}`; stage 2 requires `{content, language}`. No document valid under stage 1's output is valid under stage 2's input, so the boundary is a hard FAIL. (Stage 2 declares no `output_schema`, so the 2→3 boundary is a non-fatal WARN: free text may not satisfy stage 3's structured input.)

```bash
AICHAT_ROLES_DIR=/tmp/aichat-phase15-roles ./target/debug/aichat --check -r p15-bad-pipe </dev/null
echo "exit: $?"
```

```output
Pipeline: p15-bad-pipe (3 stages)
  1. p15-extract              in: any                    out: json{text, metadata}
  2. p15-review               in: json{content, language} out: text
  3. p15-format               in: json{issues}           out: text

FAIL: stage 1 (p15-extract) → stage 2 (p15-review)
  Missing: content, language
  Extra:   text, metadata
  Suggestion: add a transform stage, or align the schemas so the upstream output satisfies the downstream input.

WARN: stage 2 (p15-review) → stage 3 (p15-format)
  upstream stage emits free text (no output_schema); downstream input_schema may reject non-JSON output

check failed: 1 incompatible boundary
exit: 3
```

## A compatible pipeline passes

`p15-good-pipe` chains producer → consumer. Stage 1 emits `{issues, severity}` (both required); stage 2 needs only `{issues}`. Every stage-1 document satisfies stage 2 — `severity` is an allowed extra — so the boundary is compatible and the check passes.

```bash
AICHAT_ROLES_DIR=/tmp/aichat-phase15-roles ./target/debug/aichat --check -r p15-good-pipe </dev/null
echo "exit: $?"
```

```output
Pipeline: p15-good-pipe (2 stages)
  1. p15-producer             in: any                    out: json{issues, severity}
  2. p15-consumer             in: json{issues}           out: text

OK: 1 boundary checked
check passed
exit: 0
```

## Machine-readable output for CI

`-o json` emits the full report — per-stage ports and per-boundary verdicts — for gating in CI. `valid: false` plus a non-zero exit makes it a one-liner in a pre-commit hook or pipeline step.

```bash
AICHAT_ROLES_DIR=/tmp/aichat-phase15-roles ./target/debug/aichat --check -r p15-bad-pipe -o json </dev/null
echo "exit: $?"
```

```output
{
  "valid": false,
  "target": "p15-bad-pipe",
  "kind": "pipeline",
  "stages": [
    {
      "position": 1,
      "role": "p15-extract",
      "input": "any",
      "output": "json{text, metadata}"
    },
    {
      "position": 2,
      "role": "p15-review",
      "input": "json{content, language}",
      "output": "text"
    },
    {
      "position": 3,
      "role": "p15-format",
      "input": "json{issues}",
      "output": "text"
    }
  ],
  "boundaries": [
    {
      "from": "p15-extract",
      "to": "p15-review",
      "status": "fail",
      "missing": [
        "content",
        "language"
      ],
      "extra": [
        "text",
        "metadata"
      ],
      "forbidden": [],
      "type_mismatches": [],
      "notes": []
    },
    {
      "from": "p15-review",
      "to": "p15-format",
      "status": "warn",
      "missing": [],
      "extra": [],
      "forbidden": [],
      "type_mismatches": [],
      "notes": [
        "upstream stage emits free text (no output_schema); downstream input_schema may reject non-JSON output"
      ]
    }
  ],
  "non_sequential": false,
  "errors": []
}
exit: 3
```

## Ad-hoc pipelines and standalone roles

`--check` also works on an ad-hoc `--pipe --stage A --stage B` chain (no role file needed) and on a single role (validates capability + that its declared schemas are valid JSON Schema, and prints its ports).

```bash
AICHAT_ROLES_DIR=/tmp/aichat-phase15-roles ./target/debug/aichat --check --pipe --stage p15-extract --stage p15-review </dev/null
echo "exit: $?"
```

```output
Pipeline: <pipeline> (2 stages)
  1. p15-extract              in: any                    out: json{text, metadata}
  2. p15-review               in: json{content, language} out: text

FAIL: stage 1 (p15-extract) → stage 2 (p15-review)
  Missing: content, language
  Extra:   text, metadata
  Suggestion: add a transform stage, or align the schemas so the upstream output satisfies the downstream input.

check failed: 1 incompatible boundary
exit: 3
```

```bash
AICHAT_ROLES_DIR=/tmp/aichat-phase15-roles ./target/debug/aichat --check -r p15-extract </dev/null
echo "exit: $?"
```

```output
Role: p15-extract
  input:  any
  output: json{text, metadata}
check passed
exit: 0
```

## Cleanup

Remove the sandbox roles dir. The check is deterministic: re-running this document reproduces every output above verbatim.

```bash
rm -rf /tmp/aichat-phase15-roles && echo "cleaned up"
```

```output
cleaned up
```
