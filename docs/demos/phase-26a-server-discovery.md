# Server Discovery & Bridge-Token Reuse

*2026-05-15T18:07:43Z by Showboat 0.6.1*
<!-- showboat-id: a479bbab-5c38-4bed-ad19-2ad9af3106c8 -->

When the pi REPL launches, aichat needs an HTTP server for `pi` to bridge against. Rather than always spawning a private one, the launcher first probes `127.0.0.1` ports 8000-9000 for an aichat server it can reuse. Reuse is deliberately strict: it only attaches to a server whose `/v1/state/*` bridge an exported `AICHAT_BRIDGE_TOKEN` authenticates against, so every slash command is guaranteed to work against the shared server. A plain token-less `aichat --serve` is never reused, because its bridge would reject every command.

This demo shows the probe code, the test suite that exercises it, and the fingerprint behavior against real aichat servers.

## The probe functions

The launcher (`src/repl/pi.rs`) gains four functions: a pure response fingerprint, a single-port probe, a concurrent range scan, and the env-gated entry point.

```bash
grep -nE "^(const PROBE|fn is_authenticated_bridge|async fn (port_has_bridge|probe_port_range|probe_existing_server))" src/repl/pi.rs
```

```output
160:const PROBE_PORT_START: u16 = 8000;
161:const PROBE_PORT_END: u16 = 9000;
165:const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
178:fn is_authenticated_bridge(status: u16, body: &str) -> bool {
190:async fn port_has_bridge(client: &reqwest::Client, port: u16, token: &str) -> bool {
207:async fn probe_port_range(start: u16, end: u16, token: &str) -> Option<u16> {
233:async fn probe_existing_server() -> Option<String> {
```

## Fingerprint truth table

The probe sends an authenticated `GET /v1/state/info` and classifies the response:

| Target | Response | Probe verdict |
|---|---|---|
| Non-aichat server, or aichat `--serve` with no bridge token | `404` | skip |
| aichat bridge, **different** token | `401` | skip |
| aichat bridge, **matching** token | `200` + `{"info": ...}` | **reuse** (lowest port wins) |

The unit tests pin each row of that table; the integration tests stand up a real mock bridge on a TCP socket and drive the actual `probe_port_range` against it.

```bash
out=$(cargo test --bin aichat repl::pi::tests 2>&1)
echo "$out" | grep -E "(is_authenticated_bridge_|probe_).*\.\.\. " | sed "s/^test //" | sort
echo "---"
echo "$out" | grep -oE "[0-9]+ passed; [0-9]+ failed; [0-9]+ ignored"
```

```output
repl::pi::tests::is_authenticated_bridge_accepts_info_payload ... ok
repl::pi::tests::is_authenticated_bridge_rejects_foreign_200 ... ok
repl::pi::tests::is_authenticated_bridge_rejects_not_found ... ok
repl::pi::tests::is_authenticated_bridge_rejects_unauthorized ... ok
repl::pi::tests::probe_existing_server_honors_opt_out ... ok
repl::pi::tests::probe_finds_an_authenticated_bridge ... ok
repl::pi::tests::probe_ignores_a_non_bridge_server ... ok
repl::pi::tests::probe_rejects_a_bridge_with_a_different_token ... ok
repl::pi::tests::probe_returns_none_when_nothing_listening ... ok
---
14 passed; 0 failed; 0 ignored
```

## Live: a token-enabled server is reusable

Start an aichat server with `AICHAT_BRIDGE_TOKEN` exported, then hit `/v1/state/info` the way the probe does. The matching token is accepted (`200` — the probe would reuse this server); a wrong token is rejected (`401` — the probe skips it).

```bash
AICHAT_BRIDGE_TOKEN=demo-shared-token ./target/debug/aichat --serve 8000 >/dev/null 2>&1 &
SRV=$!
trap "kill -9 $SRV 2>/dev/null" EXIT
sleep 2
code() { curl -s -o /dev/null -w "%{http_code}" --retry 5 --retry-connrefused --max-time 5 "$@"; }
echo "matching token -> $(code -H "Authorization: Bearer demo-shared-token" http://127.0.0.1:8000/v1/state/info)"
echo "wrong token    -> $(code -H "Authorization: Bearer not-the-token"     http://127.0.0.1:8000/v1/state/info)"
```

```output
matching token -> 200
wrong token    -> 401
```

## Live: a token-less server is invisible to the probe

A plain `aichat --serve` started without `AICHAT_BRIDGE_TOKEN` does not expose the bridge at all — `/v1/state/*` returns `404`, even with a bearer header. The probe treats this as "not a usable bridge" and skips it, so the REPL falls back to its own private in-process server instead of attaching to one whose slash commands would all fail.

```bash
./target/debug/aichat --serve 8200 >/dev/null 2>&1 &
SRV=$!
trap "kill -9 $SRV 2>/dev/null" EXIT
sleep 2
code() { curl -s -o /dev/null -w "%{http_code}" --retry 5 --retry-connrefused --max-time 5 "$@"; }
echo "bridge route, no token configured -> $(code -H "Authorization: Bearer any-token" http://127.0.0.1:8200/v1/state/info)"
echo "public route still works          -> $(code http://127.0.0.1:8200/v1/models)"
```

```output
bridge route, no token configured -> 404
public route still works          -> 200
```

## How the launcher uses this

`launch_pi` calls `probe_existing_server` before binding its own port:

- **Match found** — the discovered URL becomes the bridge URL; no in-process server is started, and the reused server is left running on REPL exit.
- **No match** — aichat binds an ephemeral port, mints a fresh per-launch token, and starts a private in-process server it shuts down on exit (the original behavior).

Reuse therefore requires `AICHAT_BRIDGE_TOKEN` to be exported and to match the target server's token. To share one long-lived server across REPL sessions, export the same token in every shell:

```bash
export AICHAT_BRIDGE_TOKEN=$(uuidgen | tr -d - | tr 'A-Z' 'a-z')
aichat --serve 8000   # the shared server
aichat                # a REPL that discovers and reuses it
```

Set `AICHAT_NO_SERVER_PROBE=1` to skip discovery entirely and always start a private server. Full reference: `docs/features/server.md`; bridge security: `docs/features/repl-pi.md`.
