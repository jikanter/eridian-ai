# Analysis: Supporting Model-Specific Extensions in AI Clients

**Date:** 2026-04-23
**Status:** Proposed / Research Complete

## 1. Background and Motivation

As local LLM deployments (Ollama, vLLM, llama.cpp) gain popularity, they are introducing vendor-specific features and parameters that fall outside the standard OpenAI Chat Completions API. Currently, `aichat` uses an `openai-compatible` client that adheres strictly to the standard schema, preventing users from leveraging these advanced features (e.g., custom context windows, guided decoding, sampling penalties).

The goal is to provide a flexible "extension" mechanism that allows these extra parameters to be passed to any OpenAI-compatible backend without requiring hardcoded support for every new provider.

---

## 2. Research Findings

### A. Ollama Specifics
Ollama supports a wide range of inference parameters passed in an `options` field in its native API, or as top-level fields in its OpenAI-compatible endpoint.
- **Parameters:** `num_ctx` (context window), `repeat_penalty`, `num_predict` (max tokens), `top_k`, `stop` (stop sequences), `mirostat`.
- **Slash Commands:** Ollama's CLI supports interactive commands like `/set parameter <key> <value>`, `/set system <message>`, and `/show info`.

### B. vLLM Specifics
vLLM supports "Guided Decoding" and other sampling parameters often passed via an `extra_body` field or as top-level additions.
- **Features:** `guided_json`, `guided_regex`, `guided_choice`, `repetition_penalty`, `use_beam_search`.
- **Convention:** Many tools (like LangChain) use an `extra_body` key to handle these.

### C. Architectural Requirements
- **Granularity:** Extensions should be definable at the **Client** level (global defaults for a provider) and the **Model** level (task-specific overrides).
- **Merging Strategy:** A clear precedence order is needed: Base Body < Client Extensions < Model Extensions < Request Patch.

---

## 3. Proposed Specification

### A. Configuration Syntax
Introduce an `extensions` key (or `extra_body`) at both the client and model levels in `config.yaml`.

```yaml
clients:
  - type: openai-compatible
    name: ollama
    api_base: http://localhost:11434/v1
    # Client-level extensions (global defaults for this provider)
    extensions:
      num_ctx: 4096
      repeat_penalty: 1.1
    models:
      - name: llama3.1:70b
        # Model-level extensions (overrides client-level)
        extensions:
          num_ctx: 32768
          top_k: 50
```

### B. Data Structure Changes
Update the following structs:
- **`ModelData` (src/client/model.rs):** Add `pub extensions: Option<serde_json::Value>`.
- **`OpenAICompatibleConfig` (src/client/openai_compatible.rs):** Add `pub extensions: Option<serde_json::Value>`.

### C. Implementation Logic (The "Hook")
Update the body builder in `src/client/openai.rs`:

```rust
// Logic for merging extensions into the request body
pub fn openai_build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Value {
    let mut body = base_openai_body(data, model); // Existing logic

    // 1. Merge Client-level extensions
    if let Some(client_ext) = model.client().extensions() {
        json_patch::merge(&mut body, client_ext);
    }

    // 2. Merge Model-level extensions
    if let Some(model_ext) = model.extensions() {
        json_patch::merge(&mut body, model_ext);
    }

    body
}
```

### D. REPL Command Integration
To support Ollama-like interactivity, implement a generic ".extensions" or ".patch" command in the REPL.
- `.extensions set <key> <value>`: Temporarily updates the current model's extensions for the session.
- Aliases: ".ollama" can be an alias for ".extensions" when using an Ollama-named client.

---

## 4. Verification and Testing Plan

### Unit Tests
- **Serialization:** Verify `extensions` are correctly parsed from YAML.
- **Merging Logic:** Test that Model-level extensions correctly overwrite Client-level extensions and that neither breaks standard fields like `messages`.

### Integration Tests (via `bats`)
Create `tests/integration/extensions.sh`:
- **Scenario 1:** Run `aichat` with a mock config containing extensions and use `--dry-run` to verify the generated JSON body contains the expected vendor-specific flags (e.g., `num_ctx`).
- **Scenario 2:** Verify that setting an extension via REPL (if implemented) updates subsequent requests.

---

## 5. Execution Plan for Implementation

1. **Phase 1: Config Update**
   - Modify `src/client/model.rs` and `src/client/openai_compatible.rs` to include the `extensions` field.
   - Update config loading logic to populate these fields.

2. **Phase 2: Request Body Injection**
   - Modify `src/client/openai.rs` (or relevant builder) to perform the JSON merge.
   - Ensure `json_patch` or similar crate is available for deep merging.

3. **Phase 3: REPL Support (Optional but recommended)**
   - Add the ".extensions" command to `src/repl/mod.rs`.

4. **Phase 4: Documentation**
   - Update `config.example.yaml` with examples for Ollama and vLLM.
