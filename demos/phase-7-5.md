# Phase 7.5: Macro & Agent Config Override (.set Expansion)

*2026-03-16T18:44:59Z by Showboat 0.6.1*
<!-- showboat-id: c6787b49-b81e-4ce5-ae78-bf676984f365 -->

Phase 7.5 extends the `.set` REPL command to cover role-level fields that previously could only be set in role frontmatter: `model`, `output_schema`, `input_schema`, `pipe_to`, and `save_to`. This lets macros configure schemas and lifecycle hooks at runtime, and gives agents the same fields.

## 1. Build & Test — Baseline

```bash
cargo build 2>&1 | tail -3
```

```output

warning: `aichat` (bin "aichat") generated 3 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.88s
```

```bash
cargo test 2>&1 | grep 'test result'
```

```output
test result: ok. 132 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

## 2. Role Setters (role.rs)

Four new pub setter methods on Role allow runtime mutation of fields that were previously only settable via frontmatter parsing.

```bash
grep -n 'pub fn set_output_schema\|pub fn set_input_schema\|pub fn set_pipe_to\|pub fn set_save_to' src/config/role.rs
```

```output
677:    pub fn set_output_schema(&mut self, value: Option<Value>) {
681:    pub fn set_input_schema(&mut self, value: Option<Value>) {
685:    pub fn set_pipe_to(&mut self, value: Option<String>) {
689:    pub fn set_save_to(&mut self, value: Option<String>) {
```

Unit tests verify round-trip set/get for all four:

```bash
cargo test test_set_ 2>&1 | grep -E 'test config|test result'
```

```output
test config::role::tests::test_set_pipe_to ... ok
test config::role::tests::test_set_save_to ... ok
test config::role::tests::test_set_output_schema ... ok
test config::role::tests::test_set_input_schema ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 128 filtered out; finished in 0.02s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
```

## 3. AgentConfig Fields + Agent Methods (agent.rs)

AgentConfig gains four new optional fields. Agent delegates setters to `self.config.*` and `to_role()` propagates them after `role.sync()`.

```bash
grep -A1 'pub output_schema\|pub input_schema\|pub pipe_to\|pub save_to' src/config/agent.rs | head -8
```

```output
    pub output_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipe_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_to: Option<String>,
}
```

Agent::to_role() propagation — the four fields are forwarded to the derived role when set:

```bash
sed -n '/impl RoleLike for Agent/,/^    fn model/p' src/config/agent.rs | head -22
```

```output
impl RoleLike for Agent {
    fn to_role(&self) -> Role {
        let prompt = self.interpolated_instructions();
        let mut role = Role::new("", &prompt);
        role.sync(self);
        if self.config.output_schema.is_some() {
            role.set_output_schema(self.config.output_schema.clone());
        }
        if self.config.input_schema.is_some() {
            role.set_input_schema(self.config.input_schema.clone());
        }
        if self.config.pipe_to.is_some() {
            role.set_pipe_to(self.config.pipe_to.clone());
        }
        if self.config.save_to.is_some() {
            role.set_save_to(self.config.save_to.clone());
        }
        role
    }

    fn model(&self) -> &Model {
```

## 4. Session Transient Fields (session.rs)

Session gets four `#[serde(skip)]` fields so schema/hook overrides live only for the current session and aren't persisted to YAML.

```bash
grep -n 'serde(skip)' src/config/session.rs
```

```output
47:    #[serde(skip)]
49:    #[serde(skip)]
51:    #[serde(skip)]
53:    #[serde(skip)]
55:    #[serde(skip)]
57:    #[serde(skip)]
59:    #[serde(skip)]
61:    #[serde(skip)]
63:    #[serde(skip)]
65:    #[serde(skip)]
67:    #[serde(skip)]
69:    #[serde(skip)]
71:    #[serde(skip)]
```

```bash
grep -B1 'output_schema\|input_schema\|pipe_to_override\|save_to_override' src/config/session.rs | grep -E 'serde|output_schema|input_schema|pipe_to_override|save_to_override' | head -8
```

```output
    #[serde(skip)]
    output_schema: Option<serde_json::Value>,
    #[serde(skip)]
    input_schema: Option<serde_json::Value>,
    #[serde(skip)]
    pipe_to_override: Option<String>,
    #[serde(skip)]
    save_to_override: Option<String>,
```

Session::to_role() propagates overrides the same way Agent does:

```bash
sed -n '/impl RoleLike for Session/,/^    fn model/p' src/config/session.rs | head -22
```

```output
impl RoleLike for Session {
    fn to_role(&self) -> Role {
        let role_name = self.role_name.as_deref().unwrap_or_default();
        let mut role = Role::new(role_name, &self.role_prompt);
        role.sync(self);
        if self.output_schema.is_some() {
            role.set_output_schema(self.output_schema.clone());
        }
        if self.input_schema.is_some() {
            role.set_input_schema(self.input_schema.clone());
        }
        if self.pipe_to_override.is_some() {
            role.set_pipe_to(self.pipe_to_override.clone());
        }
        if self.save_to_override.is_some() {
            role.set_save_to(self.save_to_override.clone());
        }
        role
    }

    fn model(&self) -> &Model {
```

## 5. Fixed .set Value Parsing (mod.rs)

The old parser used `split_whitespace().collect()` requiring exactly 2 tokens — breaking on JSON values with spaces like `{"type": "object"}`. Now uses `split_once(whitespace)`: key is the first token, value is everything after.

```bash
sed -n '/pub fn update(config: &GlobalConfig/,/match key/p' src/config/mod.rs
```

```output
    pub fn update(config: &GlobalConfig, data: &str) -> Result<()> {
        let (key, value) = match data.split_once(|c: char| c.is_whitespace()) {
            Some((k, v)) => (k, v.trim()),
            None => bail!("Usage: .set <key> <value>. If value is null, unset key."),
        };
        if value.is_empty() {
            bail!("Usage: .set <key> <value>. If value is null, unset key.");
        }
        match key {
```

## 6. New .set Match Arms (mod.rs)

Five new keys are handled in `Config::update()`: `model`, `output_schema`, `input_schema`, `pipe_to`, `save_to`.

```bash
grep -n '"model" =>\|"output_schema" =>\|"input_schema" =>\|"pipe_to" =>\|"save_to" =>' src/config/mod.rs | head -5
```

```output
778:            "model" => {
781:            "output_schema" => {
788:            "input_schema" => {
795:            "pipe_to" => {
803:            "save_to" => {
```

## 7. Guard Rails

`parse_schema_value` handles `null` (unset), `@path` (read file as JSON), and inline JSON. `validate_json_schema` meta-validates via the existing `jsonschema` crate. `validate_pipe_to_command` checks binary existence via `which`.

```bash
sed -n '/^fn parse_schema_value/,/^}/p' src/config/mod.rs
```

```output
fn parse_schema_value(value: &str) -> Result<Option<serde_json::Value>> {
    if value == "null" {
        return Ok(None);
    }
    if let Some(path) = value.strip_prefix('@') {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read schema file '{path}'"))?;
        let schema: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("Invalid JSON in schema file '{path}'"))?;
        Ok(Some(schema))
    } else {
        let schema: serde_json::Value =
            serde_json::from_str(value).with_context(|| "Invalid JSON for schema")?;
        Ok(Some(schema))
    }
}
```

```bash
sed -n '/^fn validate_json_schema/,/^}/p' src/config/mod.rs && echo '---' && sed -n '/^fn validate_pipe_to_command/,/^}/p' src/config/mod.rs
```

```output
fn validate_json_schema(schema: &serde_json::Value) -> Result<()> {
    jsonschema::validator_for(schema)
        .map_err(|e| anyhow!("Invalid JSON schema: {e}"))?;
    Ok(())
}
---
fn validate_pipe_to_command(cmd: &str) -> Result<()> {
    let binary = cmd.split_whitespace().next().unwrap_or(cmd);
    which::which(binary)
        .map_err(|_| anyhow!("Command not found: '{binary}'"))?;
    Ok(())
}
```

## 8. Tab Completion

The `.set` key list now includes all five new keys, with value completions for each:

```bash
grep -A25 '".set" =>' src/config/mod.rs | grep '"model"\|"output_schema"\|"input_schema"\|"pipe_to"\|"save_to"'
```

```output
                        "model",
                        "output_schema",
                        "input_schema",
                        "pipe_to",
                        "save_to",
```

```bash
grep -A2 '"model" =>.*list_models\|"output_schema" |\|"input_schema"\|"pipe_to" |\|"save_to" =>' src/config/mod.rs | grep -E 'model.*list_models|output_schema|input_schema|pipe_to|save_to|null' | tail -4
```

```output
                        "save_to",
                "model" => list_models(self, ModelType::Chat)
                "output_schema" | "input_schema" => vec!["null".to_string()],
                "pipe_to" | "save_to" => vec!["null".to_string()],
```

## 9. Sysinfo Display

When set, the new fields appear in the `.info` output between `save_session` and `compress_threshold`:

```bash
sed -n '/if role.output_schema/,/items.extend/p' src/config/mod.rs
```

```output
        if role.output_schema().is_some() {
            items.push(("output_schema", role.output_schema().unwrap().to_string()));
        }
        if role.input_schema().is_some() {
            items.push(("input_schema", role.input_schema().unwrap().to_string()));
        }
        if role.pipe_to().is_some() {
            items.push(("pipe_to", role.pipe_to().unwrap().to_string()));
        }
        if role.save_to().is_some() {
            items.push(("save_to", role.save_to().unwrap().to_string()));
        }
        items.extend([
```

## 10. ROADMAP Updated

All four items marked Done:

```bash
grep '7\.5[A-D]' docs/ROADMAP.md
```

```output
| 7.5A. Extend `.set` with role-level fields | Done | Add `model`, `output_schema`, `input_schema`, `pipe_to`, `save_to` to `.set` dispatch in `Config::set()`. Schema fields accept inline JSON or `@file` path. |
| 7.5B. Macro frontmatter assembly | Done | Macros can now use `.set` to dress up a `.prompt` or override fields on a `.role` before prompting. This turns macros into role factories without collapsing the declarative/imperative boundary. |
| 7.5C. Agent `.set` parity | Done | Agents gain the same `.set` overrides through the `RoleLike` trait. `AgentConfig` adds optional `output_schema`, `input_schema`, `pipe_to`, `save_to`. `Agent::to_role()` propagates them via `role.sync()`. |
| 7.5D. Guard rails | Done | `.set output_schema` and `.set input_schema` validate the schema itself (meta-validation via `jsonschema::is_valid`) before accepting. `.set pipe_to` validates the target exists. Errors use Phase 4 structured format. |
    // Phase 7.5C additions:
**Agent** — Directory-based (`<functions_dir>/agents/name/`). Implements `RoleLike` trait — wraps a Role via `to_role()`. Adds: own tool functions (`functions.json`), RAG (documents), dynamic instructions (`_instructions` shell function), interactive variable prompting, session management, env-var bridging (`LLM_AGENT_VAR_*`). Defined in llm-functions, not in aichat's config directory. Does NOT support: input_schema, output_schema, pipe_to, save_to, mcp_servers, extends/include, pipeline. Phase 7.5C proposes adding `output_schema`, `input_schema`, `pipe_to`, `save_to` to `AgentConfig` (per-invocation overrides, not baked into definition). Invoked via `aichat -a name`.
| `input_schema` validation | No | **Yes** (`jsonschema` before LLM) | No (Phase 7.5C) | No |
| `output_schema` validation | No | **Yes** (`jsonschema` after LLM) | No (Phase 7.5C) | No |
| `pipe_to` / `save_to` | No | **Yes** | No (Phase 7.5C) | No |
```

## 11. Full Test Suite — Final

All 305 tests pass (132 unit + 173 integration), including the 4 new setter tests:

```bash
cargo test 2>&1 | grep -E 'test (config::role::tests::test_set|result)'
```

```output
test config::role::tests::test_set_input_schema ... ok
test config::role::tests::test_set_output_schema ... ok
test config::role::tests::test_set_save_to ... ok
test config::role::tests::test_set_pipe_to ... ok
test result: ok. 132 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```
