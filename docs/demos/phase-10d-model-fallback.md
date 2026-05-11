# Phase 10D: Pipeline Model Fallback

*2026-04-17T02:50:27Z by Showboat 0.6.1*
<!-- showboat-id: 33fb0522-467f-4786-8355-83745c2321b5 -->

Phase 10D wraps Phase 10C's retry loop in an outer **model fallback** iterator. Roles declare an ordered list via the new `fallback_models:` frontmatter field. On a retryable failure after the primary model exhausts its `stage_retries` budget, the stage is retried against the next model in the chain — with its own retry budget — and so on until one succeeds or the chain is exhausted. Non-retryable errors (config, auth, usage, tool, abort) still short-circuit immediately: hammering a broken role on a different model won't help. The resulting nesting is `fallback ( retry ( cache ( run_stage_inner ) ) )`.

## Role frontmatter field

```bash
grep "fallback_models" src/config/role.rs | head -10
```

```output
    fallback_models: Vec<String>,
                                "fallback_models" => {
                                        role.fallback_models = arr
        if !self.fallback_models.is_empty() {
                "fallback_models".into(),
                serde_json::json!(self.fallback_models),
    pub fn fallback_models(&self) -> &[String] {
        &self.fallback_models
    fn test_fallback_models_default_empty() {
        assert!(role.fallback_models().is_empty());
```

Example role declaration (4-space indented to keep showboat from executing a YAML block):

    ---
    model: deepseek:deepseek-chat
    stage_retries: 2
    fallback_models:
      - openai:gpt-4o-mini
      - openai:gpt-4o
    ---
    Prompt body.

Empty `fallback_models` lists are dropped on export — round-trip stays clean.

## Role-level tests

```bash
cargo test --bin aichat -- fallback_models 2>&1 | grep -E "^test config::role::tests::test_fallback_models|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/"
```

```output
test config::role::tests::test_fallback_models_default_empty ... ok
test config::role::tests::test_fallback_models_empty_list_is_not_exported ... ok
test config::role::tests::test_fallback_models_in_export ... ok
test config::role::tests::test_fallback_models_parsed_from_frontmatter ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## Fallback iteration in pipe.rs

```bash
grep "Phase 10D\|fallback_models\|candidates\|model_override\|total_models" src/pipe.rs
```

```output
    let fallback_models: Vec<String> = role
        .map(|r| r.fallback_models().to_vec())
    // Phase 10D: build the candidate chain — primary first, then each fallback.
    let mut candidates: Vec<Option<String>> = vec![stage.model_id.clone()];
    for fb in &fallback_models {
        candidates.push(Some(fb.clone()));
    let total_models = candidates.len();
    for (model_index, model_override) in candidates.into_iter().enumerate() {
            model_id: model_override.clone(),
        let model_label = model_override
                        && model_index + 1 < total_models =>
                    let final_model_id = model_override
```

## Control flow — three arms in the retry match

1. **`attempt < max_stage_retries && is_retryable_stage_error`** — same-model retry (Phase 10C).
2. **`is_retryable_stage_error && model_index + 1 < total_models`** — retries exhausted on a transient error and another fallback exists → break the inner loop and advance to the next candidate.
3. **default** — non-retryable error, or the last candidate ran out of retries → wrap in `AichatError::PipelineStage` with the last-tried `model_id` and return.

The cache key (Phase 10B) incorporates `model_id`, so each fallback attempt has its own cache slot — replays are correct across model switches.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```
