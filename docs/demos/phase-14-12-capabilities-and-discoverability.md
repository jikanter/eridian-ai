# Phase 14 + 12: Capability Manifests & Discoverability

*2026-05-04T20:17:48Z by Showboat 0.6.1*
<!-- showboat-id: 799fea82-4ba8-4233-8f95-86877ed6b835 -->

Phase 14 (Capability Manifests, Epic 4) and Phase 12 (Discoverability & Previews, Epic 3) ship together. Together they let you see what's available before spending tokens, and what a role will do before sending it.

Eight items across the two phases:
- 14A — `capabilities:` field on roles. Free-form tags for discovery (`code-review`, `summarization`).
- 14B — Port type summaries derived from `input_schema` / `output_schema` (`json{a, b, c}`, `text`, `array`, `any`).
- 14C — `Config::find_roles_by_capability` and `find_roles_by_port` resolvers.
- 14D — `--find-role` CLI flag with `--capability`, `--accepts`, `--produces` filters.
- 12A — `--dry-run` shows the resolved role on stderr (extends/include/model/tools/ports/capabilities).
- 12B — Pipeline diagram in `--dry-run` (numbered stages with model per row).
- 12C — `--list-roles --verbose` and `--find-role --verbose` share one renderer.
- 12D — REPL composition summary after `.role <name>`.

## Setup — a fixture role with capabilities and structured ports

A reviewer role declaring three capabilities, an object input port, and an object output port. The same fixture exercises every Phase 14 / 12 surface.

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
mkdir -p "$ROLES_DIR"
cat > "$ROLES_DIR/p14-reviewer.md" <<'EOF'
---
description: Phase 14/12 reviewer fixture
capabilities: [code-review, security-audit]
input_schema:
  type: object
  properties:
    code: { type: string }
    language: { type: string }
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string }
---
You are a code reviewer.
EOF
cat > "$ROLES_DIR/p14-summarize.md" <<'EOF'
---
description: Phase 14/12 summarize fixture
capabilities: [summarization]
---
Summarize __INPUT__.
EOF
echo "Wrote $ROLES_DIR/p14-reviewer.md and p14-summarize.md"
```

```output
Wrote /Users/admin/Library/Application Support/aichat/roles/p14-reviewer.md and p14-summarize.md
```

## 14D — `--find-role` discovers by capability tag

```bash
./target/debug/aichat --find-role --capability code-review
```

```output
p14-reviewer
```

Case-insensitive substring match — `AUDIT` finds `security-audit`.

```bash
./target/debug/aichat --find-role --capability AUDIT
```

```output
p14-reviewer
```

## 14D — port-type filters narrow by what a role accepts and produces

`--accepts json --produces json` selects roles whose input and output ports are object-shaped. Bare `json` matches both `json` and `json{...}`.

```bash
./target/debug/aichat --find-role --accepts json --produces json
```

```output
p14-reviewer
```

## 14D + 12C — `--verbose` is shared between `--find-role` and `--list-roles`

Same renderer powers both. Each row carries port signatures, tool count (when nonzero), capability tags, and pipeline depth (when present).

```bash
./target/debug/aichat --find-role --capability code-review --verbose
```

```output
  p14-reviewer  in: json{code, language}  out: json{issues, severity}  capabilities: [code-review, security-audit]
```

## 14D — `-o json` emits structured records for tooling

Verbose mode in JSON adds `capabilities`, `input`, and `output` to every record. Useful for piping through `jq` or feeding into another tool.

```bash
./target/debug/aichat --find-role --capability code-review --verbose -o json
```

```output
[
  {
    "name": "p14-reviewer",
    "description": "Phase 14/12 reviewer fixture",
    "model": "default",
    "tools": [],
    "capabilities": [
      "code-review",
      "security-audit"
    ],
    "input": "json{code, language}",
    "output": "json{issues, severity}"
  }
]
```

## 14D — `--find-role` rejects an empty filter set

Without `--capability`, `--accepts`, or `--produces`, `--find-role` is just a less-friendly `--list-roles`. The CLI tells you so instead of silently echoing the universe.

```bash
./target/debug/aichat --find-role 2>&1; echo "(exit $?)"
```

```output
Error: --find-role requires at least one of --capability, --accepts, --produces
(exit 1)
```
