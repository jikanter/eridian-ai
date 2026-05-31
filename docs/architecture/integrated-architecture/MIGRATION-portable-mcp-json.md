# Migration: from `llm-functions/mcp.json` to portable `~/.config/mcp/mcp.json`

**Audience:** users running aichat against the [llm-functions](https://github.com/jikanter/personal-llm-functions) Node HTTP bridge.
**Time:** ~5 minutes.
**Reversible:** yes — the rollback section restores the bridge in three commands.
**Companion docs:** [`bridge-retirement.md`](bridge-retirement.md), [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md).

This is a hand-followed migration. No `showboat verify` is required.

## What changes

Before:

- MCP server declarations live in `llm-functions/mcp.json`.
- The Node bridge in `llm-functions/mcp/bridge/` listens on port 8808.
- Each MCP tool is reachable via a `bin/<server>__<tool>` shim that aichat shells out to.

After:

- MCP server declarations live in `~/.config/mcp/mcp.json` (portable, consumer-agnostic).
- aichat's native `mcp_pool` consumes the file directly via stdio/HTTP transports.
- `config.yaml` carries one new line: `mcp_servers_file: ~/.config/mcp/mcp.json`. Inline `mcp_servers:` entries still work and override file-loaded entries on key conflict.
- The bridge process and per-tool shims are no longer needed.

## Steps

### 1. Place the portable file

```bash
mkdir -p ~/.config/mcp
cp <path-to-llm-functions>/mcp.json ~/.config/mcp/mcp.json
```

The bridge already uses `mcpServers` as the top-level key, so the format is bit-compatible.

### 2. Validate the file

```bash
aichat --validate-mcp-config ~/.config/mcp/mcp.json
```

Expected output for a healthy file:

```
ok: /Users/<you>/.config/mcp/mcp.json
  N servers (X stdio, Y http/sse)
    [stdio] sqlite
    [stdio] git
    ...
```

Exit codes: `0` valid, `1` parse/schema error, `2` no file found. For a JSON payload, add `-o json`. Schema rules enforced ([`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md) § Validation):

1. Parses as JSON.
2. Has top-level `mcpServers` object.
3. Each entry sets `command` (stdio) **or** `url` (http/sse), not both, not neither.
4. `args` (when present) is a string array.
5. `env` and `headers` (when present) are string-to-string maps.

If validation fails, fix the file and re-run before continuing.

### 3. Point aichat at the file

Edit `~/Library/Application Support/aichat/config.yaml` (or `$AICHAT_CONFIG_DIR/config.yaml`) and append:

```yaml
mcp_servers_file: ~/.config/mcp/mcp.json
```

Inline `mcp_servers:` entries (if any) keep working. They override file-loaded entries on key conflict — useful for tests or one-off overrides.

### 4. Smoke-test against aichat

```bash
aichat --mcp-server "$HOME/.local/bin/uvx mcp-server-git" --list-tools
```

This bypasses the pool but confirms the binary is healthy. Then run aichat normally — your tools should appear under their namespaced `<server>:<tool>` names.

If a server registers but its tools don't appear, check the warning logs (`RUST_LOG=warn`); per-server failures during pool init now print a `MCP server '<name>' failed to register: ...` warning instead of poisoning the whole pool.

### 5. Stop the bridge and clean up llm-functions

In your `llm-functions` checkout:

```bash
argc mcp stop                       # kill the Node listener on 8808
argc mcp recovery-functions -S      # strip bridge-merged entries from functions.json
```

The deletion of `mcp/bridge/`, `scripts/mcp.sh`, `scripts/run-mcp-tool.sh`, the local `mcp.json`, and the `bin/<server>__<tool>` shims is a separate commit in `llm-functions` — see [`bridge-retirement.md`](bridge-retirement.md) § "The retirement diff." Until that lands, the bridge directory is harmless dead code.

### 6. Sweep external scripts

Anything outside aichat or llm-functions that calls `http://localhost:8808/tools` or invokes `bin/<server>__<tool>` shims directly needs to switch to:

- `aichat --mcp` (stdio MCP) for tool listing/invocation, or
- `aichat --mcp-server "<command>"` for one-off probes of a single server.

A sweep:

```bash
grep -r "8808\|MCP_BRIDGE_PORT\|mcp/bridge\|run-mcp-tool" ~/path/to/scripts
```

should return empty before you delete the bridge directory.

## Rollback

If anything regresses:

1. In `aichat/config.yaml`, remove the `mcp_servers_file:` line.
2. In `llm-functions`, run `argc mcp start` to relaunch the bridge.
3. Run `argc mcp merge-functions -S` to re-merge the bridge-prefixed entries into `functions.json`.

The portable file at `~/.config/mcp/mcp.json` can stay in place during rollback — the bridge's own `mcp.json` and the portable file co-exist without coupling.

## Cross-repo independence

You can migrate (or roll back) without ever cloning the other repo. The portable file lives outside both projects:

- aichat reads it from `~/.config/mcp/mcp.json` (or `$XDG_CONFIG_HOME/mcp/mcp.json`, or `./mcp.json`, or wherever `mcp_servers_file:` points).
- llm-functions, after retirement, no longer participates in MCP server declarations at all.

This is the property that makes the file "portable" — any consumer (aichat, the future harness interface, Claude Code, Cursor) can read it without depending on any other consumer being installed.
