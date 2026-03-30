# Phase 8: Data Processing & Observability

*2026-03-30T15:48:42Z by Showboat 0.6.1*
<!-- showboat-id: 1edf13dd-1740-4994-8087-af378cb1bdf6 -->

Phase 8 connects pricing/token infrastructure into actionable observability and adds batch record processing.

## Tests

```bash
cargo test 2>&1 | grep "test result:" | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 144 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```

## 8A1: Cost Accounting

New `--cost` and `--trace` CLI flags:

```bash
aichat --help 2>&1 | grep -E "^\s+--(cost|trace|each|parallel)"
```

```output
      --cost
      --trace
      --each
      --parallel <PARALLEL>
```

CallMetrics unit tests:

```bash
cargo test -- call_metrics compute_cost 2>&1 | grep -E "test.*ok|test result" | sort
```

```output
test client::common::tests::test_call_metrics_merge ... ok
test client::common::tests::test_call_metrics_merge_empty_model_id ... ok
test client::common::tests::test_compute_cost_no_prices ... ok
test client::common::tests::test_compute_cost_with_prices ... ok
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 140 filtered out; finished in 0.00s
```

## 8C: Record Field Templating

`{{.}}` for full record, `{{.field}}` for JSON field extraction:

```bash
cargo test -- interpolate_record 2>&1 | grep -E "test.*ok|test result" | sort
```

```output
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.00s
test utils::variables::tests::test_interpolate_record_fields_full_json_record ... ok
test utils::variables::tests::test_interpolate_record_fields_full_record ... ok
test utils::variables::tests::test_interpolate_record_fields_json_field ... ok
test utils::variables::tests::test_interpolate_record_fields_missing_field ... ok
test utils::variables::tests::test_interpolate_record_fields_non_json ... ok
```

## 8F/8G: Trace Module

```bash
cargo test -- utils::trace 2>&1 | grep -E "test.*ok|test result" | sort
```

```output
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 141 filtered out; finished in 0.00s
test utils::trace::tests::test_truncate_long ... ok
test utils::trace::tests::test_truncate_newlines ... ok
test utils::trace::tests::test_truncate_short ... ok
```

## 8D: Headless RAG

The `IS_STDOUT_TERMINAL` bail was replaced with a non-interactive config-defaults path:

```bash
grep -n "IS_STDOUT_TERMINAL" src/rag/mod.rs | head -5
```

```output
66:        let (embedding_model, chunk_size, chunk_overlap) = if *IS_STDOUT_TERMINAL {
98:            if !*IS_STDOUT_TERMINAL {
111:        if rag.save()? && *IS_STDOUT_TERMINAL {
433:            if *IS_STDOUT_TERMINAL && total > 0 {
```

## 8A2: Pipeline Trace Metadata

```bash
grep -A7 "struct StageTrace" src/pipe.rs
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

## 8B: Batch Record Processing

```bash
grep -n "async fn batch_execute\|async fn process_one_record" src/main.rs
```

```output
568:async fn batch_execute(
637:async fn process_one_record(
```

## 8A1: Run Log Ledger

```bash
wc -l < src/utils/ledger.rs
```

```output
      20
```

## Integration Tests

Verify CLI flags are registered:

```bash
aichat --help 2>&1 | grep -c -E "(cost|trace|each|parallel)"
```

```output
7
```

Test record field templating via `--dry-run` with a role that uses `{{.}}` placeholders:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-record-tmpl.md" <<'ROLE'
Process this record: {{.}}
Extract the name field: {{.name}}
ROLE
echo "{\"name\": \"Alice\", \"age\": 30}" | aichat --dry-run -r test-record-tmpl 2>/dev/null
rm "$ROLES_DIR/test-record-tmpl.md"
```

```output
Process this record: {{.}}
Extract the name field: {{.name}}

{"name": "Alice", "age": 30}
```

Record field templating resolves `{{.}}` to the full input and `{{.name}}` to the extracted JSON field.
