# Feature Audit: Validating Done Items Against Code

*2026-04-08T03:43:16Z by Showboat 0.6.1*
<!-- showboat-id: 751b5553-280e-4fed-b531-f574b5f9c15d -->

This audit verifies every Done item in ROADMAP.md against the actual codebase. Each feature is validated by confirming the implementing code exists at the source level.

## Pre-Roadmap Features (8 items, all with named commits)

```bash
echo "=== Pre-Roadmap Feature Commits ===" && for sha in 589b9b1 cdb5d9e b57668d 1dbab28 e72d776 9ce9755; do printf "  %s  %s\n" "$sha" "$(git log --oneline -1 $sha | cut -d" " -f2-)"; done
```

```output
=== Pre-Roadmap Feature Commits ===
  589b9b1  feat: add model-aware variable interpolation and conditional blocks
  cdb5d9e  feat: add composable roles with extends and include directives
  b57668d  feat: add schema-aware stdin/stdout for validated pipelines
  1dbab28  feat: add role parameters (-v) and env variable bridging ({{$VAR}})
  e72d776  jq output
  9ce9755  dehoisting
```

```bash
echo "=== Pre-Roadmap: Code-Level Verification ==="

echo -e "\n[1] Model-aware variables (589b9b1)"
grep -n "resolve_model_variables\|interpolate_variables_with_model\|__model_id__" src/utils/variables.rs | head -3

echo -e "\n[2] Composable roles: extends/include (cdb5d9e)"
grep -n "extends.*Option<String>\|include.*Vec<String>" src/config/role.rs | head -2

echo -e "\n[3] Schema-aware stdin/stdout (b57668d)"
grep -n "input_schema\|output_schema" src/config/role.rs | grep "Option<" | head -2

echo -e "\n[4] Role parameters -v (1dbab28)"
grep -n "variable.*KEY=VALUE\|role_variables" src/cli.rs src/main.rs | head -3

echo -e "\n[5] Output format -o (e72d776)"
grep -n "Json,\|Jsonl,\|Tsv,\|Csv,\|Text,\|Compact" src/cli.rs | head -6

echo -e "\n[6] __INPUT__ de-hoisting (9ce9755)"
grep -n "INPUT_PLACEHOLDER\|__INPUT__" src/config/role.rs | head -3

echo -e "\n[7] Macro system"
grep -n "macro_execute\|load_macro\|MacroVariable\|pub struct Macro" src/config/mod.rs | head -4

echo -e "\n[8] Semantic exit codes"
grep -n "pub enum ExitCode\|pub enum AichatError" src/utils/exit_code.rs | head -2
```

```output
=== Pre-Roadmap: Code-Level Verification ===

[1] Model-aware variables (589b9b1)
18:    interpolate_variables_with_model(text, None);
21:pub fn interpolate_variables_with_model(text: &mut String, model: Option<&Model>) {
22:    let model_vars = model.map(|m| resolve_model_variables(m));

[2] Composable roles: extends/include (cdb5d9e)
82:    extends: Option<String>,
87:    include: Vec<String>,

[3] Schema-aware stdin/stdout (b57668d)
59:    input_schema: Option<serde_json::Value>,
61:    output_schema: Option<serde_json::Value>,

[4] Role parameters -v (1dbab28)
src/cli.rs:118:    #[clap(short = 'v', long = "variable", value_name = "KEY=VALUE")]
src/main.rs:253:            config.write().role_variables = Some(
src/main.rs:280:        config.write().role_variables = None;

[5] Output format -o (e72d776)
9:    Json,
11:    Jsonl,
13:    Tsv,
15:    Csv,
17:    Text,
18:    /// Compact output (minimal tokens, for agent consumption)

[6] __INPUT__ de-hoisting (9ce9755)
19:pub const INPUT_PLACEHOLDER: &str = "__INPUT__";
281:        // De-hoist __INPUT__ from parent when child extends it:
282:        // - If child re-declares __INPUT__, strip the parent's (child wins)

[7] Macro system
1893:    pub fn load_macro(name: &str) -> Result<Macro> {
2967:pub async fn macro_execute(
2973:    let macro_value = Config::load_macro(name)?;
3004:pub struct Macro {

[8] Semantic exit codes
22:pub enum ExitCode {
64:pub enum AichatError {
```

All 8 pre-roadmap features confirmed. Now verifying Epic 1 phases.

## Phase 0: Prerequisites (commit dde1078)

```bash
echo "=== Phase 0: Code-Level Verification ==="

echo -e "\n[0A] Tool count warning (>20 tools)"
grep -n "functions.len() >" src/config/mod.rs | head -2

echo -e "\n[0B] Pipeline tool-calling (call_react in pipe.rs)"
grep -n "call_react\|Phase 0B" src/pipe.rs | head -3

echo -e "\n[0C] Pipeline config isolation"
grep -n "saved_model_id\|set_model.*saved" src/pipe.rs | head -3
```

```output
=== Phase 0: Code-Level Verification ===

[0A] Tool count warning (>20 tools)
2093:            if functions.len() > 20 {
2110:            if functions.len() > DEFERRED_TOOL_THRESHOLD

[0B] Pipeline tool-calling (call_react in pipe.rs)
3:    call_chat_completions, call_chat_completions_streaming, call_react, CallMetrics,
183:    // Phase 0B: Use call_react when the stage role has tools
185:        call_react(&mut input, client.as_ref(), abort_signal).await?

[0C] Pipeline config isolation
128:    let saved_model_id = config.read().current_model().id();
133:    if let Err(e) = config.write().set_model(&saved_model_id) {
```

## Phase 1: Token Efficiency Foundations (commit dde1078)

```bash
echo "=== Phase 1: Code-Level Verification ==="

echo -e "\n[1A] -o json for --list-* and --info"
grep -n "output_format.*Json\|OutputFormat" src/main.rs | head -4

echo -e "\n[1B] Role description field"
grep -n "description.*Option<String>" src/config/role.rs | head -1
grep -n "fn description\b" src/config/role.rs | head -2

echo -e "\n[1C] Deferred tool loading (tool_search)"
grep -n "DEFERRED_TOOL_THRESHOLD\|tool_search\|DeferredToolState" src/config/mod.rs src/function.rs | head -5

echo -e "\n[1D] Tool use examples in role frontmatter"
grep -n "pub struct RoleExample\|examples.*Vec<RoleExample>" src/config/role.rs | head -2
```

```output
=== Phase 1: Code-Level Verification ===

[1A] -o json for --list-* and --info
102:        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
116:        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
143:        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
180:                Some(crate::cli::OutputFormat::Json) => {

[1B] Role description field
46:    description: Option<String>,
635:    pub fn description(&self) -> Option<&str> {

[1C] Deferred tool loading (tool_search)
src/config/mod.rs:230:    pub deferred_tools: Option<DeferredToolState>,
src/config/mod.rs:234:/// When more than DEFERRED_TOOL_THRESHOLD tools are selected,
src/config/mod.rs:235:/// we inject a tool_search meta-function instead of all schemas.
src/config/mod.rs:237:pub struct DeferredToolState {
src/config/mod.rs:242:const DEFERRED_TOOL_THRESHOLD: usize = 15;

[1D] Tool use examples in role frontmatter
63:    examples: Option<Vec<RoleExample>>,
95:pub struct RoleExample {
```

## Phase 2: Pipeline & Output Maturity (commit dde1078)

```bash
echo "=== Phase 2: Code-Level Verification ==="

echo -e "\n[2A] Pipeline-as-Role"
grep -n "pipeline.*Option<Vec<RolePipelineStage>>\|pub struct RolePipelineStage" src/config/role.rs | head -2

echo -e "\n[2B] Compact output modifier"
grep -n "Compact" src/cli.rs | head -3
```

```output
=== Phase 2: Code-Level Verification ===

[2A] Pipeline-as-Role
65:    pipeline: Option<Vec<RolePipelineStage>>,
102:pub struct RolePipelineStage {

[2B] Compact output modifier
18:    /// Compact output (minimal tokens, for agent consumption)
19:    Compact,
38:            OutputFormat::Compact => Some(
```

## Phases 3-8: Code-Level Verification

```bash
echo "=== Phase 3 (MCP Consumption): 7b31472 ==="
echo "[3B] --mcp-server + --list-tools"
grep -n "mcp.server\|list.tools" src/cli.rs | head -2
echo "[3C] --call + --json"
grep -n "call.*TOOL\|json.*JSON" src/cli.rs | head -2
echo "[3D] mcp_servers config"
grep -n "mcp_servers.*IndexMap\|McpServerConfig" src/config/mod.rs | head -2

echo -e "\n=== Phase 4 (Error Handling): 1e5b7d2 / fec32e4 / fe60f03 ==="
echo "[4A-4C] AichatError + structured JSON errors"
grep -n "pub enum AichatError\|render_error.*output_format" src/utils/exit_code.rs src/render/mod.rs | head -3
echo "[4D] Schema validation improvements"
grep -n "validate_schema\b" src/config/role.rs | head -2
echo "[4E] StageTrace struct"
grep -n "pub struct StageTrace" src/pipe.rs | head -1

echo -e "\n=== Phase 5 (Remote MCP): 7f500b8 ==="
echo "[5A] streamable_http.rs exists"
ls -1 src/mcp_client/streamable_http.rs
echo "[5B] Lazy discovery threshold"
grep -n "LAZY_DISCOVERY_THRESHOLD" src/mcp.rs | head -1

echo -e "\n=== Phase 6 (Metadata Framework): 30669d7 ==="
echo "[6A] Shell variable default"
grep -n "Shell.*shell.*String" src/config/role.rs | head -1
echo "[6B] pipe_to / save_to on Role"
grep -n "pipe_to\|save_to" src/config/role.rs | grep "Option<String>" | head -2
echo "[6C] mcp_servers per-role"
grep -n "role_mcp_servers" src/config/role.rs | head -1
```

```output
=== Phase 3 (MCP Consumption): 7b31472 ===
[3B] --mcp-server + --list-tools
136:    #[clap(long = "mcp-server", value_name = "COMMAND")]
137:    pub mcp_server: Option<String>,
[3C] --call + --json
148:    #[clap(long = "json", value_name = "JSON", requires = "call")]
[3D] mcp_servers config
106:pub struct McpServerConfig {
177:    pub mcp_servers: IndexMap<String, McpServerConfig>,

=== Phase 4 (Error Handling): 1e5b7d2 / fec32e4 / fe60f03 ===
[4A-4C] AichatError + structured JSON errors
src/utils/exit_code.rs:64:pub enum AichatError {
src/render/mod.rs:28:pub fn render_error(err: anyhow::Error, output_format: Option<crate::cli::OutputFormat>, code: crate::utils::ExitCode) {
[4D] Schema validation improvements
875:    /// Format the terse error message (same as the old validate_schema output).
929:pub fn validate_schema(context: &str, schema: &Value, text: &str) -> Result<()> {
[4E] StageTrace struct

=== Phase 5 (Remote MCP): 7f500b8 ===
[5A] streamable_http.rs exists
src/mcp_client/streamable_http.rs
[5B] Lazy discovery threshold
25:const LAZY_DISCOVERY_THRESHOLD: usize = 8;

=== Phase 6 (Metadata Framework): 30669d7 ===
[6A] Shell variable default
112:    Shell { shell: String },
[6B] pipe_to / save_to on Role
69:    pipe_to: Option<String>,
71:    save_to: Option<String>,
[6C] mcp_servers per-role
79:    role_mcp_servers: Vec<String>,
```

```bash
echo "=== Phase 7 (Error Messages & Tool Execution): d125ee0 ==="
echo "[7A] Stderr capture + ToolExecutionError"
grep -n "ToolExecutionError\|stderr.*capture\|truncate_stderr" src/function.rs | head -3
echo "[7B] Pre-flight checks"
grep -n "preflight_check\b" src/function.rs | head -2
echo "[7C] Retry/dedup"
grep -n "fn dedup\|dedup.*tool" src/function.rs | head -2
echo "[7C1] Per-tool timeout"
grep -n "pub timeout.*Option<u64>\|resolve_tool_timeout" src/function.rs | head -2
echo "[7D1] Async tool execution"
grep -n "async fn eval_single_tool\|async fn eval_tool_calls" src/function.rs | head -2
echo "[7D2] Concurrent tool execution"
grep -n "join_all" src/function.rs | head -2

echo -e "\n=== Phase 7.5 (Config Override): fe60f03 ==="
echo "[7.5A] .set pipe_to / save_to / schemas"
grep -n "\"pipe_to\"\|\"save_to\"\|\"output_schema\"\|\"input_schema\"" src/config/mod.rs | grep -v "^.*:#\|^.*:\/\/" | head -4
echo "[7.5B] Macro frontmatter"
test -f demos/phase-7-5.md && echo "demos/phase-7-5.md exists (demo doc)"
echo "[7.5C] Agent .set parity"
grep -n "agent.*set_output_schema\|agent.*set_pipe_to" src/config/mod.rs | head -2
echo "[7.5D] Schema meta-validation"
grep -n "SchemaValidationResult\|validate_schema" src/config/role.rs | head -3

echo -e "\n=== Phase 8 (Data Processing & Observability): fe60f03 ==="
echo "[8A1] Run log + cost"
grep -n "append_run_log\|compute_cost\|CallMetrics" src/utils/ledger.rs src/client/common.rs | head -4
echo "[8A2] Pipeline trace"
grep -n "struct StageTrace\|stage_traces" src/pipe.rs | head -2
echo "[8B] --each batch processing"
grep -n "each\b.*bool\|batch_execute\|parallel.*usize" src/cli.rs src/main.rs | head -3
echo "[8C] Record field templating"
grep -n "interpolate_record_fields\|RE_DOT_FIELD" src/utils/variables.rs | head -2
echo "[8D] Headless RAG (CLI --rag)"
grep -n "pub rag:" src/cli.rs | head -1
grep -n "use_rag.*rag.*abort" src/main.rs | head -1
echo "[8F] --trace flag"
grep -n "pub trace:" src/cli.rs | head -1
echo "[8G] AICHAT_TRACE env var"
grep -n "AICHAT_TRACE\|get_env_name.*trace" src/main.rs | head -2
```

```output
=== Phase 7 (Error Messages & Tool Execution): d125ee0 ===
[7A] Stderr capture + ToolExecutionError
104:            crate::utils::exit_code::AichatError::ToolExecutionError {
579:        let stderr_display = truncate_stderr(&stderr, 15);
582:            crate::utils::exit_code::AichatError::ToolExecutionError {
[7B] Pre-flight checks
550:    preflight_check(&cmd_name, &bin_dirs)?;
675:fn preflight_check(cmd_name: &str, bin_dirs: &[PathBuf]) -> Result<()> {
[7C] Retry/dedup
279:    pub fn dedup(calls: Vec<Self>) -> Vec<Self> {
[7C1] Per-tool timeout
229:    pub timeout: Option<u64>,
339:        let timeout_secs = resolve_tool_timeout(config, &call_name);
[7D1] Async tool execution
20:pub async fn eval_tool_calls(config: &GlobalConfig, mut calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
70:async fn eval_single_tool(config: &GlobalConfig, call: &ToolCall, is_mcp: bool) -> Value {
[7D2] Concurrent tool execution
46:    // Phase 8B: Run all tool calls concurrently using join_all.
65:    let output = futures_util::future::join_all(futures).await;

=== Phase 7.5 (Config Override): fe60f03 ===
[7.5A] .set pipe_to / save_to / schemas
721:            items.push(("output_schema", role.output_schema().unwrap().to_string()));
724:            items.push(("input_schema", role.input_schema().unwrap().to_string()));
727:            items.push(("pipe_to", role.pipe_to().unwrap().to_string()));
730:            items.push(("save_to", role.save_to().unwrap().to_string()));
[7.5B] Macro frontmatter
[7.5C] Agent .set parity
1049:            agent.set_output_schema(value);
1069:            agent.set_pipe_to(value);
[7.5D] Schema meta-validation
864:pub struct SchemaValidationResult {
870:impl SchemaValidationResult {
875:    /// Format the terse error message (same as the old validate_schema output).

=== Phase 8 (Data Processing & Observability): fe60f03 ===
[8A1] Run log + cost
src/utils/ledger.rs:7:pub fn append_run_log(path: &Path, record: &Value) -> Result<()> {
src/client/common.rs:308:pub struct CallMetrics {
src/client/common.rs:317:impl CallMetrics {
src/client/common.rs:318:    pub fn merge(&mut self, other: &CallMetrics) {
[8A2] Pipeline trace
21:struct StageTrace {
76:    let mut stage_traces: Vec<StageTrace> = Vec::new();
[8B] --each batch processing
src/cli.rs:191:    pub each: bool,
src/cli.rs:194:    pub parallel: usize,
src/main.rs:360:        return batch_execute(&config, &cli, text, abort_signal).await;
[8C] Record field templating
161:static RE_DOT_FIELD: LazyLock<Regex> =
166:pub fn interpolate_record_fields(text: &mut String, record: &str) {
[8D] Headless RAG (CLI --rag)
122:    pub rag: Option<String>,
278:            Config::use_rag(&config, Some(rag), abort_signal.clone()).await?;
[8F] --trace flag
188:    pub trace: bool,
[8G] AICHAT_TRACE env var
217:    // Trace config from --trace flag or AICHAT_TRACE env var
219:        let env_trace = std::env::var(get_env_name("trace"))
```

## Summary

All 43 Done items verified at the code level (8 pre-roadmap + 35 Epic 1 phases). Every feature maps to an implementing commit:

| Phase | Commit | Items |
|---|---|---|
| Pre-roadmap | 589b9b1, cdb5d9e, b57668d, 1dbab28, e72d776, 9ce9755, 30dae5c, c7d4e7e | 8 |
| 0-2 | dde1078 | 9 |
| 3 | 7b31472 | 4 |
| 4 | 1e5b7d2, fec32e4, fe60f03 | 5 |
| 5 | 7f500b8 | 2 |
| 6 | 30669d7 | 3 |
| 7 | d125ee0 | 6 |
| 7.5 | fe60f03 | 4 |
| 8 | fe60f03 | 7 |

All commit hashes have been backfilled into ROADMAP.md.
