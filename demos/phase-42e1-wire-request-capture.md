# Phase 42E-1 — Wire-True Request Capture

*2026-06-11T16:03:47Z by Showboat 0.6.1*
<!-- showboat-id: de67111f-b430-47f7-8840-dd69d8b9a17a -->

**Phase 42E-1** moves `provider.request` capture to the **`reqwest` boundary**.
Before, the event's body was a pre-send stub (`{"text": …}`) reconstructed from
aichat's intent. Now `retry::send` captures the **actual serialized wire body and
endpoint** just before dispatch, and the active trace turn drains it — so
`messages_hash` is wire-true (the key Phase 45D/46C replay correlation needs).

This is the in-process realization of EVAL-001 §5 (ground truth at the transport
boundary, **without** mitmproxy). Capture is guarded on an active trace turn, so
tracing-off — the default — pays nothing on the request hot path.

The demo points at a dead provider: the request is fully serialized and dispatched
(so it is captured) before the connection fails. Only stable structural fields are
extracted (the assembled system prompt embeds environment-specific memory), so the
demo is evergreen.

### provider.request now carries the real wire body

Run a traced turn, then resolve the `messages_hash` blob and print stable
structural fields: the real endpoint, model, message roles, and user text — the
payload that actually crossed (would cross) the wire, not a stub.

### provider.request now carries the real wire body

Run a traced turn (a role with `output_schema`, which uses the non-streaming
path — the single `retry::send` chokepoint 42E-1 captures). Resolve the
`messages_hash` blob and print stable structural fields: real endpoint, model,
message roles, user text — the payload that crossed the wire, not a stub.

```bash
BIN=/Volumes/ExternalData/admin/Developer/Projects/aichat/.claude/worktrees/keystone-trace-format/target/debug/aichat
D=/tmp/aichat-42e1-demo; rm -rf "$D"; mkdir -p "$D/cfg/roles" "$D/trace"
cat > "$D/cfg/config.yaml" <<YAML
model: dead:test-model
clients:
  - type: openai-compatible
    name: dead
    api_base: http://127.0.0.1:1/v1
    models:
      - name: test-model
YAML
cat > "$D/cfg/roles/wire.md" <<YAML
---
output_schema:
  type: object
---
You are a demo role.
YAML
AICHAT_CONFIG_DIR="$D/cfg" AICHAT_TRACE_DIR="$D/trace" \
  "$BIN" --trace --role wire "hello wire" >/dev/null 2>&1 || true
TURN=$(ls "$D"/trace/traces/turn-*.jsonl | head -1)
python3 - "$D" "$TURN" <<\PY
import sys,json,os
demo,turn=sys.argv[1],sys.argv[2]
req=[json.loads(l) for l in open(turn) if json.loads(l)["type"]=="provider.request"][0]["data"]
h=req["messages_hash"].split(":")[1]
body=json.load(open(os.path.join(demo,"trace","blobs",h[:2],h[2:4],h)))
print("endpoint:          ", req["endpoint"])
print("model:             ", body["model"])
print("message_roles:     ", [m["role"] for m in body["messages"]])
print("user_message:      ", [m["content"] for m in body["messages"] if m["role"]=="user"][0])
print("is_wire_not_stub:  ", req["endpoint"].endswith("/chat/completions") and "model" in body)
PY
```

```output
endpoint:           http://127.0.0.1:1/v1/chat/completions
model:              test-model
message_roles:      ['system', 'user']
user_message:       hello wire
is_wire_not_stub:   True
```

### The streaming path is honest about its limit

Streaming bypasses `retry::send` (it uses `.eventsource()`), so there is no
capture and `provider.request` falls back to the input-text stub — `endpoint`
empty, body `{"text": …}`. This is the documented **42E-3** boundary, not a
silent gap. A bare role (no `output_schema`) takes the streaming path:

```bash
BIN=/Volumes/ExternalData/admin/Developer/Projects/aichat/.claude/worktrees/keystone-trace-format/target/debug/aichat
D=/tmp/aichat-42e1-demo
printf -- "---\nmodel: dead:test-model\n---\nbare.\n" > "$D/cfg/roles/streamy.md"
rm -rf "$D/trace"; mkdir -p "$D/trace"
AICHAT_CONFIG_DIR="$D/cfg" AICHAT_TRACE_DIR="$D/trace" \
  "$BIN" --trace --role streamy "hi" >/dev/null 2>&1 || true
TURN=$(ls "$D"/trace/traces/turn-*.jsonl | head -1)
python3 - "$TURN" <<\PY
import sys,json
turn=sys.argv[1]
req=[json.loads(l) for l in open(turn) if json.loads(l)["type"]=="provider.request"][0]["data"]
print("endpoint:  ", repr(req["endpoint"]), "(empty -> stub, streaming)")
print("body_bytes:", req["request_body_bytes"], "(small -> input-text stub)")
PY
```

```output
endpoint:   '' (empty -> stub, streaming)
body_bytes: 13 (small -> input-text stub)
```

**Result.** On the non-streaming path `provider.request` is now wire-true:
real endpoint + the exact serialized body, so `messages_hash` is a stable replay
key (Phase 45D/46C). In-process, no mitmproxy, zero hot-path cost when tracing is
off. Streaming wire capture + per-attempt response capture follow in 42E-2/3.
