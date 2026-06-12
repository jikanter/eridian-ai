# SPEC-001: Eridian Trace Format

**Version:** 0.1
**Status:** Draft, ready for implementation
**Owners:** project lead

This is the contract. Downstream consumers — promptfoo regression assertions,
control-flow integration tests, Inspect AI evals (deferred), training data
pipelines (deferred) — depend on this spec being stable. Schema changes are
deliberate and bump `schema_version`.

## 1. File layout

All paths relative to `${XDG_STATE_HOME:-$HOME/.local/state}/aichat/`.

```text
$XDG_STATE_HOME/aichat/
├── traces/
│   ├── manifest.jsonl              # one entry per parent session
│   ├── turn-<turn_session_id>.jsonl
│   ├── turn-<turn_session_id>.jsonl
│   └── ...
└── blobs/
    ├── ab/cd/ef0123...             # SHA-256, sharded by first 4 hex chars
    └── ...
```

- **Per-turn JSONL files.** One file per conversational turn, named
  `turn-<turn_session_id>.jsonl`. The session ID is a [ULID](https://github.com/ulid/spec)
  so files sort chronologically.
- **Manifest.** A single `manifest.jsonl` lists parent-session bindings:
  `{"parent_session_id": "...", "turn_session_id": "...", "ts_ns": ...}`.
  Tailable to watch a multi-turn conversation as it unfolds.
- **Blob store.** `blobs/<sha256>` content-addressed. Sharded two levels deep
  by the first four hex characters to avoid pathological directory sizes
  (e.g., `ab/cd/ef0123...`).

### Configuration

- `--trace-out <path>` — override the JSONL path for this invocation. Useful
  for tests; sets the file path directly rather than the directory.
- `--no-trace` — disable tracing for this invocation.
- `AICHAT_TRACE=0` — disable tracing globally.
- `AICHAT_TRACE_DIR` — override the base directory.
- `AICHAT_TRACE_CHANNEL_CAPACITY` — override the bounded-channel size
  (default 1024). See `ADR-0003`.

**Default behavior is on.** Traces are too valuable as accumulating training
data to be opt-in. Throughput is low enough that storage cost is negligible.

## 2. Wire format

JSONL, one event per line, UTF-8, LF line endings. Every event has the same
envelope:

```json
{
  "schema_version": "0.1",
  "session_id": "01HQ...",
  "parent_session_id": "01HQ...",
  "seq": 47,
  "ts_ns": 1729872000123456789,
  "type": "provider.retry",
  "data": { ... }
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `schema_version` | string | yes | `"0.1"` for this version. Present on every event. |
| `session_id` | string | yes | ULID. Per-turn. Matches the file name's `<turn_session_id>`. |
| `parent_session_id` | string \| null | yes | ULID of the parent (multi-turn) session if any. `null` for one-shot invocations. |
| `seq` | uint | yes | Monotonic counter within `session_id`, starting at 0. Two events with the same `ts_ns` are ordered by `seq`. |
| `ts_ns` | uint | yes | Wall-clock nanoseconds since UNIX epoch. |
| `type` | string | yes | Dotted hierarchical event type. See §3. |
| `data` | object | yes | Type-specific payload. May be `{}`. |

`schema_version` is on every event (not just `session.start`) because
streaming consumers may attach mid-file. Consumers MUST reject events with
unknown major versions.

## 3. Event types

Thirteen types, grouped. All payloads documented under `data:`.

### 3.1 Session lifecycle

#### `session.start`

Emitted once at the start of a turn, before any other event.

```json
{
  "data": {
    "aichat_version": "0.30.0+eridian.1",
    "config_hash": "<sha256 of effective config>",
    "role": "rust-reviewer" | null,
    "model_spec": "anthropic:claude-opus-4-7",
    "fixture_id": "<test fixture name>" | null,
    "cwd": "/path/to/working/dir",
    "args": ["aichat", "--role", "rust-reviewer", "..."],
    "env_subset": {
      "AICHAT_CONFIG_DIR": "/...",
      "OPENAI_API_BASE": "http://localhost:1234"
    },
    "entity_id": "rust-reviewer" | null,
    "facets": ["Act:referenced", "Shape:owned"]
  }
}
```

- `fixture_id` is set by tests via `AICHAT_FIXTURE_ID` env var. `null` in
  normal user invocations. Lets test consumers filter traces.
- `env_subset` includes only env vars relevant to aichat behavior. See
  §6 for the allowlist. Values are redacted per §6.
- `entity_id` (Phase 52D) is the resolved entity's stable, addressable id —
  the cross-preset (Prompt/Role/Agent/Macro) attribution key Phase 49 reads.
  `null` when no entity resolved. Distinct from `role`: `role` is the human
  label, `entity_id` is the stable join key. At the `call_react` keystone the
  resolved entity is the synthesized `Role` (`to_role()`); for an agent the
  facets therefore reflect the resolved role, not the agent directory's owned
  facets — the richer owned view lands with Phase 52C.
- `facets` (Phase 52D) is the resolved facet token set actually used this turn,
  each entry a `Family:ownership` token (`owned` / `referenced`) from the closed
  six-family taxonomy (Know·Act·Shape·Govern·Compose·Judge — see
  `entity-model.md` §4). Sorted and stable; a family present under both
  ownerships yields two tokens. Empty array when the entity carries no facets.
  This is a machine GROUP BY key, not a display string (cf. `--dry-run`'s
  `Act(ref), Shape(owned)` rendering).
- **Versioning:** `entity_id` and `facets` are **additive optional fields** per
  §5, so `schema_version` stays `"0.1"`. Consumers predating Phase 52D ignore
  them; consumers reading them MUST tolerate `facets: []` and `entity_id: null`.

#### `session.end`

Emitted once at turn end, before the writer thread is released.

```json
{
  "data": {
    "exit_status": 0,
    "wall_time_ns": 1234567890,
    "tokens_in_total": 1024,
    "tokens_out_total": 512,
    "cost_usd": 0.0123 | null
  }
}
```

### 3.2 Context assembly

#### `context.system_prompt_built`

```json
{
  "data": {
    "content_hash": "sha256:...",
    "byte_len": 4096
  }
}
```

Full system prompt stored in blob store under `content_hash`.

#### `context.role_applied`

```json
{
  "data": {
    "role_name": "rust-reviewer",
    "tool_whitelist": ["fs_read", "fs_grep"] | null,
    "rag_sources_enabled": ["docs/", "examples/"]
  }
}
```

`tool_whitelist: null` means no whitelist (all tools allowed).

#### `context.rag_retrieved`

Emitted once per RAG query. Multiple events per turn are possible.

```json
{
  "data": {
    "query": "<text>",
    "hits": [
      {
        "source_id": "docs/api.md",
        "chunk_id": "<id>",
        "score": 0.87,
        "included": true,
        "content_hash": "sha256:..."
      }
    ],
    "top_k": 5,
    "score_threshold": 0.5
  }
}
```

### 3.3 Provider interaction

#### `provider.request`

```json
{
  "data": {
    "request_id": "<uuid>",
    "provider": "anthropic",
    "model": "claude-opus-4-7",
    "params": {
      "temperature": 0.7,
      "max_tokens": 4096,
      "stream": true
    },
    "messages_hash": "sha256:...",
    "request_body_bytes": 8192,
    "endpoint": "https://api.anthropic.com/v1/messages"
  }
}
```

`messages_hash` is a hash of the canonicalized message list. Full bodies
go to the blob store under that hash.

#### `provider.response`

```json
{
  "data": {
    "request_id": "<uuid>",
    "request_body_hash": "sha256:...",
    "status": 200,
    "finish_reason": "stop" | "length" | "tool_use" | "content_filter" | "error",
    "tokens_in": 1024,
    "tokens_out": 512,
    "latency_ns": 1234567890,
    "response_body_hash": "sha256:..."
  }
}
```

Self-contained: includes `request_body_hash` so streaming consumers can
correlate without reading backward. See `ADR-0002`.

#### `provider.retry`

First-class event. Emitted per retry attempt, *not* once per request.

```json
{
  "data": {
    "request_id": "<uuid>",
    "attempt": 2,
    "trigger": "http_5xx",
    "details": "HTTP 502 Bad Gateway",
    "backoff_ms": 1000,
    "will_fallback": false
  }
}
```

`trigger` is one of:

- `timeout` — request exceeded configured timeout
- `http_5xx` — server error
- `http_4xx` — client error (only for retryable 4xxs like 408, 429)
- `rate_limit` — explicit 429 with retry-after
- `parse_error` — response body could not be parsed
- `stream_interrupted` — streaming response was cut off mid-stream
- `connection_error` — TCP/TLS-level failure

`will_fallback: true` indicates the next attempt will use a different
provider. A `provider.fallback` event follows.

#### `provider.fallback`

```json
{
  "data": {
    "from_provider": "anthropic",
    "from_model": "claude-opus-4-7",
    "to_provider": "openai",
    "to_model": "gpt-5",
    "reason": "max_retries_exceeded"
  }
}
```

### 3.4 Tool interaction

#### `tool.requested`

The model asked for a tool call. Not yet executed or denied.

```json
{
  "data": {
    "tool_call_id": "<id>",
    "tool_name": "fs_read",
    "args": { "path": "src/main.rs" },
    "args_hash": "sha256:..."
  }
}
```

`args` inline if small (<1KB total); otherwise reference `args_hash` and
move to blob store.

#### `tool.denied`

The whitelist or policy blocked the call.

```json
{
  "data": {
    "tool_call_id": "<id>",
    "tool_name": "shell_exec",
    "reason": "not_in_whitelist",
    "policy": "role:rust-reviewer"
  }
}
```

#### `tool.executed`

```json
{
  "data": {
    "tool_call_id": "<id>",
    "tool_name": "fs_read",
    "exit_status": 0,
    "duration_ns": 12345678,
    "stdout_bytes": 4096,
    "stdout_hash": "sha256:...",
    "stderr_bytes": 0,
    "stderr_hash": null,
    "stdout_truncated": false
  }
}
```

If stdout exceeds the configured cap (default 1MB), the truncated content
is hashed and stored, and `stdout_truncated: true` is set.

### 3.5 Output

#### `output.final`

```json
{
  "data": {
    "content_hash": "sha256:...",
    "byte_len": 2048,
    "tokens_out": 512
  }
}
```

#### `output.chunk` (verbose only)

Streaming output chunks. Disabled by default; enable with
`AICHAT_TRACE_VERBOSE=1`. Useful for debugging stream interruption tests.

```json
{
  "data": {
    "request_id": "<uuid>",
    "chunk_index": 17,
    "content": "<chunk text>",
    "delta_tokens": 4
  }
}
```

### 3.6 Errors

#### `error`

Catch-all for failures not handled by `provider.retry` (config errors,
panics rescued by `catch_unwind`, malformed responses that exhausted
retries).

```json
{
  "data": {
    "kind": "config" | "panic" | "exhausted_retries" | "unknown",
    "message": "<human-readable summary>",
    "context": { ... } | null
  }
}
```

### 3.7 Trace meta

#### `trace.heartbeat`

Emitted every `HEARTBEAT_INTERVAL` (default 30s) when no other events have
flowed. Lets `tail -f` consumers know aichat is alive during slow operations.

```json
{
  "data": {
    "uptime_ns": 1234567890
  }
}
```

#### `trace.dropped`

Emitted when the writer recovers after the bounded channel was full. See
`ADR-0003`.

```json
{
  "data": {
    "count": 17,
    "since_seq": 42
  }
}
```

`count` is the number of events dropped since `since_seq`. Consumers that
care about exact ordering can detect drops and mark gaps.

## 4. Blob store

Large payloads (full prompts, full RAG contexts, tool stdout, request /
response bodies) are stored under `blobs/<sha256>` and referenced from
events via `*_hash` fields.

### Rules

- **Content-addressed.** SHA-256 of the bytes. Identical content
  deduplicates across events and across sessions.
- **Sharded.** First 4 hex chars become two directory levels:
  `blobs/ab/cd/ef0123...`. Avoids inode pressure on `tmpfs` and
  filesystem stalls on large directory listings.
- **Write-once.** If a blob with the same hash already exists, skip the
  write. Use `O_EXCL` semantics or a check-then-link pattern.
- **No automatic GC.** The trace store grows monotonically. A
  `aichat trace gc` command (out of scope for v0.1) can prune blobs
  unreferenced by any current trace file.

### Resolution

`aichat trace show <session_id>` resolves blobs inline for human
inspection. Implementation detail; not part of this spec.

## 5. Versioning

- `schema_version` is `"0.1"` for this document.
- Breaking changes (event field removed, event type semantics changed,
  required field added) bump to `"0.2"`, etc. Consumers MUST reject
  unknown major versions.
- Additive changes (new optional field, new event type) do **not** bump
  the version. Consumers MUST ignore unknown event types and unknown
  fields gracefully.
- v0.x is explicitly pre-stable. Expect breaking changes between minor
  versions until v1.0. Phase 1 may produce a v0.2 within weeks of v0.1
  based on test-harness feedback.

## 6. Redaction

The trace MUST NOT contain plaintext secrets. A redaction layer runs
before events hit disk.

### Default rules (v0.1)

- Any `env_subset` value matching common API-key patterns is replaced with
  `"<redacted:{key_name}>"`. Patterns:
  - `*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`
  - Values matching common provider key prefixes: `sk-`, `xai-`, `pk_`,
    `Bearer ` (with anything following)
- HTTP `Authorization` and `X-Api-Key` headers in stored request bodies
  are stripped before hashing. (Do this before computing the
  `messages_hash` so reproducibility is unaffected by which key was used.)
- Tool stdout and stderr pass through unchanged by default. Users can
  configure additional regex-based redaction patterns in
  `~/.config/aichat/redaction.yaml`.

### `env_subset` allowlist (v0.1)

Only these env vars appear in `session.start.env_subset`:

```text
AICHAT_CONFIG_DIR
AICHAT_FIXTURE_ID
AICHAT_TRACE
AICHAT_TRACE_DIR
AICHAT_TRACE_VERBOSE
OPENAI_API_BASE
ANTHROPIC_API_BASE
HOME
PWD
USER
LANG
LC_ALL
```

Other env vars are dropped entirely. API-key vars are explicitly NOT in
the allowlist; `env_subset` never includes secret material.

## 7. Streaming-safety invariants

These are enforced by code review and by tests in the `eridian-trace` crate.
See `ADR-0002`.

1. **Atomic line writes.** Each event serialized to a `Vec<u8>` ending in
   `\n`, then written with one `write_all` followed by `flush`. No
   partial writes.
2. **No forward references.** Every event is interpretable on its own,
   given the blob store.
3. **Causal ordering by emission.** `provider.retry` emits after the
   `provider.request` it relates to. `provider.response` emits after the
   final retry. Consumers can rely on this without reordering.
4. **Crash safety.** A crash mid-event can leave at most one truncated
   line. Consumers MUST tolerate trailing junk on a partial line.

## 8. Implementation invariants

These are enforced in code, not just by spec text:

1. **`seq` is generated by the writer thread**, not the producer. This
   ensures `seq` reflects on-disk order even if events arrive out of
   order on the channel (which can happen with multiple senders).
2. **`ts_ns` is captured at producer side**, not writer side. The
   producer is the source of truth for when an event happened.
3. **`schema_version` is constant for a binary build**, not a runtime
   value. Hardcoded.
4. **The `TraceSender` is `Clone + Send + Sync`**, cheap to clone, and
   may be used from any tokio task or thread.

## 9. Open questions to resolve before v1.0

These are deliberately deferred but tracked:

- **Per-event durability.** v0.1 flushes but doesn't fsync per event. If
  stronger durability is needed, add a verbose mode.
- **Manifest concurrency.** Two parallel turns of the same parent session
  appending to `manifest.jsonl` need either a lock or a single-writer
  invariant. v0.1 assumes turns are sequential within a parent session
  (which is true for REPL mode but not for the future `--serve` HTTP
  path). Revisit when `--serve` becomes a tested target.
- **Schema documentation format.** Inline in this Markdown for v0.1.
  Migrate to JSON Schema files generated from Rust types when v0.2 lands.
- **Cross-language consumer libraries.** Phase 2 needs JS (for
  promptfoo's `javascript` assertions) and Rust (for the integration
  tests). Phase 3 will want Python (for Inspect AI). Keep the schema
  simple enough that hand-written consumers stay practical.
- **Compression.** JSONL compresses well; if storage growth becomes a
  concern, gzip rotated trace files. Out of scope for v0.1.

## 10. Acceptance criteria for SPEC-001 v0.1

A correctly-implemented Eridian trace emitter satisfies all of the
following, verified by tests:

1. Every aichat invocation produces exactly one `turn-<id>.jsonl` file
   that begins with `session.start` and ends with `session.end`.
2. Every event has every required envelope field, populated correctly.
3. `seq` is strictly monotonic within a file, starting at 0.
4. The blob store contains all referenced hashes; no event references a
   hash that's missing.
5. `env_subset` contains no values matching common API-key patterns.
6. A consumer doing `tail -f turn-<id>.jsonl` sees events appear within
   100ms of the underlying operation.
7. Under burst load (10,000 events/sec sustained for 1 second), the
   request path's p99 latency does not increase by more than 5%.
8. After a synthetic crash mid-turn (SIGKILL), the trace file contains
   all events emitted before the crash, possibly plus one truncated
   trailing line.
