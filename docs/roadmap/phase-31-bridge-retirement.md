# Phase 31: Bridge Retirement & MCP Pool Hardening

**Epic:** 11 — Bridge Retirement
**Cross-cutting:** [`bridge-retirement.md`](../architecture/integrated-architecture/bridge-retirement.md), [`SPEC-mcp-json-artifact.md`](../architecture/integrated-architecture/SPEC-mcp-json-artifact.md)
**Validation:** `tests/integration/mcp-server.sh`, `docs/demos/demo-mcp-server.md`

---

## Why this is a phase

The bridge-retirement plan in `docs/architecture/integrated-architecture/` is a cross-repo migration. Two of its prerequisites are pure aichat work — pinned by `skip`-marked tests in `tests/integration/mcp-server.sh`. A third item (the portable-file loader) is also aichat-only. Bundling the three under a phase makes the retirement diff in `llm-functions` a single review step instead of three.

After this phase ships, `llm-functions` can delete the Node + Express bridge, the per-tool `bin/<server>__<tool>` shims, `mcp.json`, and `scripts/mcp.sh` in one commit. That commit is out of scope here; this phase only lands the aichat-side enablement.

## Items

### 31A. `ToolCall::eval` MCP-pool routing

**Problem.** `eval_tool_calls` (`src/function.rs:33-44`) checks whether each call is MCP-sourced and dispatches through `mcp_pool.call_tool`. The single-call path `ToolCall::eval` (`src/function.rs:337`) — used by `src/mcp.rs:191` when aichat runs as `--mcp` server — does not. It falls through to `run_llm_function`, which looks up a binary in `llm-functions/bin/` and fails with "binary not found" for any namespaced tool like `git:git_status`.

**Fix.** Extract a shared `is_mcp_call(config, name) -> bool` helper. At the head of `ToolCall::eval`, branch into `eval_mcp_tool` when the helper returns true, mirroring the batch path in `eval_single_tool`.

**Test that flips green.** `mcp-server: tool-call dispatch through mcp_servers pool (probe a)`.

**Files.** `src/function.rs`, `src/mcp_client/mod.rs` (no new code, helper visibility only), `tests/integration/mcp-server.sh` (remove `skip`).

### 31B. Multi-server pool init regression at N≥5

**Problem.** With 5+ entries in `mcp_servers:`, `McpConnectionPool` regresses to one of two failure modes:

- 5-server subset (sqlite/memory/sequential-thinking/n8n-mcp/ollama): pool init returns 0 registered tools.
- 5-server subset (git/todoist-mcp/server-fetch/dash-api/obsidian): pool advertises `discover_roles` but `tools/call` requests hang indefinitely.
- 10-server full set: same hang as the second subset.

3 servers boot cleanly. Threshold is somewhere between 3 and 5. Bumping `mcp_startup_timeout` and `mcp_call_timeout` to 60s does not help.

**Hypotheses to investigate.**

1. `RunningService` background tasks compounding under tokio's executor as more accumulate — each connection is a long-lived task; saturation of the runtime could starve the rmcp message pump.
2. `McpConnection::connect` discards stderr (`Stdio::null()`) but the child may block when pipes back up; combined with #1, this could deadlock.
3. RwLock contention in `get_or_connect` — the read/write/read dance is sequential per connection, but each `list_all_tools` runs while holding no lock; concurrent boot is not exercised today.
4. Some servers send unsolicited notifications during boot that rmcp 0.x doesn't drain promptly.

**Approach.** Add a focused 5-server reproducer test first. Capture trace logs (RUST_LOG=rmcp=trace). Compare against the small-N happy path. Likely fix is one of: (a) parallelize `all_tool_declarations` with a bounded `JoinSet`, (b) inherit child stderr to a writer instead of `null` so blocking pipes drain, (c) bump tokio worker thread count, (d) replace the stale-eviction lock dance with a single guard.

**Test that flips green.** `mcp-server: many concurrent stdio servers regression (probe b large-N)`.

**Files.** `src/mcp_client/mod.rs` (primary), `tests/integration/mcp-server.sh`.

### 31C. `mcp_servers_file:` portable loader

**Problem.** Today MCP server declarations live in `config.yaml` under `mcp_servers:`. The bridge keeps a parallel `mcp.json` inside `llm-functions/`. Per `SPEC-mcp-json-artifact.md`, the portable file at `~/.config/mcp/mcp.json` (or `$XDG_CONFIG_HOME/mcp/mcp.json`, or `./mcp.json` per project) must be the canonical home. `config.yaml` keeps a one-line pointer plus runtime tuning.

**Fix.** Add `mcp_servers_file: Option<String>` to `Config`. Load order, first hit wins:

1. Path explicitly provided in `mcp_servers_file:`.
2. `./mcp.json` in CWD.
3. `$XDG_CONFIG_HOME/mcp/mcp.json`.
4. `~/.config/mcp/mcp.json` if `XDG_CONFIG_HOME` unset.

Parse `{ "mcpServers": { ... } }`, normalize each entry into `McpServerConfig`. Field translation:

| portable JSON | aichat `McpServerConfig` |
|---|---|
| `command` | `command` |
| `args` | `args` |
| `env` | `env` (with `${VAR}` interpolation against parent env) |
| `url` | `endpoint` |
| `headers` | `headers` (with `${VAR}` interpolation) |
| `type` | inferred from `command`/`url`; ignored otherwise |
| `x-aichat.*` | reserved for follow-on, ignored for now |

Merge with inline `mcp_servers:` from `config.yaml`. **Inline wins on key conflict.** This matches the spec's rationale: inline is a test/override surface, not the canonical home.

**Tests.** Unit tests for: parse, env-var interpolation, search order, inline-wins-on-conflict, missing-file is non-fatal.

**Files.** `src/config/mod.rs` (add field, load step), `src/mcp_client/mod.rs` (parse helper), `Cargo.toml` (no new deps — `serde_json` already present).

### 31E. `aichat --validate-mcp-config [PATH]`

**Problem.** The portable file lives outside both repos; users (and the future
harness interface) need a way to validate it from outside aichat's main
runtime — a typo there should not silently disable an MCP server only to
surface as a confusing "0 tools" symptom at chat time.

**Fix.** Added `--validate-mcp-config [PATH]` to the CLI. Runs **before**
`Config::init` so a broken `config.yaml` doesn't mask validation results.
Honors the same search order as the loader when `PATH` is omitted. Exit
codes:

- `0` — valid; prints `ok: <path>` with a per-server breakdown (or JSON
  payload when `-o json`).
- `1` — parse error or schema rule violation (any of SPEC § Validation 1–5).
- `2` — no file found via search order (or explicit path missing).

**Files.** `src/cli.rs` (flag), `src/main.rs` (early dispatch),
`src/mcp_client/mod.rs` (`run_validate_mcp_config` + helpers),
`tests/integration/mcp-validate.sh` (9 bats cases).

### 31D. Unskip tests; refresh demo

- Remove `skip` from `tests/integration/mcp-server.sh` for probe-a and probe-b large-N.
- Add a new test: portable `mcp.json` discovery from `~/.config/mcp/mcp.json` (use `BATS_TEST_TMPDIR` + `XDG_CONFIG_HOME` override).
- Update `docs/demos/demo-mcp-server.md` § 4 ("Known limitation: tool-call dispatch") and § 5 ("multi-server pool happy path") to reflect fixed behavior.
- Bring `showboat verify` to green.

## Parallelization

- 31A and 31C are independent. Either can land first.
- 31B sits behind a diagnostic step; can run in parallel with 31A/31C.
- 31D is the last gate — runs after A, B, C land.

## What is explicitly NOT done in this phase

- The actual deletion of `llm-functions/mcp/bridge/`, `scripts/mcp.sh`, `mcp.json`, and the `bin/<server>__<tool>` shims. That is a separate commit in `llm-functions` once this phase's validation gates pass.
- The `x-aichat` extension namespace (`namespace`, `lazyDiscover` per server). Reserved; first per-server tuning need triggers it.
- Hot-reload of `mcp.json` on file change. `mcp_cache_ttl` covers most of the cost; revisit if `POST /v1/reload` lands in Phase 16E.
- Renaming the bridge's `srv__tool` namespace convention to aichat's `srv:tool` convention in any external scripts the user maintains. Out of scope for this repo; `grep` sweep is the user's job.

## Dependencies (external)

- **rmcp** version pinned in `Cargo.toml` — 31B may force a bump if the regression is upstream.
- **uvx**, **node** on PATH for the bats fixtures (already present in dev environment).

## Key files

- `src/function.rs` — primary 31A target.
- `src/mcp_client/mod.rs` — 31B and 31C target.
- `src/config/mod.rs` — 31C target (config field + load step).
- `tests/integration/mcp-server.sh` — 31D target.
- `docs/demos/demo-mcp-server.md` — 31D target.
