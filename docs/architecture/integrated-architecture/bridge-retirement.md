# Bridge Retirement: llm-functions Node HTTP bridge → portable `mcp.json` + aichat native MCP pool

**Status:** Blocked. Two upstream aichat fixes required before any of this can land.
**Last updated:** 2026-05-01
**Companion spec:** [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md)
**Validation:** `tests/integration/mcp-server.sh`, `docs/demos/demo-mcp-server.md`

## Goal

Retire the Node + Express HTTP bridge in [llm-functions/mcp/bridge/](https://github.com/jikanter/personal-llm-functions/tree/main/mcp/bridge) (port 8808). Replace it with two pieces:

1. A **portable `mcp.json` artifact** at `~/.config/mcp/mcp.json` — declarations live here, owned by neither aichat nor llm-functions. Schema specified in [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md).
2. **aichat's native MCP client** (`mcp_pool`) — reads the portable file and consumes external MCP servers directly via stdio/HTTP.

After retirement:

- `mcp.json` no longer lives inside llm-functions. It moves to a portable, consumer-agnostic path.
- aichat's `config.yaml` does **not** carry MCP server declarations. It carries a one-line pointer (`mcp_servers_file:`) and aichat-specific runtime tuning only. Earlier drafts of this plan proposed inlining server declarations into `config.yaml`; that approach is rejected because it makes aichat the source-of-truth for what is actually environment configuration.
- `argc mcp *` subcommands are deleted; `argc build` no longer calls `merge-functions`.

## Framing: aichat --mcp is a facade, not a destination

`aichat --mcp` (stdio MCP server) is **one** way to expose aichat's tools and roles to clients that speak MCP — Claude Code, Cursor, Inspect AI runners. It is not the canonical surface. Other exposure surfaces exist or are planned:

- The CLI itself (`aichat -r role`, `aichat --each`, `aichat call …`) — humans, scripts, and `argc`-driven pipelines.
- The HTTP server (`aichat --serve`) — OpenAI-compatible API, OpenWebUI integration. Phases 16-18 (deferred).
- The trace JSONL — the test harness, eval, and (future) training pipelines. See [`docs/analysis/caching/ECOSYSTEM.md`](../../analysis/caching/ECOSYSTEM.md).
- The future harness interface (TBD; see [`README.md`](README.md)) — the cross-tool consumption point that lets agentic clients see aichat's roles, agents, and tools as a single unit.

This plan retires the Node bridge so `aichat --mcp` and `mcp_pool` can do their jobs cleanly. It does **not** elevate `aichat --mcp` to "the surface." If the harness interface materializes and supersedes the MCP exposure, that is a separate decision; this plan does not block it.

## Why retire the bridge

The bridge predates aichat's native MCP support. As of aichat 0.5.1-eridian:

| Capability | Bridge | aichat native |
|---|---|---|
| Read `mcpServers`-style config | ✓ | ✓ (after this plan, via the portable file) |
| Spawn stdio MCP servers | ✓ | ✓ (rmcp + tokio) |
| Remote HTTP/SSE MCP servers | ✗ | ✓ (`url:` field) |
| Configurable startup/call timeouts | ✗ | ✓ (`mcp_startup_timeout`, `mcp_call_timeout`, `mcp_max_connections`) |
| Schema cache TTL | ✗ | ✓ (`mcp_cache_ttl`, default 1h) |
| Tool name namespacing | `srv__tool` (double-underscore) | `srv:tool` (colon) |
| Surfaces tools to external MCP clients | indirect (via shims in `bin/`) | direct (`aichat --mcp`) |

The bridge also adds: ~1500 LOC of Node, an Express HTTP listener, a per-tool symlink in `llm-functions/bin/`, a separate `mcp.json`, ~10s of cold-start latency from `argc mcp start` (waiting on stdio child boot before HTTP bind), and a translation layer that diverges from aichat's tool-naming convention.

## Pre-requisites (upstream aichat fixes)

Pinned by tests in `tests/integration/mcp-server.sh`. The two skips below must pass before this plan can execute.

### 1. `ToolCall::eval` MCP-pool routing

`src/function.rs:337` (`eval`) is the single-call dispatch path used by `src/mcp.rs:191` when aichat is running as `--mcp` server. It currently has special cases for `tool_search`, `search_knowledge`, and pipeline-roles, then falls through to llm-functions binary lookup via `run_llm_function`. It does NOT have the `is_mcp` check that `eval_tool_calls` (`src/function.rs:33-44`) uses to dispatch through `mcp_pool`.

Result: `aichat --mcp` advertises `mcp_servers:` tools but cannot invoke them. Calling `git:git_status` returns a "binary not found" error against `functions/bin/`.

**Fix sketch:** port the same `is_mcp` check + `mcp_pool.call()` dispatch from `eval_tool_calls` into the head of `ToolCall::eval`. Both paths should share a single helper.

**Test that flips green:** `mcp-server: tool-call dispatch through mcp_servers pool (probe a)`.

### 2. Multi-server pool init at N≥5

Booting all 10 entries from the user's production server set under `mcp_servers:` regresses to one of two failure modes:

- 5-server subset (sqlite/memory/sequential-thinking/n8n-mcp/ollama): pool init returns 0 registered tools.
- 5-server subset (git/todoist-mcp/server-fetch/dash-api/obsidian): pool advertises `discover_roles` but `tools/call` requests hang indefinitely.
- 10-server full set: same hang as the second subset.

Bumping `mcp_startup_timeout` and `mcp_call_timeout` to 60s does not help — symptom looks like a runtime/IO race or deadlock, not slow startup. Likely candidates: rmcp client message-pump backpressure, tokio executor saturation under N concurrent stdio children, or a shared lock in `McpConnectionPool`.

3 servers boot cleanly (verified in `mcp-server: 3 concurrent stdio servers register all tools`). Threshold is somewhere between 3 and 5.

**Test that flips green:** `mcp-server: many concurrent stdio servers regression (probe b large-N)`.

## Validation gates

Before executing the retirement diff, all four must hold:

1. `bats tests/integration/mcp-server.sh` reports 7/7 passing (no skips).
2. `showboat verify docs/demos/demo-mcp-server.md` exits 0 with the "Known limitation" sections re-purposed to document the fixed behavior.
3. End-to-end: `aichat --mcp` from a clean checkout, with `mcp_servers_file:` pointing at a portable `mcp.json` per [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md), can list AND invoke at least one tool from each of the user's 10 production servers.
4. The portable `mcp.json` validates against the schema in `SPEC-mcp-json-artifact.md` § Validation.

## The retirement diff

### llm-functions changes

See [github.com/jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions) for the working tree.

#### Delete

- `mcp/bridge/` — entire directory (Node sources, package.json, node_modules)
- `mcp/server/` — review first; this is the inverse experiment and may also be retireable
- `mcp.json` — content moves into the portable file (see below)
- `scripts/mcp.sh` — entire file
- `scripts/run-mcp-tool.sh` — bridge-side per-tool dispatcher
- `bin/<server>__<tool>` shims — generated by `argc mcp build-bin`; gone when bridge is gone
- `cache/__mcp__/` — bridge log + state

#### Modify

**`Argcfile.sh`** — current bridge-coupled lines:

```
74:    if [[ -f mcp.json ]]; then
75:        argc mcp merge-functions -S
76:    fi
...
371:    argc mcp check
...
574:# @cmd Run mcp command
576:mcp() {
577:    bash ./scripts/mcp.sh "$@"
...
791:_choice_mcp_args() {
```

After retirement:
- Drop the `mcp.json` block at lines 74-76 from `build`.
- Drop `argc mcp check` at line 371 (it lived inside `check-env`).
- Delete the `mcp` subcommand (lines 574-577) and `_choice_mcp_args` completion helper (lines 791-795).
- The `recovery-functions` step inside `scripts/mcp.sh` (which strips entries with `"mcp"` field from `functions.json`) needs to run ONCE before deleting the script, to clean up `functions.json` of stale bridge-merged entries. After that, `argc build` produces a `functions.json` containing only first-party llm-functions tools — the rest come from aichat's `mcp_pool` at runtime, sourced from the portable `mcp.json`.

**`functions.json`** — regenerate via `argc build@tool`. After retirement this file contains only entries built from `tools.txt`; no `"mcp": "<server>"` entries.

**`README.md`** and **`CLAUDE.md`** — remove the "MCP bridge" section. Replace with a one-paragraph pointer to the portable artifact spec at [github.com/jikanter/aichat-private/blob/main/docs/roadmap/integrated-architecture/SPEC-mcp-json-artifact.md](https://github.com/jikanter/aichat-private/blob/main/docs/roadmap/integrated-architecture/SPEC-mcp-json-artifact.md).

### aichat changes

**`config.yaml`** — append a single line:

```yaml
mcp_servers_file: ~/.config/mcp/mcp.json
```

The inline `mcp_servers:` block in `config.yaml` remains supported (useful for tests and one-off configs) but is **not** the canonical home for declarations. Inline entries merge with the file-loaded entries; inline wins on key conflict. See `SPEC-mcp-json-artifact.md` § "How aichat consumes this file."

**`src/mcp_client/mod.rs`** — add the loader for `mcp_servers_file`. Follow the search order in `SPEC-mcp-json-artifact.md` § "File location and discovery." Normalize each `mcpServers` entry into the existing `McpServerConfig` struct.

`.env` for secrets is still loaded by aichat from `~/Library/Application Support/aichat/.env`. Env interpolation in the portable file uses parent-process environment per the spec.

### User migration (one-time)

The hand-followable steps live in [`MIGRATION-portable-mcp-json.md`](MIGRATION-portable-mcp-json.md). Summary: copy `mcp.json` to `~/.config/mcp/mcp.json`, run `aichat --validate-mcp-config` against it, append `mcp_servers_file:` to `config.yaml`, then `argc mcp stop` + `argc mcp recovery-functions -S` to clean up llm-functions. The bridge's existing `mcp.json` already uses `mcpServers` as the top-level key, so the migration is a straight copy in most cases.

### External-client changes

If anything outside aichat or llm-functions calls `http://localhost:8808/tools` or invokes a `bin/<server>__<tool>` shim directly, those callers move to:

- `aichat --mcp` (stdio MCP) for tool listing and invocation, or
- `aichat --list-tools --mcp-server "<command>"` for a one-off probe of a single server, or
- (future) the harness interface — see [`README.md`](README.md).

`grep -r "8808\|MCP_BRIDGE_PORT\|mcp/bridge\|run-mcp-tool"` across the user's scripts directories should turn up empty before the bridge directory is deleted.

## Migration order

1. Land both upstream fixes; confirm `bats tests/integration/mcp-server.sh` is 7/7 green.
2. Update `aichat/docs/demos/demo-mcp-server.md` "Known limitation" sections to reflect fixed behavior; re-run `showboat verify`.
3. Create `~/.config/mcp/mcp.json` per `SPEC-mcp-json-artifact.md`, populated from the existing llm-functions `mcp.json`.
4. Add `mcp_servers_file: ~/.config/mcp/mcp.json` to aichat `config.yaml` (production: `~/Library/Application Support/aichat/config.yaml`).
5. Run `argc mcp recovery-functions -S` once in llm-functions to clean bridge-merged entries from `functions.json`.
6. Run `argc mcp stop` to kill the bridge process; remove the cache.
7. Delete `mcp/bridge/`, `scripts/mcp.sh`, `scripts/run-mcp-tool.sh`, `mcp.json`, all `bin/*__*` shims.
8. Edit `Argcfile.sh` per the modify list above.
9. Run `argc build` from a clean checkout; confirm `functions.json` parses and matches expected tool count (first-party tools only).
10. Run `aichat --mcp`-stdio handshake from end-to-end against the same test harness used in `tests/integration/mcp-server.sh` to verify all 10 servers' tools are still callable through the portable file.
11. `git rm -r mcp/bridge/ mcp.json scripts/mcp.sh scripts/run-mcp-tool.sh` and commit (in llm-functions).

Each step is independently reversible up through step 10.

## Rollback plan

If post-retirement behavior regresses:

1. `git revert` the deletion commit in [llm-functions](https://github.com/jikanter/personal-llm-functions). The bridge directory and `scripts/mcp.sh` come back.
2. Remove the `mcp_servers_file:` line from aichat `config.yaml`.
3. Run `argc mcp start` to bring the bridge back.
4. Run `argc mcp merge-functions -S` to re-merge bridge-prefixed entries into `functions.json`.

The portable `~/.config/mcp/mcp.json` can stay in place during rollback; the bridge's `mcp.json` and the portable file co-exist without coupling. The only state the bridge holds is the per-tool symlinks in `bin/` and the contents of `cache/__mcp__/`. Both regenerate on the next `argc mcp start`.

## Cross-repo independence

This plan is deliberately structured so neither repo's filesystem references the other:

- aichat docs (this directory) link to llm-functions via [github.com/jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions), not via local paths.
- llm-functions docs ([CLAUDE.md](https://github.com/jikanter/personal-llm-functions/blob/main/CLAUDE.md)) link to this spec via the eridian-ai GitHub URL, not via local paths.
- The portable `mcp.json` lives outside both repos.
- A user can clone either repo independently and follow the docs to the other via GitHub URLs.

This is the property that future-proofs the plan against re-organizing either repo's directory structure.

## Open questions

1. Does anything depend on the bridge's prefix scheme (`git__commit` with double underscore) vs aichat's (`git:commit` with colon)? `tools.txt` and existing agents don't reference these names; external scripts might.
2. Is `mcp/server/` (the inverse experiment) actively used by anything? If not, retire it together with the bridge.
3. Should aichat watch the portable `mcp.json` for changes and reload? `mcp_cache_ttl` already exists; a config-reload hook would be cleaner but isn't blocking.
4. The `x-aichat` extension namespace (per spec) is not implemented yet. Add when the first per-server tuning need arises.