# Phase 8: Data Processing & Observability

*2026-03-16T22:24:00Z by Showboat 0.6.1*
<!-- showboat-id: 4dc4903e-baa1-4152-8743-bf97730205f0 -->

Phase 8 connects the existing-but-disconnected pricing/token infrastructure into actionable observability, and adds batch record processing. This demo verifies all seven sub-items: 8D (headless RAG), 8A1 (cost accounting), 8F/8G (interaction trace), 8A2 (pipeline trace), 8C (record field templating), and 8B (batch processing).

## Build verification

All 317 tests pass (144 unit + 173 compatibility) with no failures.

```bash
cargo test 2>&1 | grep '^test result'
```

```output
test result: ok. 144 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```bash
cargo build 2>&1 | tail -1
```

```output
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
```

## 8A1: New CLI flags

The `--cost`, `--trace`, `--each`, and `--parallel` flags are registered in the CLI.

```bash
./target/debug/aichat --help 2>&1 | grep -E '^\s+--(cost|trace|each|parallel)'
```

```output
      --cost
      --trace
      --each
      --parallel <PARALLEL>
```

## 8A1: CallMetrics & compute_cost unit tests

New unit tests verify cost arithmetic and metrics merging.

```bash
cargo test -- call_metrics compute_cost 2>&1 | grep -E 'running|test.*ok|test result'
```

```output
running 4 tests
test client::common::tests::test_call_metrics_merge ... ok
test client::common::tests::test_call_metrics_merge_empty_model_id ... ok
test client::common::tests::test_compute_cost_no_prices ... ok
test client::common::tests::test_compute_cost_with_prices ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 140 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
```

## 8C: Record field templating unit tests

`interpolate_record_fields` supports `{{.}}` for full record and `{{.field}}` for JSON field extraction.

```bash
cargo test -- interpolate_record 2>&1 | grep -E 'running|test.*ok|test result'
```

```output
running 5 tests
test utils::variables::tests::test_interpolate_record_fields_full_record ... ok
test utils::variables::tests::test_interpolate_record_fields_full_json_record ... ok
test utils::variables::tests::test_interpolate_record_fields_missing_field ... ok
test utils::variables::tests::test_interpolate_record_fields_json_field ... ok
test utils::variables::tests::test_interpolate_record_fields_non_json ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.01s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
```

## 8F/8G: Trace module unit tests

The TraceEmitter correctly truncates content and handles newlines.

```bash
cargo test -- utils::trace 2>&1 | grep -E 'running|test.*ok|test result'
```

```output
running 3 tests
test utils::trace::tests::test_truncate_newlines ... ok
test utils::trace::tests::test_truncate_short ... ok
test utils::trace::tests::test_truncate_long ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 141 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
```

## 8D: Headless RAG

The `IS_STDOUT_TERMINAL` bail was replaced with a non-interactive config-defaults path.

```bash
grep -n 'IS_STDOUT_TERMINAL' src/rag/mod.rs | head -5
```

```output
66:        let (embedding_model, chunk_size, chunk_overlap) = if *IS_STDOUT_TERMINAL {
98:            if !*IS_STDOUT_TERMINAL {
111:        if rag.save()? && *IS_STDOUT_TERMINAL {
433:            if *IS_STDOUT_TERMINAL && total > 0 {
```

```bash
grep -A2 'Non-interactive' src/rag/mod.rs
```

```output
            // Non-interactive: use config defaults without prompts
            let config_r = config.read();
            let emb_id = config_r
```

## 8A1: Return type changes

All three core functions now return `CallMetrics` as a third tuple element.

```bash
grep -n 'Result<(String, Vec<ToolResult>, CallMetrics)>' src/client/common.rs
```

```output
447:) -> Result<(String, Vec<ToolResult>, CallMetrics)> {
584:) -> Result<(String, Vec<ToolResult>, CallMetrics)> {
630:) -> Result<(String, Vec<ToolResult>, CallMetrics)> {
```

## 8A1: SseHandler usage tracking

Streaming handlers now capture token counts via `set_usage()` and return them from `take()`.

```bash
grep -n 'set_usage\|input_tokens.*Option<u64>' src/client/stream.rs
```

```output
16:    input_tokens: Option<u64>,
76:    pub fn set_usage(&mut self, input_tokens: Option<u64>, output_tokens: Option<u64>) {
```

```bash
grep -n 'set_usage' src/client/openai.rs src/client/claude.rs
```

```output
src/client/openai.rs:151:            handler.set_usage(
src/client/claude.rs:150:                        handler.set_usage(
```

## 8A1: JSONL run log ledger

New `ledger.rs` module provides `append_run_log()` for cost tracking. Activated via `AICHAT_RUN_LOG` env var.

```bash
cat src/utils/ledger.rs
```

```output
use anyhow::{Context, Result};
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// Append one JSONL record to the run log file.
pub fn append_run_log(path: &Path, record: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create run log directory: {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open run log: {}", path.display()))?;
    let line = serde_json::to_string(record)?;
    writeln!(file, "{line}")?;
    Ok(())
}
```

## 8A2: Pipeline trace metadata

`StageTrace` struct collects per-stage metrics. When `-o json` is used with pipelines, a trace envelope wraps the output.

```bash
grep -A7 'struct StageTrace' src/pipe.rs
```

```output
struct StageTrace {
    role: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    latency_ms: u64,
}
```

## 8B: Batch record processing

`batch_execute()` reads stdin line-by-line with `--each`. Supports `--parallel N` for concurrent execution via `buffer_unordered`.

```bash
grep -n 'async fn batch_execute\|async fn process_one_record' src/main.rs
```

```output
568:async fn batch_execute(
637:async fn process_one_record(
```

```bash
grep -c 'buffer_unordered' src/main.rs
```

```output
1
```

## 8F/8G: TraceEmitter integration

The trace emitter hooks into `call_react` — emitting per-turn request info, tool results, and a final summary.

```bash
grep -n 'emit_request\|emit_tool_results\|emit_summary' src/client/common.rs
```

```output
479:            t.emit_request(
497:                t.emit_summary(&cumulative_metrics);
511:            t.emit_tool_results(&tool_results, metrics.latency_ms);
```

## Files changed summary

**New files (2):** `src/utils/ledger.rs`, `src/utils/trace.rs`

**Modified files (12):** `src/client/common.rs`, `src/client/stream.rs`, `src/client/openai.rs`, `src/client/claude.rs`, `src/cli.rs`, `src/config/mod.rs`, `src/main.rs`, `src/pipe.rs`, `src/repl/mod.rs`, `src/rag/mod.rs`, `src/utils/variables.rs`, `src/utils/mod.rs`

**Tests:** 317 total (144 unit + 173 compatibility), 12 new unit tests added.
