# Phase 22: DAG Observability & Budget : Overview - Epic 7

| Item | Description | Status |
|---|---|---|
| 22A | DAG trace visualization (tree structure in `--trace` output) | -- |
| 22B | Per-branch cost tracking in parallel execution | -- |
| 22C | Budget-aware fan-out (split pipeline budget across parallel branches) | -- |
| 22D | DAG stage caching (cache branches independently, skip unchanged) | -- |
| 22E | Fix flaky `mcp_client::tests::test_load_mcp_servers_file_rejects_neither_command_nor_url` — test pollution: passes alone, fails in `cargo test --bin aichat`. Currently `--skip`'d in `docs/demos/demo-test-suite.md` and `docs/demos/phase-9a-openai-response-format.md`. Remove the skip once fixed. | -- |

**22A Design — DAG Trace:**

```
[pipeline] secure-review (5 stages, 2 parallel)
  [1] extract              deepseek:deepseek-chat   500→200tok  $0.0001  0.8s
  [2] parallel (3 branches)
    [2a] security-review   claude:claude-sonnet-4-6  200→300tok  $0.004   1.2s
    [2b] style-review      deepseek:deepseek-chat    200→150tok  $0.0001  0.6s
    [2c] perf-review       deepseek:deepseek-chat    200→180tok  $0.0001  0.7s
    merge: concatenate     --                        --          --       0ms
  [3] synthesize           claude:claude-sonnet-4-6  630→200tok  $0.006   1.5s
  total: $0.0103  4.3s (wall) vs 6.1s (sequential)
```
