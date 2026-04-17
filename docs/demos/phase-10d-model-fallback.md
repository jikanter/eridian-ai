# Phase 10D: Pipeline Model Fallback

*2026-04-17T02:50:27Z by Showboat 0.6.1*
<!-- showboat-id: 33fb0522-467f-4786-8355-83745c2321b5 -->

Phase 10D wraps Phase 10C's retry loop in an outer **model fallback** iterator. Roles declare an ordered list via the new `fallback_models:` frontmatter field. On a retryable failure after the primary model exhausts its `stage_retries` budget, the stage is retried against the next model in the chain — with its own retry budget — and so on until one succeeds or the chain is exhausted. Non-retryable errors (config, auth, usage, tool, abort) still short-circuit immediately: hammering a broken role on a different model won't help. The resulting nesting is `fallback ( retry ( cache ( run_stage_inner ) ) )`.

## Role frontmatter field

```bash
grep -n "fallback_models" src/config/role.rs | head -10
```

```output
92:    fallback_models: Vec<String>,
487:                                "fallback_models" => {
489:                                        role.fallback_models = arr
585:        if !self.fallback_models.is_empty() {
587:                "fallback_models".into(),
588:                serde_json::json!(self.fallback_models),
763:    pub fn fallback_models(&self) -> &[String] {
764:        &self.fallback_models
2113:    fn test_fallback_models_default_empty() {
2116:        assert!(role.fallback_models().is_empty());
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
cargo test --bin aichat -- fallback_models 2>&1 | grep -E "^test config::role::tests::test_fallback_models|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test config::role::tests::test_fallback_models_default_empty ... ok
test config::role::tests::test_fallback_models_empty_list_is_not_exported ... ok
test config::role::tests::test_fallback_models_in_export ... ok
test config::role::tests::test_fallback_models_parsed_from_frontmatter ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 249 filtered out; finished in Xs
```

## Fallback iteration in pipe.rs

```bash
grep -n "Phase 10D\|fallback_models\|candidates\|model_override\|total_models" src/pipe.rs
```

```output
145:    let fallback_models: Vec<String> = role
147:        .map(|r| r.fallback_models().to_vec())
150:    // Phase 10D: build the candidate chain — primary first, then each fallback.
153:    let mut candidates: Vec<Option<String>> = vec![stage.model_id.clone()];
154:    for fb in &fallback_models {
155:        candidates.push(Some(fb.clone()));
157:    let total_models = candidates.len();
159:    for (model_index, model_override) in candidates.into_iter().enumerate() {
162:            model_id: model_override.clone(),
164:        let model_label = model_override
206:                        && model_index + 1 < total_models =>
220:                    let final_model_id = model_override
```

## Control flow — three arms in the retry match

1. **`attempt < max_stage_retries && is_retryable_stage_error`** — same-model retry (Phase 10C).
2. **`is_retryable_stage_error && model_index + 1 < total_models`** — retries exhausted on a transient error and another fallback exists → break the inner loop and advance to the next candidate.
3. **default** — non-retryable error, or the last candidate ran out of retries → wrap in `AichatError::PipelineStage` with the last-tried `model_id` and return.

The cache key (Phase 10B) incorporates `model_id`, so each fallback attempt has its own cache slot — replays are correct across model switches.

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 253 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
