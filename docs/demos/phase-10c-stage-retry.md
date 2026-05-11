# Phase 10C: Pipeline Stage Retry

*2026-04-17T01:13:39Z by Showboat 0.6.1*
<!-- showboat-id: 581b990f-44ec-4ed2-83ab-c43258819905 -->

Phase 10C wraps `run_stage_inner` in a retry loop. On a transient failure (network / API / model / schema — exit codes 5, 6, 7, 8), the stage is retried up to `stage_retries` times (default 1) before the error is wrapped into `AichatError::PipelineStage` and propagated. Non-transient failures (config, auth, usage, tool, abort) fail fast — hammering them would waste tokens. Classification reuses the existing `classify_error` chain walker, so every wrapped/contextualized error is still correctly categorized before the retry decision.

## Role frontmatter field

```bash
grep "stage_retries" src/config/role.rs | head -10
```

```output
    stage_retries: Option<usize>,
                                "stage_retries" => {
                                    role.stage_retries = value.as_u64().map(|v| v as usize)
        if let Some(n) = self.stage_retries {
            meta.insert("stage_retries".into(), serde_json::json!(n));
    pub fn stage_retries(&self) -> Option<usize> {
        self.stage_retries
    fn test_stage_retries_default_none() {
        assert_eq!(role.stage_retries(), None);
    fn test_stage_retries_parsed_from_frontmatter() {
```

## Retryable classifier + tests

```bash
grep -E "^pub fn is_retryable_stage_error|^/// Phase 10C" src/utils/exit_code.rs
```

```output
/// Phase 10C: Decide whether a pipeline-stage failure is transient enough to
pub fn is_retryable_stage_error(err: &anyhow::Error) -> bool {
```

```bash
cargo test --bin aichat -- is_retryable stage_retries 2>&1 | grep -E "^test (config::role::tests::test_stage_retries|utils::exit_code::tests::test_is_retryable|utils::exit_code::tests::test_not_retryable)|^test result:" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/"
```

```output
test config::role::tests::test_stage_retries_coexists_with_schema_retries ... ok
test config::role::tests::test_stage_retries_default_none ... ok
test config::role::tests::test_stage_retries_in_export ... ok
test config::role::tests::test_stage_retries_parsed_from_frontmatter ... ok
test config::role::tests::test_stage_retries_zero_means_fail_fast ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
test utils::exit_code::tests::test_is_retryable_api_and_network ... ok
test utils::exit_code::tests::test_is_retryable_model_and_schema ... ok
```

(The first grep shows 5 role-field tests + 2 classifier tests. The corresponding `test_not_retryable_*` tests verify that config/auth/abort/usage/tool/general errors fail fast — see the `--- Phase 10C tests ---` block in `src/utils/exit_code.rs`.)

## Retry loop in pipe.rs

```bash
grep "Phase 10C\|is_retryable_stage_error\|max_stage_retries" src/pipe.rs
```

```output
    // Phase 10C/10D: peek at the role once for the retry budget and the model
    let max_stage_retries = role
                Err(e) if attempt < max_stage_retries && is_retryable_stage_error(&e) => {
                        max_stage_retries + 1,
                    if is_retryable_stage_error(&e)
```

Interaction with other phases:

- **10A** (HTTP retry) fires first inside each provider call — so most transient network blips are smoothed out *before* a stage failure is even classified. 10C covers the residual cases (429 that persists past 3 HTTP retries, output schema validation failure, model-level errors).
- **9C** (schema retry) runs inside `run_stage_inner`. A persistent schema failure that exhausts `schema_retries` becomes an exit-code-8 error at the stage boundary and is then eligible for another `stage_retries` attempt with a fresh input.
- Model-state restoration (Phase 0C) happens per-attempt so each retry starts from the same model state, not the state left by a prior failure.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```
