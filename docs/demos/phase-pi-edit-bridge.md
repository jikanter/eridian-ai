# Editing aichat config from the pi REPL (/aichat-edit)

*2026-06-10T18:27:27Z by Showboat 0.6.1*
<!-- showboat-id: f8eb0361-8501-4fb1-bb8f-e1fb63a70290 -->

The legacy REPL's `.edit` family spawned `$EDITOR` on a YAML file. Pi owns the terminal, so the bridge re-exposes `.edit` as the `/aichat-edit <target>` slash command: it reads the current file text over HTTP, hands it to **pi's own in-TUI editor**, and POSTs the result back for aichat to persist and reload. Targets: `config`, `role`, `rag-docs`, `agent-config`. Sessions are excluded — pi owns that format and edits them via its native `/session`.

Below we drive the underlying `/v1/state/edit` endpoint against a live `aichat --serve` (the same routes the extension calls from pi).

```bash
PORT=8947
CFG=$(mktemp -d)
mkdir -p "$CFG/roles"
cat >"$CFG/config.yaml" <<'YAML'
model: openai:gpt-4o-mini
clients:
  - type: openai
    api_key: dummy
YAML
cat >"$CFG/roles/demo.md" <<'YAML'
---
model: openai:gpt-4o-mini
---

You are a TERSE assistant.
YAML

AICHAT_CONFIG_DIR="$CFG" AICHAT_BRIDGE_TOKEN=demo-token \
  ./target/debug/aichat --serve 127.0.0.1:$PORT >/tmp/edit_demo_serve.log 2>&1 &
SRV=$!
until curl -s -o /dev/null "http://127.0.0.1:$PORT/health"; do sleep 0.2; done
AUTH=(-H "Authorization: Bearer demo-token")
CT=(-H "Content-Type: application/json")
H="http://127.0.0.1:$PORT"

echo "# /aichat-edit config  ->  GET reads the live config.yaml"
curl -s "${AUTH[@]}" "$H/v1/state/edit?target=config" | jq -r '"target=\(.target)", .content'

echo "# switch to role 'demo', then GET its file through the edit surface"
curl -s -o /dev/null "${AUTH[@]}" "${CT[@]}" --data '{"name":"demo"}' "$H/v1/state/role"
curl -s "${AUTH[@]}" "$H/v1/state/edit?target=role" | jq -r '"label=\(.label)", .content'

echo "# edit the role body and POST it back  ->  saved AND reloaded live"
curl -s "${AUTH[@]}" "${CT[@]}" \
  --data '{"target":"role","content":"---\nmodel: openai:gpt-4o-mini\n---\n\nYou are an EXUBERANT assistant.\n"}' \
  "$H/v1/state/edit" | jq -r '.info'
echo -n "  live role prompt now: "
curl -s "${AUTH[@]}" "$H/v1/state/info?of=role" | jq -r '.info' | grep -io 'You are an[A-Za-z ]*assistant.'

echo "# guard rails (HTTP status codes)"
printf '  session is pi-native -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$H/v1/state/edit?target=session")"
printf '  missing target       -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$H/v1/state/edit")"
printf '  no active agent      -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$H/v1/state/edit?target=agent-config")"
printf '  unsupported method   -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "${AUTH[@]}" "$H/v1/state/edit")"

kill $SRV 2>/dev/null
wait $SRV 2>/dev/null
rm -rf "$CFG"
exit 0

```

```output
# /aichat-edit config  ->  GET reads the live config.yaml
target=config
model: openai:gpt-4o-mini
clients:
  - type: openai
    api_key: dummy

# switch to role 'demo', then GET its file through the edit surface
label=active role
---
model: openai:gpt-4o-mini
---

You are a TERSE assistant.

# edit the role body and POST it back  ->  saved AND reloaded live
Saved and reloaded role 'demo'.
  live role prompt now: You are an EXUBERANT assistant.
# guard rails (HTTP status codes)
  session is pi-native -> 400
  missing target       -> 400
  no active agent      -> 409
  unsupported method   -> 405
```
