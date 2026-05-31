# Phase 16: Server Hardening

*2026-05-29T18:03:35Z by Showboat 0.6.1*
<!-- showboat-id: 9817f978-3afe-444f-b502-4fa77dc44cc8 -->

Phase 16 finishes Epic 5's server surface: the OpenAI-compatible `--serve` server gains the production-safety knobs it was missing. Every addition is opt-in — a plain `aichat --serve` behaves exactly as before. Items A-E are server changes in `src/serve.rs` (config keys parse in `src/config/mod.rs`); 16I is a one-line-class fix in the embedded playground. F/G/H shipped earlier for Phase 20 federation.

**16A** configurable CORS, **16B** optional bearer-token auth, **16C** `GET /health`, **16D** streaming usage chunk, **16E** `POST /v1/reload`, **16I** playground unfreeze. None of them touch the Phase 13 (authoring) or Phase 15 (contract-testing) substrate.

## The config surface (16A/16B)

Three new keys land in `Config`, all defaulting to off so existing setups are untouched:

```bash
grep -nE "serve_cors_origins|serve_cors_allow_all|serve_api_key" src/config/mod.rs | head -6
```

```output
246:    /// bundled playground/arena). `serve_cors_allow_all` overrides the list
249:    pub serve_cors_origins: Option<Vec<String>>,
251:    pub serve_cors_allow_all: bool,
254:    /// `Authorization: Bearer <serve_api_key>`. When unset, no auth (the
257:    pub serve_api_key: Option<String>,
436:            serve_cors_origins: None,
```

## The serve.rs surface

The new HTTP behaviors are small, testable units: a `CorsPolicy` (replaces the hardcoded localhost-only gate), a constant-time `check_api_key`, the `/health` and `/v1/reload` handlers, and the streaming `usage` chunk builder.

```bash
grep -nE "fn from_config|fn allows|fn check_api_key|fn health\(|fn reload_endpoint|fn create_usage_frame|fn build_usage_value" src/serve.rs
```

```output
363:    fn health(&self) -> Result<AppResponse> {
381:    fn reload_endpoint(&self) -> Result<AppResponse> {
1881:    fn from_config(config: &Config) -> Self {
1889:    fn allows(&self, origin: &str) -> bool {
1899:fn check_api_key(configured: Option<&str>, auth_header: Option<&str>) -> bool {
2040:fn create_usage_frame(id: &str, model: &str, created: i64, usage: &Value) -> Frame<Bytes> {
2054:fn build_usage_value(client: &dyn Client, input_tokens: u64, output_tokens: u64) -> Value {
```

## Unit coverage

The new HTTP behaviors decompose into small pure units, each pinned by a
unit test: the CORS allow/deny decision, the constant-time bearer-key check,
`stream_options` parsing, and the shape of the trailing usage frame. They run
without a socket.

```bash
# The CORS policy, the api-key check, stream_options parsing, and the usage
# frame are small pure units, each pinned by a unit test.
out=$(cargo test --bin aichat serve::tests 2>&1)
echo "$out" | grep -E 'serve::tests::(cors_|auth_|stream_options_|usage_frame)' | sed 's/^test //' | sort
echo "---"
echo "$out" | grep -oE '[0-9]+ passed; [0-9]+ failed; [0-9]+ ignored'
```

```output
serve::tests::auth_passes_when_no_key_configured ... ok
serve::tests::auth_requires_matching_bearer_when_key_set ... ok
serve::tests::cors_allow_all_echoes_any_origin ... ok
serve::tests::cors_allows_configured_origin ... ok
serve::tests::cors_localhost_is_always_allowed ... ok
serve::tests::cors_rejects_unlisted_remote_origin_by_default ... ok
serve::tests::stream_options_absent_defaults_to_none ... ok
serve::tests::stream_options_include_usage_parses ... ok
serve::tests::usage_frame_is_openai_shaped ... ok
---
14 passed; 0 failed; 0 ignored
```

## Live: health probe and bearer auth (16C / 16B)

A throwaway config dir whose only provider never listens (`api_base` on port
1) lets the server boot without any real LLM call. `serve_api_key` turns on
the gate: `/health` stays open for orchestration probes, while every other
route now demands the bearer token. The health body reports a stable model
count (the synthetic `default` plus `fake-model`); the role count is
normalized here because built-ins vary by build.

```bash
# A throwaway config dir whose only provider never listens (api_base port 1),
# so the server boots without making real LLM calls. serve_api_key turns on the
# bearer gate.
CFG=$(mktemp -d); mkdir -p "$CFG/roles"
cat > "$CFG/config.yaml" <<'EOF'
model: fake:fake-model
function_calling: false
serve_api_key: sk-demo-secret
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:1/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
EOF
AICHAT_CONFIG_DIR="$CFG" ./target/debug/aichat --serve 127.0.0.1:18160 >/dev/null 2>&1 &
SRV=$!; trap "kill -9 $SRV 2>/dev/null; rm -rf $CFG" EXIT
# /health is unauthenticated, so polling it is the ready signal even with a key set.
for i in $(seq 1 50); do curl -sf http://127.0.0.1:18160/health >/dev/null 2>&1 && break; sleep 0.1; done
code() { curl -s -o /dev/null -w "%{http_code}" "$@"; }
echo "GET /health      (no key)    -> $(code http://127.0.0.1:18160/health)"
echo "GET /v1/roles    (no key)    -> $(code http://127.0.0.1:18160/v1/roles)"
echo "GET /v1/roles    (wrong key) -> $(code -H 'Authorization: Bearer nope' http://127.0.0.1:18160/v1/roles)"
echo "GET /v1/roles    (right key) -> $(code -H 'Authorization: Bearer sk-demo-secret' http://127.0.0.1:18160/v1/roles)"
# Health body: models is stable (default + fake-model); the role count varies by
# build, so normalize it to keep this demo evergreen.
echo "health body                  -> $(curl -s http://127.0.0.1:18160/health | sed -E 's/"roles":[0-9]+/"roles":N/')"
```

```output
GET /health      (no key)    -> 200
GET /v1/roles    (no key)    -> 401
GET /v1/roles    (wrong key) -> 401
GET /v1/roles    (right key) -> 200
health body                  -> {"status":"ok","models":2,"roles":N}
```

## Live: CORS allowlist (16A)

`serve_cors_origins` widens the allowlist beyond the always-allowed
localhost. A browser preflight (`OPTIONS`) from a listed origin — or any
localhost origin — gets it echoed back in `Access-Control-Allow-Origin`; an
unlisted origin gets no header, so the browser blocks the cross-origin read.
(`serve_cors_allow_all: true` would echo *every* origin instead — for trusted
networks only.)

```bash
# serve_cors_origins widens the allowlist beyond the always-allowed localhost.
CFG=$(mktemp -d); mkdir -p "$CFG/roles"
cat > "$CFG/config.yaml" <<'EOF'
model: fake:fake-model
function_calling: false
serve_cors_origins:
  - http://host.docker.internal:3000
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:1/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
EOF
AICHAT_CONFIG_DIR="$CFG" ./target/debug/aichat --serve 127.0.0.1:18161 >/dev/null 2>&1 &
SRV=$!; trap "kill -9 $SRV 2>/dev/null; rm -rf $CFG" EXIT
for i in $(seq 1 50); do curl -sf http://127.0.0.1:18161/health >/dev/null 2>&1 && break; sleep 0.1; done
# Echo the Access-Control-Allow-Origin header (or note its absence) for a
# preflight from each origin.
acao() {
  local h
  h=$(curl -s -D - -o /dev/null -X OPTIONS -H "Origin: $1" \
        http://127.0.0.1:18161/v1/chat/completions \
      | grep -i '^access-control-allow-origin:' | tr -d '\r')
  [ -n "$h" ] && echo "$h" || echo "(no Access-Control-Allow-Origin)"
}
echo "listed origin    -> $(acao http://host.docker.internal:3000)"
echo "localhost        -> $(acao http://localhost:3000)"
echo "unlisted origin  -> $(acao https://evil.example.com)"

```

```output
listed origin    -> access-control-allow-origin: http://host.docker.internal:3000
localhost        -> access-control-allow-origin: http://localhost:3000
unlisted origin  -> (no Access-Control-Allow-Origin)
```

## Live: hot reload (16E)

Role-authoring edits should not require a server bounce. `late-role` does not
exist at boot, so it 404s. Drop the file on disk, `POST /v1/reload`, and the
server re-reads the roles directory: the role now resolves and the live count
in `/health` goes up by exactly one. (Provider `clients:` changes still need a
restart — the model wiring is fixed at boot.)

```bash
# No api key, no CORS — a plain server. We add a role on disk after boot.
CFG=$(mktemp -d); mkdir -p "$CFG/roles"
cat > "$CFG/config.yaml" <<'EOF'
model: fake:fake-model
function_calling: false
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:1/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
EOF
AICHAT_CONFIG_DIR="$CFG" ./target/debug/aichat --serve 127.0.0.1:18162 >/dev/null 2>&1 &
SRV=$!; trap "kill -9 $SRV 2>/dev/null; rm -rf $CFG" EXIT
for i in $(seq 1 50); do curl -sf http://127.0.0.1:18162/health >/dev/null 2>&1 && break; sleep 0.1; done
code() { curl -s -o /dev/null -w "%{http_code}" "$@"; }
roles() { curl -s http://127.0.0.1:18162/health | grep -oE '"roles":[0-9]+' | grep -oE '[0-9]+'; }
before=$(roles)
echo "GET /v1/roles/late-role (before reload) -> $(code http://127.0.0.1:18162/v1/roles/late-role)"
cat > "$CFG/roles/late-role.md" <<'ROLE'
---
description: added after boot
---
BODY
ROLE
curl -s -X POST http://127.0.0.1:18162/v1/reload >/dev/null
after=$(roles)
echo "POST /v1/reload, then..."
echo "GET /v1/roles/late-role (after reload)  -> $(code http://127.0.0.1:18162/v1/roles/late-role)"
echo "live role count delta                   -> +$((after - before))"

```

```output
GET /v1/roles/late-role (before reload) -> 404
POST /v1/reload, then...
GET /v1/roles/late-role (after reload)  -> 200
live role count delta                   -> +1
```

## Live: streaming usage chunk (16D)

Following OpenAI's convention, a caller that sets
`stream_options: {include_usage: true}` gets a trailing usage-only chunk
(`choices: []`) before `[DONE]`. aichat asks the upstream provider for the
same block, captures the token counts, and adds its own `cost_usd` (here 11
prompt tokens at \$1/MTok plus 3 completion tokens at \$2/MTok =
`0.000017`). Without the opt-in the stream is
byte-for-byte what it was before — no extra chunk. A throwaway mock SSE
backend stands in for the provider.

```bash
# A tiny OpenAI-compatible SSE backend: two content chunks, then a usage-only
# chunk (choices:[]), then [DONE] — exactly what an upstream that honors
# stream_options emits.
MOCK=$(mktemp)
cat > "$MOCK" <<'PY'
import http.server
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self): self.send_response(200); self.end_headers()
    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0)); self.rfile.read(n)
        self.send_response(200); self.send_header('Content-Type', 'text/event-stream'); self.end_headers()
        for c in ['{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"}}]}',
                  '{"choices":[{"index":0,"delta":{"content":" world"}}]}',
                  '{"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14}}']:
            self.wfile.write(('data: ' + c + '\n\n').encode()); self.wfile.flush()
        self.wfile.write(b'data: [DONE]\n\n'); self.wfile.flush()
http.server.HTTPServer(('127.0.0.1', 18164), H).serve_forever()
PY
python3 "$MOCK" >/dev/null 2>&1 &
MOCKPID=$!
# Prices make cost_usd > 0: 11*1/1e6 + 3*2/1e6 = 0.000017.
CFG=$(mktemp -d); mkdir -p "$CFG/roles"
cat > "$CFG/config.yaml" <<'EOF'
model: fake:fake-model
function_calling: false
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:18164/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
        input_price: 1.0
        output_price: 2.0
EOF
AICHAT_CONFIG_DIR="$CFG" ./target/debug/aichat --serve 127.0.0.1:18163 >/dev/null 2>&1 &
SRV=$!; trap "kill -9 $SRV $MOCKPID 2>/dev/null; rm -rf $CFG $MOCK" EXIT
for i in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:18164/ 2>/dev/null && break; sleep 0.1; done
for i in $(seq 1 50); do curl -sf http://127.0.0.1:18163/health >/dev/null 2>&1 && break; sleep 0.1; done
echo "with stream_options.include_usage -> trailing usage object:"
curl -s -N -X POST http://127.0.0.1:18163/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"model":"fake:fake-model","messages":[{"role":"user","content":"hi"}],"stream":true,"stream_options":{"include_usage":true}}' \
  | grep -o '"usage":{[^}]*}'
n=$(curl -s -N -X POST http://127.0.0.1:18163/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"model":"fake:fake-model","messages":[{"role":"user","content":"hi"}],"stream":true}' \
  | grep -c '"usage"')
echo "without stream_options -> usage chunk count: $n"
```

```output
with stream_options.include_usage -> trailing usage object:
"usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14,"cost_usd":0.000017}
without stream_options -> usage chunk count: 0
```

## Playground unfreeze (16I)

The one non-server change. Previously `buildBody()` (and anything else that
threw) sat outside the `try`, and `this.asking = false` ran only at the
function tail — so an exception left `asking` stuck `true`, and
`handleAsk()`'s `if (this.asking) return` guard then froze the send/input
path for the rest of the page's life. The fix moves the throwable work
inside the `try` and resets the guard state in a `finally`.

```bash
# 16I: buildBody() (and the stream) now live inside the try, and both
# `asking` and `askAbortController` reset in a finally — so any throw still
# clears the send/input guard instead of freezing the playground for the rest
# of the page's life.
grep -nE 'Phase 16I' assets/playground.html
grep -nA1 'const body = this.buildBody' assets/playground.html
grep -nA2 '} finally {' assets/playground.html
```

```output
1213:          // Phase 16I: everything that can throw (buildBody, RAG search, the
1219:            const body = this.buildBody();
1220-            if (this.settings.rag) {
1245:          } finally {
1246-            this.asking = false;
1247-            this.askAbortController = null;
```

## The full hardening suite

The live snippets above each isolate one knob; the integration suite drives
all of them end-to-end against a real `aichat --serve` (including the 16D
mock-SSE round-trip), so the whole hardening surface is exercised in one
pass.

```bash
AICHAT_BIN=./target/debug/aichat bats tests/integration/server-hardening.sh 2>&1
```

```output
1..12
ok 1 phase 16C: GET /health returns ok with model and role counts
ok 2 phase 16B: no auth required when serve_api_key is unset
ok 3 phase 16B: request without a key is 401 when serve_api_key is set
ok 4 phase 16B: correct bearer token is accepted
ok 5 phase 16B: wrong bearer token is rejected
ok 6 phase 16B + 16C: /health stays open even when serve_api_key is set
ok 7 phase 16A: configured origin receives Access-Control-Allow-Origin
ok 8 phase 16A: unlisted remote origin gets no CORS header by default
ok 9 phase 16A: serve_cors_allow_all echoes any origin
ok 10 phase 16E: POST /v1/reload picks up a newly added role
ok 11 phase 16D: stream_options.include_usage emits a usage chunk with cost
ok 12 phase 16D: without include_usage no usage chunk is emitted
```

## Where this lives

Every knob is opt-in and defaults to off, so a plain `aichat --serve` behaves
exactly as before. User-facing reference (config keys, env-var overrides,
orchestration notes) is in [`docs/features/server.md`](../features/server.md);
the config keys are commented in `config.example.yaml`. Items 16F/G/H
(`RolePublicView`, single-role retrieval, cost headers) shipped earlier as
the role-publish surface Phase 20 federation reads from.

