# Discovery: flags & docs from the pi REPL

*2026-06-08T23:51:17Z by Showboat 0.6.1*
<!-- showboat-id: c4c9d419-5af7-4556-9f8c-0f70589526a6 -->

aichat's discovery surface answers "what can this thing do?" without leaving the REPL. The pi bridge exposes two read-only slash commands — `/aichat-flags` and `/aichat-docs` — each backed by an HTTP route on the bridge server. Flags are introspected live from the Clap command tree; feature docs are embedded into the binary at build time.

```bash
PORT=8931
AICHAT_BRIDGE_TOKEN=demo-token ./target/debug/aichat --serve 127.0.0.1:$PORT >/tmp/disc_demo_serve.log 2>&1 &
SRV=$!
until curl -s -o /dev/null "http://127.0.0.1:$PORT/health"; do sleep 0.2; done
AUTH=(-H "Authorization: Bearer demo-token")
H="http://127.0.0.1:$PORT"

echo "# /aichat-flags rag  ->  GET /v1/discovery/flags?q=rag"
curl -s "${AUTH[@]}" "$H/v1/discovery/flags?q=rag" \
  | jq -c '.flags[] | {flag: ("--" + .long), takes_value}'

echo
echo "# /aichat-docs  ->  GET /v1/discovery/docs   (bundled feature docs)"
curl -s "${AUTH[@]}" "$H/v1/discovery/docs" \
  | jq -r '"\(.count) docs:", (.docs[] | "  \(.name) — \(.title)")'

echo
echo "# /aichat-docs discovery  ->  GET /v1/discovery/docs?name=discovery"
curl -s "${AUTH[@]}" "$H/v1/discovery/docs?name=discovery" \
  | jq -r '.content' | head -n 1

echo
echo "# auth + method gating"
printf 'no token        -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' "$H/v1/discovery/flags")"
printf 'wrong method    -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' -X POST "${AUTH[@]}" "$H/v1/discovery/flags")"
printf 'unknown doc     -> %s\n' "$(curl -s -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$H/v1/discovery/docs?name=nope")"

kill $SRV 2>/dev/null
wait $SRV 2>/dev/null
exit 0
```

```output
# /aichat-flags rag  ->  GET /v1/discovery/flags?q=rag
{"flag":"--rag","takes_value":true}
{"flag":"--rebuild-rag","takes_value":false}
{"flag":"--list-rags","takes_value":false}
{"flag":"--knowledge-stat","takes_value":true}

# /aichat-docs  ->  GET /v1/discovery/docs   (bundled feature docs)
15 docs:
  agents — Agents
  authoring — Authoring & Teaching Roles
  auto-memory — Auto-Memory (`memory/MEMORY.md`)
  contract-testing — Contract Testing (`--check`)
  discovery — Discovery: find flags and docs from the REPL
  instructions — Important Instructions
  knowledge — Knowledge (Native RAG)
  macros — Macros
  pi-repl-migration — Migrating to the pi REPL
  pipeline-isolation — Pipeline Stage Config Isolation
  playground — The browser playground
  repl-pi — REPL via pi
  role-evaluation — Role Evaluation
  server — The aichat HTTP server
  typed-input — Typed Input

# /aichat-docs discovery  ->  GET /v1/discovery/docs?name=discovery
# Discovery: find flags and docs from the REPL

# auth + method gating
no token        -> 401
wrong method    -> 405
unknown doc     -> 404
```

Both halves are always accurate by construction: the flag list is introspected from the live Clap command tree at request time (no second catalog to drift from `src/cli.rs`), and the feature docs are embedded into the binary at build time, so `/aichat-docs` works for an installed aichat with no source tree on disk. See [discovery.md](../features/discovery.md).
