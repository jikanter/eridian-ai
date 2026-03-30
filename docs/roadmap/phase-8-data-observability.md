# Phase 8: Data Processing & Observability

**Status:** Done

---

> **[CHANGED 2026-03-15]** 8A expanded into 8A1–8A2. New items 8F (interaction trace) and 8G (trace JSONL) added.
> These address a blind spot: the existing DEBUG log shows raw API request/response JSON but does NOT surface
> model output content cleanly, does NOT trace multi-turn `call_react` loops (tool_call → tool_result → next response),
> and does NOT show cost/token summaries. Informed by openclaw's stage-based JSONL cache-trace and per-turn
> `model.usage` diagnostic event patterns. See conversation on 2026-03-15 for full analysis.

| Item | Status | Notes |
|---|---|---|
| 8A1. Run log & cost accounting | Done | `CallMetrics` struct, `compute_cost()`, `--cost` flag, JSONL ledger via `AICHAT_RUN_LOG` env var. All call sites return metrics. |
| 8A2. Pipeline trace metadata | Done | `StageTrace` struct, `-o json` wraps pipeline output in envelope with per-stage metrics and totals. |
| 8B. Batch record processing (`--each`) | Done | `--each` reads stdin line-by-line, `--parallel N` for concurrent execution. `batch_execute` + `process_one_record`. |
| 8C. Record field templating (`{{.field}}`) | Done | `interpolate_record_fields()` in `variables.rs`. `{{.}}` = full record, `{{.field}}` = JSON field. |
| 8D. Headless RAG | Done | Non-interactive path in `Rag::init` uses config defaults. Guards `add_documents()` for non-terminal. |
| 8F. Interaction trace (`--trace`) | Done | `TraceEmitter` in `trace.rs`, human-readable stderr output per `call_react` turn with tool calls, tokens, latency. |
| 8G. Trace JSONL (`AICHAT_TRACE=1`) | Done | Env-var gated JSONL trace. `AICHAT_TRACE_FILE` for file output. Per-turn and summary events. |

**8A1/8A2 — Cost wiring.** The infrastructure exists but is disconnected. `ModelData` carries `input_price`/`output_price` (loaded from `models.yaml`). Every API response populates `input_tokens`/`output_tokens` in `ChatCompletionsOutput`. The multiplication never happens — prices only appear in `--list-models`, token counts only in `serve.rs`. Phase 8A1 connects them into a ledger; 8A2 extends the `-o json` pipeline envelope with per-stage accounting.

Run log record:
```jsonl
{"ts":"2026-03-14T10:23:01Z","run_id":"a1b2c3","model":"claude:claude-sonnet-4-6","role":"classify","input_tokens":1847,"output_tokens":423,"cost_usd":0.012,"exit_code":0,"latency_ms":2340}
```

Pipeline trace envelope (`-o json`):
```json
{
  "output": "...",
  "trace": {
    "stages": [
      {"role": "extract", "model": "deepseek:deepseek-chat", "input_tokens": 892, "output_tokens": 341, "cost_usd": 0.0003, "latency_ms": 1100},
      {"role": "review", "model": "claude:claude-sonnet-4-6", "input_tokens": 341, "output_tokens": 423, "cost_usd": 0.012, "latency_ms": 2340}
    ],
    "total_cost_usd": 0.0123,
    "total_latency_ms": 3440
  }
}
```

**8F/8G — Interaction trace.** **[ADDED 2026-03-15]** Today's DEBUG log has a critical blind spot: multi-turn `call_react` loops are opaque. The raw `non-stream-data` JSON contains model output, but you cannot see: (a) what the model actually said, (b) what tool results came back, (c) how many turns the agent loop took, (d) whether `output_schema` validation passed. This was identified by tracing `aichat --role "data-discoverer" "ruby programming"` — the model called `web_search` but the trace stopped there.

**Design principles** (informed by openclaw's `cache-trace.ts` and `diagnostic-events.ts`):

- **Stage-based, not continuous.** One record per `call_react` turn, not per byte. Low overhead.
- **Stderr for humans, JSONL for machines.** `--trace` prints a compact summary; `AICHAT_TRACE=1` writes structured JSONL.
- **Truncation by default.** Tool results capped at 500 chars in trace output. Full payloads stay in DEBUG log.
- **Composable with `--cost`.** `--trace` subsumes `--cost` (includes token/cost per turn plus totals). Using both is redundant but not an error.

`--trace` stderr output example (multi-turn tool-calling invocation):
```text
[1] → qwen3-coder  1tok in  24tok out  0.3s
    ← tool_call: web_search({"query":"ruby programming datasets"})
[2] ← web_search  exit=0  1.2s  (342 chars)
[3] → qwen3-coder  892tok in  341tok out  2.1s
    ← {"datasets": [{"name": "Ruby Quiz",...}]}
    ✓ output_schema valid
total: 3 turns  893tok in  365tok out  $0.001  3.6s
```

`AICHAT_TRACE=1` JSONL output (same invocation):
```jsonl
{"ts":"2026-03-15T22:36:59Z","turn":1,"direction":"request","model":"qwen3-coder","input_tokens":1,"output_tokens":24,"latency_ms":300,"tool_calls":[{"name":"web_search","args":{"query":"ruby programming datasets"}}]}
{"ts":"2026-03-15T22:37:00Z","turn":2,"direction":"tool_result","tool_name":"web_search","exit_code":0,"latency_ms":1200,"content_length":342}
{"ts":"2026-03-15T22:37:02Z","turn":3,"direction":"request","model":"qwen3-coder","input_tokens":892,"output_tokens":341,"latency_ms":2100,"content_summary":"{\"datasets\": [{\"name\": \"Ruby Quiz\",...}]}","schema_valid":true}
{"ts":"2026-03-15T22:37:02Z","turn":0,"direction":"summary","total_turns":3,"total_input_tokens":893,"total_output_tokens":365,"total_cost_usd":0.001,"total_latency_ms":3600}
```

**Implementation hook points:**

- `src/client/common.rs` — `call_react` loop: emit trace record after each API response and after each tool result batch.
- `src/function.rs` — `eval_tool_calls`: emit tool result trace records (one per tool, with exit code and truncated output).
- New: `src/utils/trace.rs` — Trace emitter. Holds `TraceConfig { enabled, format (stderr|jsonl), file_path, truncate_at }`. Initialized from `--trace` flag and `AICHAT_TRACE`/`AICHAT_TRACE_FILE` env vars. Writes to stderr (human) or file (JSONL).

**What NOT to build (informed by openclaw evaluation):**

| Proposal | Reason |
|---|---|
| In-memory event bus / diagnostic listener API | Over-engineered for CLI. JSONL is the interface; downstream tools (`jq`, `duckdb`) are the listeners. |
| Message fingerprinting (SHA256 digests) | Useful for servers with session persistence. aichat is single-shot CLI — the full trace is short enough to store directly. |
| OpenTelemetry export | Wrong abstraction for Unix CLI. JSONL traces can be ingested by any OTEL collector via file receiver if needed. |
| Payload deduplication | aichat doesn't have persistent sessions across invocations. Each trace is self-contained. |
| Configurable per-stage inclusion (messages/prompt/system toggles) | Premature. `--trace` shows the summary; DEBUG log shows everything. Two levels is enough. |

**8B/8C — Record processing.** `--each` is the minimal batch primitive. Everything else — schema validation (`input_schema`/`output_schema`), lifecycle hooks (`pipe_to`/`save_to`), output formatting (`-o jsonl`) — already works per-invocation. `--each` adds only the iteration loop. `{{.field}}` adds only field extraction. Together they compose with the full feature set of whichever entity type is invoked.

**8B/8C work uniformly across all entity types because they are input-level features, resolved before entity dispatch:**

| Entity | `--each` | `{{.field}}` in... | Mechanism |
|---|---|---|---|
| Role (`-r`) | Yes | Prompt template | Fields interpolated alongside `{{var}}` and `{{$VAR}}` |
| Agent (`-a`) | Yes | `instructions` template | Fields interpolated via same path as `{{__tools__}}` |
| Macro (`--macro`) | Yes | Step interpolation | Fields available alongside positional `{{var}}` in each step |
| Prompt (bare) | Yes | Prompt text | Fields interpolated before sending |

**Template interpolation namespaces (cumulative with existing):**
```
{{var}}       Role/agent declared variable (-v key=value, --agent-variable)
{{$VAR}}      Environment variable
{{.field}}    Record field from current --each input line (Phase 8C)
{{.}}         Full record (entire input line)
{{timestamp}} Built-in (lifecycle hooks only)
```

**Example — JSONL dataset with a role:**
```bash
# classify.md has output_schema enforcing {"label": "string", "confidence": "number"}
cat emails.jsonl | aichat -r classify -o jsonl --each --parallel 4
```
Role prompt uses `{{.subject}}` and `{{.body}}`. `output_schema` validates each response. Output: one JSONL line per input.

**Example — JSONL dataset with an agent:**
```bash
cat tickets.jsonl | aichat -a triage-agent --each --parallel 2
```
Agent `instructions` uses `{{.title}}` and `{{.description}}`. Agent tools and RAG available per-invocation (8D required for RAG).

**Example — JSONL dataset with a macro:**
```yaml
# macros/enrich.yaml
variables:
  - name: model
    default: "openai:gpt-4o-mini"
steps:
  - ".role enricher -m {{model}}"
  - "Enrich: {{.}}"
```
```bash
cat records.jsonl | aichat --macro enrich --each
```

**8D — Headless RAG.** `Rag::init` currently calls `bail!("Failed to init rag in non-interactive mode")` when `!IS_STDOUT_TERMINAL`. This blocks any pipeline or automation use of agent RAG. The config defaults (`rag_embedding_model`, `rag_chunk_size`, `rag_chunk_overlap`) already exist — the fix is to use them instead of prompting interactively. Prerequisite for 8B to work with RAG-enabled agents.

**What to kill:**

| Proposal | Reason |
|---|---|
| `--resume` / checkpoint in `--each` | Unix composition: `tail -n +N` the input and re-run. Stateless batch is simpler. |
| Windowing / aggregation / streaming | `--each` processes one line at a time. Aggregation belongs downstream (`jq`, `duckdb`). |
| Per-record retry logic | Failed records emit structured errors (Phase 4C) on stderr. Filter and re-process. |
| Cost dashboard / visualization | JSONL run log is the interface. Pipe to `jq`, `duckdb`, Grafana. |
| `{{.field.nested}}` deep access | Premature. If needed, the role prompt can instruct the model to extract nested fields. |
