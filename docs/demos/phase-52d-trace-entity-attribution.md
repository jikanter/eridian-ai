# Phase 52D — Trace Entity Attribution

*2026-06-10T21:21:08Z by Showboat 0.6.1*
<!-- showboat-id: bc5b27c3-1ba9-4b87-92f4-86f1d559845e -->

**Phase 52D** makes each keystone trace carry *who ran*: the resolved `entity_id`
plus the **facet set actually used**, written into the SPEC-001 `session.start`
event. This is the stable GROUP BY key Phase 49 attribution reads — emitted once
per turn, before any provider call.

Both fields are **additive optional** per SPEC-001 §5, so `schema_version` stays
`"0.1"` (no consumer break). Facets serialize as sorted `Family:ownership`
tokens — a machine key, not a display string.

The setup below points aichat at a dead provider on purpose: `session.start` is
emitted at turn start, so the attribution lands in the trace even though the
call then fails. Only the stable fields are extracted, so this demo is evergreen.

### 1. A role with two facets

`use_tools` makes the role *reference* the **Act** family; `output_schema`
makes it *own* the **Shape** family (the §5.2 backing-gates-ownership rule).

```bash
D=/tmp/aichat-52d-demo; rm -rf "$D"; mkdir -p "$D/cfg/roles" "$D/trace"
cat > "$D/cfg/config.yaml" <<YAML
model: dead:test-model
clients:
  - type: openai-compatible
    name: dead
    api_base: http://127.0.0.1:1/v1
    models:
      - name: test-model
YAML
cat > "$D/cfg/roles/attrib-demo.md" <<YAML
---
use_tools: fs_read
output_schema:
  type: object
---
You are a demo role.
YAML
cat "$D/cfg/roles/attrib-demo.md"
```

```output
---
use_tools: fs_read
output_schema:
  type: object
---
You are a demo role.
```

### 2. A traced turn attributes entity + facets

Run with `--trace`. The provider is dead, so the turn fails — but `session.start`
already carries the attribution. We extract only the stable fields.

```bash
BIN=/Volumes/ExternalData/admin/Developer/Projects/aichat/.claude/worktrees/keystone-trace-format/target/debug/aichat
D=/tmp/aichat-52d-demo
AICHAT_CONFIG_DIR="$D/cfg" AICHAT_TRACE_DIR="$D/trace" \
  "$BIN" --trace --role attrib-demo "hi" >/dev/null 2>&1 || true
TURN=$(ls "$D"/trace/traces/turn-*.jsonl | head -1)
grep -h "\"type\":\"session.start\"" "$TURN" | python3 -c "
import sys,json
d=json.loads(sys.stdin.read())[\"data\"]
print(json.dumps({\"role\":d[\"role\"],\"entity_id\":d[\"entity_id\"],\"facets\":d[\"facets\"]}, indent=2))
"
```

```output
{
  "role": "attrib-demo",
  "entity_id": "attrib-demo",
  "facets": [
    "Act:referenced",
    "Shape:owned"
  ]
}
```

### 3. A bare role degrades cleanly

No facets → `facets: []`; `entity_id` still present. Consumers predating 52D
simply ignore the new keys.

```bash
BIN=/Volumes/ExternalData/admin/Developer/Projects/aichat/.claude/worktrees/keystone-trace-format/target/debug/aichat
D=/tmp/aichat-52d-demo
printf -- "---\nmodel: dead:test-model\n---\nbare.\n" > "$D/cfg/roles/bare.md"
rm -rf "$D/trace"; mkdir -p "$D/trace"
AICHAT_CONFIG_DIR="$D/cfg" AICHAT_TRACE_DIR="$D/trace" \
  "$BIN" --trace --role bare "hi" >/dev/null 2>&1 || true
TURN=$(ls "$D"/trace/traces/turn-*.jsonl | head -1)
grep -h "\"type\":\"session.start\"" "$TURN" | python3 -c "
import sys,json
v=json.loads(sys.stdin.read())
d=v[\"data\"]
print(json.dumps({\"schema_version\":v[\"schema_version\"],\"entity_id\":d[\"entity_id\"],\"facets\":d[\"facets\"]}, indent=2))
"
```

```output
{
  "schema_version": "0.1",
  "entity_id": "bare",
  "facets": []
}
```

**Result.** The keystone trace now answers *whose capabilities ran*, with a
stable machine key, at zero schema-version cost. Phase 49 attribution joins on
`entity_id` + `facets`; the synthesized-role facet view (agents) is refined by
Phase 52C. Pairs with the still-open transport-fidelity work (the in-process
`reqwest` interceptor) that answers *what crossed the wire*.
