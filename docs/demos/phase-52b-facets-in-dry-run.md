# Phase 52B — Facets in --dry-run

*2026-06-08T01:16:43Z by Showboat 0.6.1*
<!-- showboat-id: 88c44803-0a39-4131-a0f0-df4e60b7e976 -->

Phase 52B formalizes the **facet taxonomy** — the six closed families an entity can carry (Know · Act · Shape · Govern · Compose · Judge) — and surfaces it in the `--dry-run` preview. Each family is tagged **owned** or **referenced** per the *backing-gates-ownership* rule (§5.2 of `docs/architecture/entity-model.md`): a file-backed role can *own* declarative facets (schemas, composition, metrics) but only *reference* executable/stateful ones (tools, knowledge). No authoring-format change; the preview emits to stderr before any model call.

A **mixed** role: it owns *Shape* (it declares an `output_schema`) but only references *Act* (it points at a tool via `use_tools`). We author it in an isolated roles dir so the demo is self-contained.

```bash
export AICHAT_ROLES_DIR="$(mktemp -d)"
cat > "$AICHAT_ROLES_DIR/demo-mixed.md" <<'ROLE'
---
description: Demo facet fixture — owns Shape, references Act.
use_tools: fs_read
output_schema:
  type: object
  properties:
    issues: { type: array }
---
You review code.
ROLE
cat "$AICHAT_ROLES_DIR/demo-mixed.md"
```

```output
---
description: Demo facet fixture — owns Shape, references Act.
use_tools: fs_read
output_schema:
  type: object
  properties:
    issues: { type: array }
---
You review code.
```

Running it under `--dry-run` (no model call) surfaces a `facets:` line on stderr. Families render in closed-taxonomy order, each tagged — `Act(ref), Shape(owned)`:

```bash
export AICHAT_ROLES_DIR="$(mktemp -d)"
cat > "$AICHAT_ROLES_DIR/demo-mixed.md" <<'ROLE'
---
description: owns Shape, references Act.
use_tools: fs_read
output_schema:
  type: object
  properties:
    issues: { type: array }
---
You review code.
ROLE
./target/debug/aichat -r demo-mixed --dry-run "{\"x\":1}" 2>&1 1>/dev/null | grep "facets:"
```

```output
  facets: Act(ref), Shape(owned)
```

A **bare** role carries no facets, so the line is **omitted entirely** — the preview stays clean. We assert its absence:

```bash
export AICHAT_ROLES_DIR="$(mktemp -d)"
cat > "$AICHAT_ROLES_DIR/demo-bare.md" <<'ROLE'
---
description: no facets.
---
Say hi.
ROLE
if ./target/debug/aichat -r demo-bare --dry-run "hello" 2>&1 1>/dev/null | grep -q "facets:"; then
  echo "FAIL: facets line present for bare role"
else
  echo "ok: no facets line for bare role"
fi
```

```output
ok: no facets line for bare role
```

**Why it matters.** `facets()` is the capability-introspection surface (Phase 52A) that lets `--dry-run`, MCP capability negotiation, and uniform resolution (52C) reason about an entity *without branching on its concrete Prompt/Role/Agent/Macro variant*. The owned-vs-referenced tag makes the backing-gates-ownership invariant visible at author time — before a single token is spent.
