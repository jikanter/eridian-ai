# Phase 3 MCP Consumption — Remaining Features

*2026-03-30T16:12:31Z by Showboat 0.6.1*
<!-- showboat-id: c33b24c9-9e5f-4212-9b64-8c2e5c5769cb -->

Completes the Phase 3 MCP consumption feature set. Adds --refresh (cache bypass), --arg KEY=VALUE (flat CLI arguments with type inference and hybrid JSON merging), configurable timeouts (mcp_cache_ttl, mcp_startup_timeout, mcp_call_timeout), idle connection eviction (5-minute timeout), and max connection limits (mcp_max_connections).

## --refresh: Cache Bypass

The --refresh flag forces re-fetching tool schemas from the MCP server, bypassing the 1-hour cache TTL. Useful when a server has updated its tool set.

```bash
cargo run --quiet 2>/dev/null -- --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /private/tmp' --list-tools --refresh | head -3
```

```output
read_file - Read the complete contents of a file as text. DEPRECATED: Use read_text_file instead.
read_text_file - Read the complete contents of a file from the file system as text. Handles various text encodings and provides detailed error messages if the file cannot be read. Use this tool when you need to examine the contents of a single file. Use the 'head' parameter to read only the first N lines of a file, or the 'tail' parameter to read only the last N lines of a file. Operates on the file as text regardless of extension. Only works within allowed directories.
read_media_file - Read an image or audio file. Returns the base64 encoded data and MIME type. Only works within allowed directories.
```

## --arg KEY=VALUE: Flat CLI Arguments

Instead of requiring --json for every tool call, --arg provides KEY=VALUE pairs with automatic type inference. Numbers, booleans, and null are detected; repeated keys become arrays.

```bash
echo 'flat arg demo content' > /private/tmp/mcp_flat_arg.txt && cargo run --quiet 2>/dev/null -- --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /private/tmp' --call read_file --arg path=/private/tmp/mcp_flat_arg.txt
```

```output
flat arg demo content

```

Hybrid merging: --arg overrides --json values (Rule 5 from the design doc).

```bash
echo 'overridden' > /private/tmp/mcp_override.txt && cargo run --quiet 2>/dev/null -- --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /private/tmp' --call read_file --json '{"path": "/private/tmp/mcp_flat_arg.txt"}' --arg path=/private/tmp/mcp_override.txt
```

```output
overridden

```

## Type Inference

--arg values automatically detect types: integers, floats, booleans, null, and strings.

```bash
cargo test --quiet mcp_client::tests::test_parse_scalar 2>&1 | grep 'test result'
```

```output
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 164 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
```

## Config Fields

New config.yaml fields with sensible defaults: mcp_cache_ttl (3600s), mcp_startup_timeout (30s), mcp_call_timeout (120s), mcp_max_connections (10). Connection pool enforces idle timeout (5 min) and max connections limit.

```bash
cargo test --quiet mcp_client::tests::test_config_mcp_defaults 2>&1 | grep -E 'test |test result'
```

```output
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 171 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
```

## Full Test Suite

```bash
cargo test --quiet 2>&1 | grep 'test result'
```

```output
test result: ok. 172 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

## Error Handling

Invalid --arg format produces a clear error.

```bash
./target/debug/aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --call read_file --arg noequals 2>&1 || true
```

```output
Error: Invalid --arg format 'noequals': expected KEY=VALUE
```
