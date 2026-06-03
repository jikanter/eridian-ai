# Pi harness pinned to aichat models

*2026-06-03T21:59:23Z by Showboat 0.6.1*
<!-- showboat-id: 72abcb8f-6177-4f31-9c27-140ab3c41914 -->

When aichat launches the pi REPL, it pins pi to **aichat's models** so every turn flows through aichat (inference, caching, roles, cost accounting) instead of pi calling Google/Anthropic/Ollama directly. Mechanism: stage a pi agent dir whose `models.json` registers a single `aichat` provider, and point pi at it via `PI_CODING_AGENT_DIR`. This demo proves pi then sees only aichat's models.

**Baseline:** without pinning, pi lists its own configured providers. Here is the provider set pi sees from the user's native config (provider column, deduped):

```bash
PI_OFFLINE=1 pi --list-models 2>&1 | awk "NR>1{print \$1}" | sort -u
```

```output
anthropic
huggingface
ollama
```

**Pinned:** the launcher stages an agent dir whose `models.json` exposes only the in-process aichat server. `PI_CODING_AGENT_DIR` points pi at it. Pi now lists ONLY the `aichat` provider — its Google/Anthropic/Ollama config is ignored:

```bash
set -e
STAGE="$(pwd)/.demo-pi-stage"
rm -rf "$STAGE"; mkdir -p "$STAGE"
cat > "$STAGE/models.json" <<JSON
{
  "providers": {
    "aichat": {
      "baseUrl": "http://127.0.0.1:8765/v1",
      "api": "openai-completions",
      "apiKey": "aichat",
      "models": [
        { "id": "default", "contextWindow": 200000, "maxTokens": 16384 },
        { "id": "openai:gpt-4o", "contextWindow": 128000, "maxTokens": 16384 },
        { "id": "ollama:llama3", "contextWindow": 8192 }
      ]
    }
  }
}
JSON
echo "{\"defaultProvider\":\"aichat\",\"defaultModel\":\"default\"}" > "$STAGE/settings.json"
PI_CODING_AGENT_DIR="$STAGE" PI_OFFLINE=1 pi --list-models 2>&1
echo "--- providers (deduped) ---"
PI_CODING_AGENT_DIR="$STAGE" PI_OFFLINE=1 pi --list-models 2>&1 | awk "NR>1{print \$1}" | sort -u
rm -rf "$STAGE"
```

```output
provider  model          context  max-out  thinking  images
aichat    default        200K     16.4K    no        no    
aichat    ollama:llama3  8.2K     16.4K    no        no    
aichat    openai:gpt-4o  128K     16.4K    no        no    
--- providers (deduped) ---
aichat
```

The Rust launcher builds that `models.json`/`settings.json` from aichat's live config (`list_all_models`, default model) and symlinks the real agent dir's `sessions/`, `auth.json`, themes, and prompts into the stage so session history survives. Unit tests cover the JSON shape, default-model selection, settings merge, and the staging/symlink/cleanup round-trip:

```bash
cargo test --bin aichat repl::pi:: 2>&1 | grep -E "test result|models_json|default_model|settings_|staged_agent_dir"
```

```output
test repl::pi::tests::default_model_falls_back_to_first_chat_when_configured_absent ... ok
test repl::pi::tests::default_model_prefers_configured_when_present ... ok
test repl::pi::tests::default_model_none_when_no_chat_models ... ok
test repl::pi::tests::settings_minimal_when_no_existing_file ... ok
test repl::pi::tests::settings_override_defaults_preserving_other_keys ... ok
test repl::pi::tests::settings_ignores_malformed_existing ... ok
test repl::pi::tests::models_json_registers_only_aichat_provider_with_chat_models ... ok
test repl::pi::tests::staged_agent_dir_handles_missing_real_dir ... ok
test repl::pi::tests::staged_agent_dir_writes_our_files_and_symlinks_the_rest ... ok
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 669 filtered out; finished in 0.49s
```

Opt out with `AICHAT_PI_NATIVE_MODELS=1` to keep pi's own provider config; if staging fails the launcher logs a warning and falls back to native config rather than aborting the launch. Pinning is transparent — `/model` (Ctrl+P) in pi shows only aichat models, and `role:<name>` virtual models still route through aichat roles.
