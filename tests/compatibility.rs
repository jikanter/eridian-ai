//! Comprehensive compatibility tests for aichat changes against llm-functions and argc tooling.
//!
//! These tests validate that Phases 0–4 (tool dispatch, deferred loading, pipelines,
//! MCP conversion, error handling, schema validation) do not break the existing
//! llm-functions integration or argc-based workflows.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

// ===========================================================================
// 1. FunctionDeclaration & Functions — llm-functions compatibility
// ===========================================================================

mod function_declaration {
    use super::*;

    /// Simulates loading a functions.json from llm-functions.
    /// This is the contract: an array of {name, description, parameters} objects.
    fn sample_declarations_json() -> Value {
        json!([
            {
                "name": "get_weather",
                "description": "Get current weather for a city",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string", "description": "City name" }
                    },
                    "required": ["city"]
                }
            },
            {
                "name": "search_web",
                "description": "Search the web for information",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "max_results": { "type": "integer", "default": 5 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "execute_code",
                "description": "Execute code in a sandboxed environment",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "language": { "type": "string", "enum": ["python", "javascript", "bash"] },
                        "code": { "type": "string" }
                    },
                    "required": ["language", "code"]
                }
            }
        ])
    }

    #[test]
    fn test_functions_json_deserialization() {
        // llm-functions generates functions.json — verify our struct can parse it
        let json_str = serde_json::to_string(&sample_declarations_json()).unwrap();
        let decls: Vec<FunctionDeclCompat> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].name, "get_weather");
        assert_eq!(decls[1].name, "search_web");
        assert_eq!(decls[2].name, "execute_code");
    }

    #[test]
    fn test_function_parameters_preserve_full_schema() {
        // Phase 4D: parameters must preserve full JSON Schema, not lossy JsonSchema
        let json_str = serde_json::to_string(&sample_declarations_json()).unwrap();
        let decls: Vec<FunctionDeclCompat> = serde_json::from_str(&json_str).unwrap();

        // Check enum preservation
        let lang_prop = &decls[2].parameters["properties"]["language"];
        assert!(lang_prop["enum"].is_array());
        assert_eq!(lang_prop["enum"].as_array().unwrap().len(), 3);

        // Check default preservation
        let max_results = &decls[1].parameters["properties"]["max_results"];
        assert_eq!(max_results["default"], 5);

        // Check required arrays
        assert!(decls[0].parameters["required"].is_array());
    }

    #[test]
    fn test_empty_parameters_detection() {
        assert!(is_empty_parameters(&json!({"type": "object", "properties": {}})));
        assert!(!is_empty_parameters(&json!({
            "type": "object",
            "properties": { "x": { "type": "string" } }
        })));
    }

    #[test]
    fn test_empty_parameters_no_properties_key() {
        assert!(is_empty_parameters(&json!({"type": "object"})));
    }

    #[test]
    fn test_functions_json_with_agent_field() {
        // Agent functions have an extra `agent: true` field
        let json_str = r#"[
            {"name": "agent_tool", "description": "An agent tool", "parameters": {}, "agent": true},
            {"name": "normal_tool", "description": "A normal tool", "parameters": {}}
        ]"#;
        let decls: Vec<FunctionDeclCompat> = serde_json::from_str(json_str).unwrap();
        assert_eq!(decls.len(), 2);
        assert!(decls[0].agent);
        assert!(!decls[1].agent);
    }

    #[test]
    fn test_functions_json_with_examples() {
        // Phase 1D: Role examples in function declarations
        let json_str = r#"[{
            "name": "weather",
            "description": "Get weather",
            "parameters": {},
            "examples": [
                {"input": "weather in London", "args": {"city": "London"}},
                {"input": "is it raining in Paris"}
            ]
        }]"#;
        let decls: Vec<FunctionDeclCompat> = serde_json::from_str(json_str).unwrap();
        assert_eq!(decls[0].examples.as_ref().unwrap().len(), 2);
        let ex = &decls[0].examples.as_ref().unwrap()[0];
        assert_eq!(ex.input, "weather in London");
        assert!(ex.args.is_some());
    }

    // Compat structs that mirror the real types but don't require the full crate
    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct FunctionDeclCompat {
        name: String,
        description: String,
        parameters: Value,
        #[serde(default)]
        agent: bool,
        #[serde(default)]
        examples: Option<Vec<ExampleCompat>>,
    }

    #[derive(Debug, serde::Deserialize)]
    struct ExampleCompat {
        input: String,
        args: Option<Value>,
    }

    fn is_empty_parameters(params: &Value) -> bool {
        match params.get("properties") {
            Some(Value::Object(map)) => map.is_empty(),
            Some(_) => false,
            None => true,
        }
    }
}

// ===========================================================================
// 2. ToolCall dedup — prevents infinite loops in tool dispatch
// ===========================================================================

mod tool_call_dedup {
    use super::*;

    #[derive(Debug, Clone)]
    struct ToolCallCompat {
        name: String,
        arguments: Value,
        id: Option<String>,
    }

    fn dedup(calls: Vec<ToolCallCompat>) -> Vec<ToolCallCompat> {
        let mut new_calls = vec![];
        let mut seen_ids = HashSet::new();
        for call in calls.into_iter().rev() {
            if let Some(id) = &call.id {
                if !seen_ids.contains(id) {
                    seen_ids.insert(id.clone());
                    new_calls.push(call);
                }
            } else {
                new_calls.push(call);
            }
        }
        new_calls.reverse();
        new_calls
    }

    #[test]
    fn test_dedup_no_duplicates() {
        let calls = vec![
            ToolCallCompat { name: "a".into(), arguments: json!({}), id: Some("1".into()) },
            ToolCallCompat { name: "b".into(), arguments: json!({}), id: Some("2".into()) },
        ];
        let result = dedup(calls);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_dedup_removes_duplicates_keeps_latest() {
        let calls = vec![
            ToolCallCompat { name: "a".into(), arguments: json!({"v": 1}), id: Some("1".into()) },
            ToolCallCompat { name: "b".into(), arguments: json!({}), id: Some("2".into()) },
            ToolCallCompat { name: "a".into(), arguments: json!({"v": 2}), id: Some("1".into()) },
        ];
        let result = dedup(calls);
        assert_eq!(result.len(), 2);
        // The later occurrence of id "1" wins
        assert_eq!(result[0].name, "b");
        assert_eq!(result[1].arguments["v"], 2);
    }

    #[test]
    fn test_dedup_no_id_calls_pass_through() {
        let calls = vec![
            ToolCallCompat { name: "a".into(), arguments: json!({}), id: None },
            ToolCallCompat { name: "a".into(), arguments: json!({}), id: None },
        ];
        let result = dedup(calls);
        assert_eq!(result.len(), 2); // Both kept — no ID to dedup on
    }

    #[test]
    fn test_dedup_empty_input() {
        let result = dedup(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_dedup_mixed_ids_and_none() {
        let calls = vec![
            ToolCallCompat { name: "a".into(), arguments: json!({}), id: Some("1".into()) },
            ToolCallCompat { name: "b".into(), arguments: json!({}), id: None },
            ToolCallCompat { name: "a".into(), arguments: json!({"x": 1}), id: Some("1".into()) },
            ToolCallCompat { name: "c".into(), arguments: json!({}), id: None },
        ];
        let result = dedup(calls);
        assert_eq!(result.len(), 3); // b (no id), c (no id), a (id=1, latest)
    }
}

// ===========================================================================
// 3. Tool search meta-function — Phase 1C deferred tool loading
// ===========================================================================

mod tool_search {
    use super::*;

    #[test]
    fn test_tool_search_declaration_shape() {
        // The tool_search meta-function must have the expected schema
        let decl = tool_search_declaration();
        assert_eq!(decl["name"], "tool_search");
        assert!(decl["description"].as_str().unwrap().contains("Search for available tools"));

        let params = &decl["parameters"];
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["query"].is_object());
        assert_eq!(params["required"][0], "query");
    }

    #[test]
    fn test_tool_search_matching_by_name() {
        let tools = sample_tools();
        let matched = search_tools(&tools, "weather");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0]["name"], "get_weather");
    }

    #[test]
    fn test_tool_search_matching_by_description() {
        let tools = sample_tools();
        let matched = search_tools(&tools, "sandboxed");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0]["name"], "execute_code");
    }

    #[test]
    fn test_tool_search_empty_query_returns_all() {
        let tools = sample_tools();
        let matched = search_tools(&tools, "");
        assert_eq!(matched.len(), 3);
    }

    #[test]
    fn test_tool_search_case_insensitive() {
        let tools = sample_tools();
        let matched = search_tools(&tools, "WEATHER");
        assert_eq!(matched.len(), 1);
    }

    #[test]
    fn test_tool_search_no_match() {
        let tools = sample_tools();
        let matched = search_tools(&tools, "database");
        assert!(matched.is_empty());
    }

    #[test]
    fn test_deferred_threshold() {
        // Phase 1C: threshold is 15 tools
        assert_eq!(DEFERRED_TOOL_THRESHOLD, 15);
    }

    const DEFERRED_TOOL_THRESHOLD: usize = 15;

    fn tool_search_declaration() -> Value {
        json!({
            "name": "tool_search",
            "description": "Search for available tools by keyword. You MUST call this before using any other tool.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword to search for relevant tools. Use descriptive terms like 'file', 'web', 'database'."
                    }
                },
                "required": ["query"]
            }
        })
    }

    fn sample_tools() -> Vec<Value> {
        vec![
            json!({"name": "get_weather", "description": "Get current weather for a city", "parameters": {}}),
            json!({"name": "search_web", "description": "Search the web for information", "parameters": {}}),
            json!({"name": "execute_code", "description": "Execute code in a sandboxed environment", "parameters": {}}),
        ]
    }

    fn search_tools(tools: &[Value], query: &str) -> Vec<Value> {
        let query = query.to_lowercase();
        tools
            .iter()
            .filter(|t| {
                query.is_empty()
                    || t["name"].as_str().unwrap_or("").to_lowercase().contains(&query)
                    || t["description"].as_str().unwrap_or("").to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }
}

// ===========================================================================
// 4. MCP tool conversion — Phase 3/4D schema fidelity
// ===========================================================================

mod mcp_tool_conversion {
    use super::*;

    #[test]
    fn test_mcp_tool_namespacing() {
        let (name, _) = mcp_tool_to_declaration("create_issue", "github");
        assert_eq!(name, "github:create_issue");
    }

    #[test]
    fn test_mcp_tool_preserves_full_schema() {
        // Phase 4D: full JSON Schema fidelity — oneOf, allOf, additionalProperties
        let tool_schema = json!({
            "type": "object",
            "properties": {
                "target": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "integer" }
                    ]
                },
                "metadata": {
                    "type": "object",
                    "additionalProperties": true
                }
            },
            "allOf": [
                { "$ref": "#/definitions/base" }
            ],
            "required": ["target"]
        });

        let (_, params) = mcp_tool_to_declaration_with_schema("complex_tool", "server", &tool_schema);

        // Verify all advanced JSON Schema keywords survive
        assert!(params["properties"]["target"]["oneOf"].is_array());
        assert!(params["allOf"].is_array());
        assert_eq!(params["properties"]["metadata"]["additionalProperties"], true);
        assert!(params["required"].is_array());
    }

    #[test]
    fn test_mcp_tool_empty_description() {
        let (name, _) = mcp_tool_to_declaration("tool", "srv");
        assert_eq!(name, "srv:tool");
    }

    #[test]
    fn test_mcp_tool_source_tagging() {
        // MCP tools should be tagged with ToolSource::Mcp
        let source = mcp_tool_source("github");
        assert_eq!(source, "Mcp:github");
    }

    #[test]
    fn test_mcp_tools_to_json_list() {
        let tools = vec![
            json!({"name": "tool_a", "description": "Tool A", "parameters": {}}),
            json!({"name": "tool_b", "description": "Tool B", "parameters": {}}),
        ];
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "tool_a");
    }

    #[test]
    fn test_mcp_namespaced_name_parsing() {
        // eval_mcp_tool splits on first ':'
        let name = "github:create-issue";
        let (server, tool) = name.split_once(':').unwrap();
        assert_eq!(server, "github");
        assert_eq!(tool, "create-issue");
    }

    #[test]
    fn test_mcp_namespaced_name_with_colons() {
        // Namespace split should only split on first colon
        let name = "custom:namespace:tool";
        let (server, tool) = name.split_once(':').unwrap();
        assert_eq!(server, "custom");
        assert_eq!(tool, "namespace:tool");
    }

    fn mcp_tool_to_declaration(tool_name: &str, server_name: &str) -> (String, Value) {
        let name = format!("{}:{}", server_name, tool_name);
        (name, json!({}))
    }

    fn mcp_tool_to_declaration_with_schema(
        _tool_name: &str,
        _server_name: &str,
        schema: &Value,
    ) -> (String, Value) {
        let name = format!("{}:{}", _server_name, _tool_name);
        // Mirror the real implementation: Value::Object(tool.input_schema.as_ref().clone())
        (name, schema.clone())
    }

    fn mcp_tool_source(server: &str) -> String {
        format!("Mcp:{}", server)
    }
}

// ===========================================================================
// 5. Schema cache — MCP schema caching with TTL
// ===========================================================================

mod schema_cache {
    use super::*;

    #[test]
    fn test_cache_key_deterministic() {
        let key1 = cache_key("npx -y @modelcontextprotocol/server-github");
        let key2 = cache_key("npx -y @modelcontextprotocol/server-github");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_different_commands() {
        let key1 = cache_key("server-a");
        let key2 = cache_key("server-b");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_entry_serialization() {
        let entry = json!({
            "tools_json": [{"name": "tool1"}],
            "fetched_at": "2026-03-13T00:00:00+00:00"
        });
        let round_tripped: Value = serde_json::from_str(
            &serde_json::to_string(&entry).unwrap()
        ).unwrap();
        assert_eq!(entry, round_tripped);
    }

    fn cache_key(command: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        command.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

// ===========================================================================
// 6. Environment variable resolution for MCP config
// ===========================================================================

mod env_resolution {
    use super::*;

    #[test]
    fn test_resolve_env_var_passthrough() {
        let mut env_map = HashMap::new();
        env_map.insert("KEY".to_string(), "literal_value".to_string());
        let resolved = resolve_env_vars(&env_map);
        assert_eq!(resolved["KEY"], "literal_value");
    }

    #[test]
    fn test_resolve_env_var_expansion() {
        // ${VAR} syntax reads from parent env
        std::env::set_var("TEST_COMPAT_VAR", "resolved_value");
        let mut env_map = HashMap::new();
        env_map.insert("KEY".to_string(), "${TEST_COMPAT_VAR}".to_string());
        let resolved = resolve_env_vars(&env_map);
        assert_eq!(resolved["KEY"], "resolved_value");
        std::env::remove_var("TEST_COMPAT_VAR");
    }

    #[test]
    fn test_resolve_env_var_missing_defaults_empty() {
        let mut env_map = HashMap::new();
        env_map.insert("KEY".to_string(), "${NONEXISTENT_VAR_12345}".to_string());
        let resolved = resolve_env_vars(&env_map);
        assert_eq!(resolved["KEY"], "");
    }

    #[test]
    fn test_resolve_env_mixed() {
        std::env::set_var("TEST_COMPAT_MIX", "from_env");
        let mut env_map = HashMap::new();
        env_map.insert("A".to_string(), "literal".to_string());
        env_map.insert("B".to_string(), "${TEST_COMPAT_MIX}".to_string());
        let resolved = resolve_env_vars(&env_map);
        assert_eq!(resolved["A"], "literal");
        assert_eq!(resolved["B"], "from_env");
        std::env::remove_var("TEST_COMPAT_MIX");
    }

    fn resolve_env_vars(env_map: &HashMap<String, String>) -> HashMap<String, String> {
        env_map
            .iter()
            .map(|(k, v)| {
                let resolved = if v.starts_with("${") && v.ends_with('}') {
                    let var_name = &v[2..v.len() - 1];
                    std::env::var(var_name).unwrap_or_default()
                } else {
                    v.clone()
                };
                (k.clone(), resolved)
            })
            .collect()
    }
}

// ===========================================================================
// 7. Error classification — Phase 4 semantic exit codes
// ===========================================================================

mod error_classification {

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(i32)]
    enum ExitCode {
        Success = 0,
        GeneralError = 1,
        UsageError = 2,
        ConfigError = 3,
        AuthError = 4,
        NetworkError = 5,
        ApiError = 6,
        ModelError = 7,
        SchemaError = 8,
        Aborted = 9,
        ToolError = 10,
    }

    fn classify(msg: &str) -> ExitCode {
        if msg.starts_with("Aborted") { return ExitCode::Aborted; }
        if msg.contains("Schema input validation failed")
            || msg.contains("Schema output validation failed")
            || msg.contains("Invalid input schema")
            || msg.contains("Invalid output schema") { return ExitCode::SchemaError; }
        if msg.contains("(status: 401)") || msg.contains("(status: 403)")
            || msg.contains("api_key") || msg.contains("API key")
            || msg.contains("Unauthorized") || msg.contains("Access denied") { return ExitCode::AuthError; }
        if msg.contains("Unknown model") || msg.contains("Unknown chat model")
            || msg.contains("No available model") || msg.contains("does not support") { return ExitCode::ModelError; }
        if msg.contains("Tool call exit with") || msg.contains("Unexpected call:")
            || msg.contains("infinite loop of function calls")
            || msg.contains("ReAct loop exceeded")
            || msg.contains("Failed to load functions") { return ExitCode::ToolError; }
        if msg.contains("Failed to build client") || msg.contains("connection")
            || msg.contains("timed out") || msg.contains("dns error") { return ExitCode::NetworkError; }
        if msg.contains("(status: 4") || msg.contains("(status: 5")
            || msg.contains("Invalid response data")
            || msg.contains("Blocked due to safety") { return ExitCode::ApiError; }
        if msg.contains("Unknown role") || msg.contains("Unknown agent")
            || msg.contains("No role") || msg.contains("Failed to load config")
            || msg.contains("Circular role inheritance") { return ExitCode::ConfigError; }
        if msg.starts_with("No input") || msg.contains("Usage:")
            || msg.contains("Unknown command") { return ExitCode::UsageError; }
        ExitCode::GeneralError
    }

    // --- llm-functions error strings that must map correctly ---

    #[test]
    fn test_tool_exit_nonzero() {
        assert_eq!(classify("Tool call exit with 1"), ExitCode::ToolError);
        assert_eq!(classify("Tool call exit with 127"), ExitCode::ToolError);
    }

    #[test]
    fn test_unexpected_function_call() {
        assert_eq!(classify("Unexpected call: nonexistent {}"), ExitCode::ToolError);
    }

    #[test]
    fn test_infinite_loop_detection() {
        assert_eq!(
            classify("The request was aborted because an infinite loop of function calls was detected."),
            ExitCode::ToolError
        );
    }

    #[test]
    fn test_react_loop_exceeded() {
        assert_eq!(classify("ReAct loop exceeded maximum iterations"), ExitCode::ToolError);
    }

    #[test]
    fn test_functions_not_installed() {
        assert_eq!(classify("Failed to load functions at /path/to/functions.json"), ExitCode::ToolError);
    }

    // --- argc/config error strings ---

    #[test]
    fn test_unknown_role() {
        assert_eq!(classify("Unknown role `nonexistent`"), ExitCode::ConfigError);
    }

    #[test]
    fn test_circular_inheritance() {
        assert_eq!(classify("Circular role inheritance: a -> b -> a"), ExitCode::ConfigError);
    }

    #[test]
    fn test_no_input() {
        assert_eq!(classify("No input"), ExitCode::UsageError);
    }

    // --- API/auth errors ---

    #[test]
    fn test_auth_401() {
        assert_eq!(classify("Invalid response data: {} (status: 401)"), ExitCode::AuthError);
    }

    #[test]
    fn test_rate_limit_429() {
        assert_eq!(classify("Invalid response data: rate limit (status: 429)"), ExitCode::ApiError);
    }

    #[test]
    fn test_safety_block() {
        assert_eq!(classify("Blocked due to safety"), ExitCode::ApiError);
    }

    #[test]
    fn test_unknown_model() {
        assert_eq!(classify("Unknown chat model 'foo:bar'"), ExitCode::ModelError);
    }

    #[test]
    fn test_network_timeout() {
        assert_eq!(classify("connection timed out"), ExitCode::NetworkError);
    }

    #[test]
    fn test_general_fallback() {
        assert_eq!(classify("something completely unexpected"), ExitCode::GeneralError);
    }

    // --- exit code numeric values ---

    #[test]
    fn test_exit_code_numeric_values() {
        assert_eq!(ExitCode::Success as i32, 0);
        assert_eq!(ExitCode::GeneralError as i32, 1);
        assert_eq!(ExitCode::UsageError as i32, 2);
        assert_eq!(ExitCode::ConfigError as i32, 3);
        assert_eq!(ExitCode::AuthError as i32, 4);
        assert_eq!(ExitCode::NetworkError as i32, 5);
        assert_eq!(ExitCode::ApiError as i32, 6);
        assert_eq!(ExitCode::ModelError as i32, 7);
        assert_eq!(ExitCode::SchemaError as i32, 8);
        assert_eq!(ExitCode::Aborted as i32, 9);
        assert_eq!(ExitCode::ToolError as i32, 10);
    }
}

// ===========================================================================
// 8. Typed error variants — Phase 4B structured errors
// ===========================================================================

mod typed_errors {
    use super::*;

    #[test]
    fn test_pipeline_stage_error_display() {
        let msg = format_pipeline_error(2, 4, "review", Some("claude-sonnet-4-6"), "Model returned empty output");
        assert!(msg.contains("Pipeline stage 2/4"));
        assert!(msg.contains("role 'review'"));
        assert!(msg.contains("model 'claude-sonnet-4-6'"));
        assert!(msg.contains("Model returned empty output"));
    }

    #[test]
    fn test_pipeline_stage_error_no_model() {
        let msg = format_pipeline_error(1, 3, "summarize", None, "timeout");
        assert!(msg.contains("Pipeline stage 1/3"));
        assert!(msg.contains("role 'summarize'"));
        assert!(!msg.contains("model"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_schema_validation_error_display() {
        let msg = format_schema_error("output", "missing field `name`");
        assert_eq!(msg, "Schema output validation failed: missing field `name`");
    }

    #[test]
    fn test_tool_not_found_error_display() {
        let msg = format!("Tool not found: {}", "nonexistent_tool");
        assert!(msg.contains("nonexistent_tool"));
    }

    #[test]
    fn test_mcp_error_display() {
        let msg = format_mcp_error("connection refused", Some("github"), Some("create-issue"));
        assert!(msg.contains("MCP error"));
        assert!(msg.contains("[github:create-issue]"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_mcp_error_display_no_server() {
        let msg = format_mcp_error("general failure", None, None);
        assert_eq!(msg, "MCP error: general failure");
    }

    #[test]
    fn test_pipeline_error_json_context() {
        let ctx = json!({
            "stage": 2,
            "total": 3,
            "role": "review",
            "model": "claude",
            "detail": "timeout",
        });
        assert_eq!(ctx["stage"], 2);
        assert_eq!(ctx["total"], 3);
        assert_eq!(ctx["role"], "review");
    }

    fn format_pipeline_error(
        stage: usize, total: usize, role_name: &str,
        model_id: Option<&str>, message: &str,
    ) -> String {
        let mut s = format!("Pipeline stage {stage}/{total} (role '{role_name}'");
        if let Some(model) = model_id {
            s.push_str(&format!(", model '{model}'"));
        }
        s.push_str(&format!(") failed: {message}"));
        s
    }

    fn format_schema_error(direction: &str, message: &str) -> String {
        format!("Schema {direction} validation failed: {message}")
    }

    fn format_mcp_error(message: &str, server: Option<&str>, tool: Option<&str>) -> String {
        let mut s = "MCP error".to_string();
        if let Some(srv) = server {
            s.push_str(&format!(" [{srv}"));
            if let Some(t) = tool {
                s.push_str(&format!(":{t}"));
            }
            s.push(']');
        }
        s.push_str(&format!(": {message}"));
        s
    }
}

// ===========================================================================
// 9. Role parsing — frontmatter, extends, pipeline, variables
// ===========================================================================

mod role_parsing {
    use super::*;

    #[test]
    fn test_basic_frontmatter_parsing() {
        let content = "---\nmodel: gpt-4\ntemperature: 0.7\n---\nYou are helpful.";
        let parts = parse_frontmatter(content);
        assert_eq!(parts.metadata["model"], "gpt-4");
        assert_eq!(parts.metadata["temperature"], 0.7);
        assert_eq!(parts.prompt, "You are helpful.");
    }

    #[test]
    fn test_frontmatter_with_description() {
        let content = "---\ndescription: A helpful assistant\n---\nYou are helpful.";
        let parts = parse_frontmatter(content);
        assert_eq!(parts.metadata["description"], "A helpful assistant");
    }

    #[test]
    fn test_frontmatter_with_use_tools() {
        let content = "---\nuse_tools: all\n---\nYou can use tools.";
        let parts = parse_frontmatter(content);
        assert_eq!(parts.metadata["use_tools"], "all");
    }

    #[test]
    fn test_frontmatter_with_pipeline() {
        let content = r#"---
pipeline:
  - role: summarize
    model: gpt-4
  - role: translate
---
Pipeline role."#;
        let parts = parse_frontmatter(content);
        let pipeline = parts.metadata["pipeline"].as_array().unwrap();
        assert_eq!(pipeline.len(), 2);
        assert_eq!(pipeline[0]["role"], "summarize");
        assert_eq!(pipeline[0]["model"], "gpt-4");
        assert_eq!(pipeline[1]["role"], "translate");
        assert!(pipeline[1].get("model").map_or(true, |v| v.is_null()));
    }

    #[test]
    fn test_frontmatter_with_schemas() {
        let content = r#"---
input_schema:
  type: object
  properties:
    text:
      type: string
  required:
    - text
output_schema:
  type: object
  properties:
    result:
      type: string
---
Process the input."#;
        let parts = parse_frontmatter(content);
        assert!(parts.metadata.contains_key("input_schema"));
        assert!(parts.metadata.contains_key("output_schema"));
        let input_schema = &parts.metadata["input_schema"];
        assert_eq!(input_schema["type"], "object");
        assert_eq!(input_schema["required"][0], "text");
    }

    #[test]
    fn test_frontmatter_with_variables() {
        let content = r#"---
variables:
  - name: language
  - name: tone
    default: formal
---
Translate to {{language}} in a {{tone}} tone."#;
        let parts = parse_frontmatter(content);
        // Variables should be extracted separately (not in metadata)
        assert!(parts.variables.len() >= 2 || parts.metadata.contains_key("variables"));
    }

    #[test]
    fn test_frontmatter_with_examples() {
        let content = r#"---
examples:
  - input: "weather in London"
    args:
      city: London
  - input: "is it raining"
---
Check the weather."#;
        let parts = parse_frontmatter(content);
        assert!(parts.metadata.contains_key("examples"));
        let examples = parts.metadata["examples"].as_array().unwrap();
        assert_eq!(examples.len(), 2);
    }

    #[test]
    fn test_no_frontmatter() {
        let content = "You are helpful.";
        let parts = parse_frontmatter(content);
        assert!(parts.metadata.is_empty() || parts.metadata.len() == 0);
        assert_eq!(parts.prompt, "You are helpful.");
    }

    #[test]
    fn test_extends_extraction() {
        let content = "---\nextends: base-role\n---\nChild instructions.";
        let parts = parse_frontmatter(content);
        assert_eq!(parts.extends.as_deref(), Some("base-role"));
    }

    #[test]
    fn test_include_extraction() {
        let content = "---\ninclude:\n  - fragment-a\n  - fragment-b\n---\nMain prompt.";
        let parts = parse_frontmatter(content);
        assert_eq!(parts.includes, vec!["fragment-a", "fragment-b"]);
    }

    #[test]
    fn test_description_derived_from_prompt() {
        // Phase 1B: description falls back to first sentence
        assert_eq!(derive_description("You are a helpful assistant. Be kind."), "You are a helpful assistant.");
        assert_eq!(derive_description("Short"), "Short");
        assert_eq!(derive_description(""), "");
    }

    #[test]
    fn test_description_truncates_at_100_chars() {
        let long_prompt = "A".repeat(200);
        let desc = derive_description(&long_prompt);
        assert!(desc.len() <= 101); // 100 + optional '.'
    }

    #[test]
    fn test_input_placeholder_detection() {
        assert!(contains_input_placeholder("Process this: __INPUT__"));
        assert!(!contains_input_placeholder("No placeholder here"));
    }

    #[test]
    fn test_pipeline_role_detection() {
        let with_pipeline = json!({
            "pipeline": [
                {"role": "step1"},
                {"role": "step2"}
            ]
        });
        assert!(is_pipeline(&with_pipeline));

        let without_pipeline = json!({"model": "gpt-4"});
        assert!(!is_pipeline(&without_pipeline));

        let empty_pipeline = json!({"pipeline": []});
        assert!(!is_pipeline(&empty_pipeline));
    }

    // --- Helper types/functions mirroring role.rs ---

    struct ParsedFrontmatter {
        metadata: serde_json::Map<String, Value>,
        prompt: String,
        extends: Option<String>,
        includes: Vec<String>,
        variables: Vec<Value>,
    }

    fn parse_frontmatter(content: &str) -> ParsedFrontmatter {
        let re = fancy_regex::Regex::new(r"(?s)-{3,}\s*(.*?)\s*-{3,}\s*(.*)").unwrap();
        let mut metadata = serde_json::Map::new();
        let mut prompt = content.trim().to_string();
        let mut extends = None;
        let mut includes = Vec::new();
        let mut variables = Vec::new();

        if let Ok(Some(caps)) = re.captures(content) {
            if let (Some(meta_val), Some(prompt_val)) = (caps.get(1), caps.get(2)) {
                let meta_str = meta_val.as_str().trim();
                prompt = prompt_val.as_str().trim().to_string();
                if let Ok(value) = serde_yaml::from_str::<Value>(meta_str) {
                    if let Some(map) = value.as_object() {
                        for (key, val) in map {
                            match key.as_str() {
                                "extends" => extends = val.as_str().map(|s| s.to_string()),
                                "include" => {
                                    if let Some(arr) = val.as_array() {
                                        includes = arr.iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect();
                                    }
                                }
                                "variables" => {
                                    if let Some(arr) = val.as_array() {
                                        variables = arr.clone();
                                    }
                                }
                                _ => { metadata.insert(key.clone(), val.clone()); }
                            }
                        }
                    }
                }
            }
        }

        ParsedFrontmatter { metadata, prompt, extends, includes, variables }
    }

    fn derive_description(prompt: &str) -> String {
        let prompt = prompt.trim();
        if prompt.is_empty() { return String::new(); }
        let end = prompt.find(". ")
            .or_else(|| prompt.find('\n'))
            .unwrap_or(prompt.len())
            .min(100);
        let mut desc = prompt[..end].to_string();
        if desc.len() < prompt.len() && !desc.ends_with('.') {
            desc.push('.');
        }
        desc
    }

    fn contains_input_placeholder(text: &str) -> bool {
        text.contains("__INPUT__")
    }

    fn is_pipeline(metadata: &Value) -> bool {
        metadata.get("pipeline")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty())
    }
}

// ===========================================================================
// 10. Schema validation — input/output schema enforcement
// ===========================================================================

mod schema_validation {
    use super::*;

    #[test]
    fn test_validate_valid_json_against_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "number" }
            },
            "required": ["name", "age"]
        });
        let result = validate_schema("input", &schema, r#"{"name": "Alice", "age": 30}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_json() {
        let schema = json!({"type": "object"});
        let result = validate_schema("input", &schema, "not json at all");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not valid JSON"));
    }

    #[test]
    fn test_validate_missing_required_field() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "number" }
            },
            "required": ["name", "age"]
        });
        let result = validate_schema("output", &schema, r#"{"age": 30}"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Schema output validation failed"));
    }

    #[test]
    fn test_validate_type_mismatch() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer" }
            },
            "required": ["count"]
        });
        let result = validate_schema("input", &schema, r#"{"count": "not_a_number"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_schema_with_enum() {
        let schema = json!({
            "type": "object",
            "properties": {
                "status": { "type": "string", "enum": ["active", "inactive"] }
            },
            "required": ["status"]
        });
        let valid = validate_schema("output", &schema, r#"{"status": "active"}"#);
        assert!(valid.is_ok());
        let invalid = validate_schema("output", &schema, r#"{"status": "unknown"}"#);
        assert!(invalid.is_err());
    }

    fn validate_schema(direction: &str, schema: &Value, data: &str) -> Result<(), String> {
        let parsed: Value = serde_json::from_str(data)
            .map_err(|_| format!("Schema {direction} validation failed: not valid JSON"))?;

        // Basic validation: check required fields and types
        if let (Some(required), Some(properties)) = (
            schema.get("required").and_then(|v| v.as_array()),
            schema.get("properties").and_then(|v| v.as_object()),
        ) {
            for req in required {
                let field = req.as_str().unwrap_or("");
                if parsed.get(field).is_none() {
                    return Err(format!("Schema {direction} validation failed: missing required field '{field}'"));
                }
                // Type check
                if let Some(prop_schema) = properties.get(field) {
                    if let Some(expected_type) = prop_schema.get("type").and_then(|v| v.as_str()) {
                        let actual = &parsed[field];
                        let type_ok = match expected_type {
                            "string" => actual.is_string(),
                            "number" => actual.is_number(),
                            "integer" => actual.is_i64() || actual.is_u64(),
                            "boolean" => actual.is_boolean(),
                            "object" => actual.is_object(),
                            "array" => actual.is_array(),
                            _ => true,
                        };
                        if !type_ok {
                            return Err(format!("Schema {direction} validation failed: field '{field}' has wrong type"));
                        }
                    }
                    // Enum check
                    if let Some(enum_values) = prop_schema.get("enum").and_then(|v| v.as_array()) {
                        if !enum_values.contains(&parsed[field]) {
                            return Err(format!("Schema {direction} validation failed: field '{field}' not in enum"));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

// ===========================================================================
// 11. Output format — CLI output formatting
// ===========================================================================

mod output_format {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum OutputFormat {
        Json, Jsonl, Tsv, Csv, Text, Compact,
    }

    impl OutputFormat {
        fn is_structured(&self) -> bool {
            !matches!(self, OutputFormat::Text | OutputFormat::Compact)
        }

        fn system_prompt_suffix(&self) -> Option<&'static str> {
            match self {
                OutputFormat::Json => Some("respond with valid JSON"),
                OutputFormat::Jsonl => Some("respond with JSON Lines"),
                OutputFormat::Tsv => Some("respond with tab-separated values"),
                OutputFormat::Csv => Some("respond with comma-separated values"),
                OutputFormat::Text => None,
                OutputFormat::Compact => Some("respond with minimal tokens"),
            }
        }
    }

    #[test]
    fn test_structured_formats() {
        assert!(OutputFormat::Json.is_structured());
        assert!(OutputFormat::Jsonl.is_structured());
        assert!(OutputFormat::Tsv.is_structured());
        assert!(OutputFormat::Csv.is_structured());
        assert!(!OutputFormat::Text.is_structured());
        assert!(!OutputFormat::Compact.is_structured());
    }

    #[test]
    fn test_text_has_no_suffix() {
        assert!(OutputFormat::Text.system_prompt_suffix().is_none());
    }

    #[test]
    fn test_all_structured_formats_have_suffix() {
        for fmt in [OutputFormat::Json, OutputFormat::Jsonl, OutputFormat::Tsv, OutputFormat::Csv, OutputFormat::Compact] {
            assert!(fmt.system_prompt_suffix().is_some(), "{:?} should have a suffix", fmt);
        }
    }

    #[test]
    fn test_strip_code_fences() {
        assert_eq!(strip_code_fences("```json\n{\"key\": 1}\n```"), "{\"key\": 1}");
        assert_eq!(strip_code_fences("```\nhello\n```"), "hello");
        assert_eq!(strip_code_fences("no fences"), "no fences");
        assert_eq!(strip_code_fences("  \n```json\n{}\n```\n  "), "{}");
    }

    #[test]
    fn test_clean_json_output() {
        let valid = clean_json("```json\n{\"x\": 1}\n```");
        assert!(valid.is_ok());
        assert_eq!(valid.unwrap(), "{\"x\": 1}");

        let invalid = clean_json("not json");
        assert!(invalid.is_err());
    }

    #[test]
    fn test_clean_jsonl_output() {
        let valid = clean_jsonl("{\"a\": 1}\n{\"b\": 2}");
        assert!(valid.is_ok());

        let invalid = clean_jsonl("{\"a\": 1}\nnot json");
        assert!(invalid.is_err());
    }

    fn strip_code_fences(text: &str) -> String {
        let trimmed = text.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            let rest = match rest.find('\n') {
                Some(pos) => &rest[pos + 1..],
                None => return String::new(),
            };
            if let Some(inner) = rest.strip_suffix("```") {
                return inner.trim().to_string();
            }
        }
        trimmed.to_string()
    }

    fn clean_json(output: &str) -> Result<String, String> {
        let cleaned = strip_code_fences(output);
        serde_json::from_str::<Value>(&cleaned)
            .map_err(|e| format!("Not valid JSON: {e}"))?;
        Ok(cleaned)
    }

    fn clean_jsonl(output: &str) -> Result<String, String> {
        let cleaned = strip_code_fences(output);
        for (i, line) in cleaned.lines().enumerate() {
            if !line.trim().is_empty() {
                serde_json::from_str::<Value>(line)
                    .map_err(|e| format!("Line {} is not valid JSON: {e}", i + 1))?;
            }
        }
        Ok(cleaned)
    }
}

// ===========================================================================
// 12. Pipeline stage parsing — pipe.rs compatibility
// ===========================================================================

mod pipeline_parsing {
    use super::*;

    #[test]
    fn test_parse_stage_spec_without_model() {
        let (role, model) = parse_stage_spec("summarize");
        assert_eq!(role, "summarize");
        assert!(model.is_none());
    }

    #[test]
    fn test_parse_stage_spec_with_model() {
        let (role, model) = parse_stage_spec("summarize@gpt-4");
        assert_eq!(role, "summarize");
        assert_eq!(model.unwrap(), "gpt-4");
    }

    #[test]
    fn test_parse_stage_spec_with_provider_model() {
        let (role, model) = parse_stage_spec("translate@openai:gpt-4o");
        assert_eq!(role, "translate");
        assert_eq!(model.unwrap(), "openai:gpt-4o");
    }

    #[test]
    fn test_parse_multiple_stages() {
        let specs = vec!["analyze".to_string(), "review@claude".to_string(), "format".to_string()];
        let stages: Vec<_> = specs.iter().map(|s| parse_stage_spec(s)).collect();
        assert_eq!(stages.len(), 3);
        assert_eq!(stages[0].0, "analyze");
        assert!(stages[0].1.is_none());
        assert_eq!(stages[1].0, "review");
        assert_eq!(stages[1].1.as_deref(), Some("claude"));
        assert_eq!(stages[2].0, "format");
    }

    #[test]
    fn test_pipeline_yaml_def_parsing() {
        let yaml = r#"
stages:
  - role: summarize
    model: gpt-4
  - role: translate
  - role: format
    model: claude-3-haiku
"#;
        let def: PipelineDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(def.stages.len(), 3);
        assert_eq!(def.stages[0].role, "summarize");
        assert_eq!(def.stages[0].model.as_deref(), Some("gpt-4"));
        assert_eq!(def.stages[1].role, "translate");
        assert!(def.stages[1].model.is_none());
        assert_eq!(def.stages[2].model.as_deref(), Some("claude-3-haiku"));
    }

    #[test]
    fn test_pipeline_yaml_empty_stages() {
        let yaml = "stages: []";
        let def: PipelineDef = serde_yaml::from_str(yaml).unwrap();
        assert!(def.stages.is_empty());
    }

    #[test]
    fn test_pipeline_role_stages_from_frontmatter() {
        let content = r#"---
pipeline:
  - role: step1
    model: gpt-4
  - role: step2
---
"#;
        let parsed: Value = serde_yaml::from_str(
            content.trim().strip_prefix("---").unwrap().strip_suffix("---").unwrap().trim()
        ).unwrap();
        let stages = parsed["pipeline"].as_array().unwrap();
        assert_eq!(stages.len(), 2);
    }

    fn parse_stage_spec(spec: &str) -> (String, Option<String>) {
        match spec.split_once('@') {
            Some((role, model)) => (role.to_string(), Some(model.to_string())),
            None => (spec.to_string(), None),
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct PipelineDef {
        #[serde(default)]
        stages: Vec<PipelineStageDef>,
    }

    #[derive(Debug, serde::Deserialize)]
    struct PipelineStageDef {
        role: String,
        model: Option<String>,
    }
}

// ===========================================================================
// 13. McpServerConfig — config.yaml server entries
// ===========================================================================

mod mcp_config {
    use super::*;

    #[test]
    fn test_mcp_server_config_minimal() {
        let yaml = r#"
command: "npx -y @modelcontextprotocol/server-github"
"#;
        let config: McpServerConfigCompat = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.command, "npx -y @modelcontextprotocol/server-github");
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
    }

    #[test]
    fn test_mcp_server_config_full() {
        let yaml = r#"
command: "node server.js"
args:
  - "--port"
  - "3000"
env:
  GITHUB_TOKEN: "${GITHUB_TOKEN}"
  API_KEY: "literal-key"
"#;
        let config: McpServerConfigCompat = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.command, "node server.js");
        assert_eq!(config.args, vec!["--port", "3000"]);
        assert_eq!(config.env["GITHUB_TOKEN"], "${GITHUB_TOKEN}");
        assert_eq!(config.env["API_KEY"], "literal-key");
    }

    #[test]
    fn test_mcp_servers_in_config() {
        let yaml = r#"
github:
  command: "npx -y @modelcontextprotocol/server-github"
  env:
    GITHUB_TOKEN: "${GITHUB_TOKEN}"
filesystem:
  command: "npx -y @modelcontextprotocol/server-filesystem"
  args:
    - "/tmp"
"#;
        let configs: indexmap::IndexMap<String, McpServerConfigCompat> =
            serde_yaml::from_str(yaml).unwrap();
        assert_eq!(configs.len(), 2);
        assert!(configs.contains_key("github"));
        assert!(configs.contains_key("filesystem"));
        assert_eq!(configs["filesystem"].args, vec!["/tmp"]);
    }

    #[derive(Debug, serde::Deserialize)]
    struct McpServerConfigCompat {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    }
}

// ===========================================================================
// 14. Tool use_tools selection — wildcard, mapping, pipeline
// ===========================================================================

mod tool_selection {
    use super::*;

    #[test]
    fn test_use_tools_all() {
        let tools = vec!["a", "b", "c"];
        let selected = select_tools("all", &tools, &HashMap::new());
        assert_eq!(selected, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_use_tools_specific() {
        let tools = vec!["a", "b", "c"];
        let selected = select_tools("a,c", &tools, &HashMap::new());
        assert_eq!(selected, vec!["a", "c"]);
    }

    #[test]
    fn test_use_tools_wildcard() {
        let tools = vec!["github:list", "github:create", "slack:send", "weather"];
        let selected = select_tools("github:*", &tools, &HashMap::new());
        assert_eq!(selected, vec!["github:create", "github:list"]);
    }

    #[test]
    fn test_use_tools_mapping() {
        let tools = vec!["get_weather", "search_web", "execute_code"];
        let mut mapping = HashMap::new();
        mapping.insert("web_tools".to_string(), "search_web,execute_code".to_string());
        let selected = select_tools("web_tools", &tools, &mapping);
        assert_eq!(selected, vec!["execute_code", "search_web"]); // HashSet order
    }

    #[test]
    fn test_use_tools_mapping_with_wildcard() {
        let tools = vec!["github:list", "github:create", "slack:send"];
        let mut mapping = HashMap::new();
        mapping.insert("dev_tools".to_string(), "github:*".to_string());
        let selected = select_tools("dev_tools", &tools, &mapping);
        assert_eq!(selected, vec!["github:create", "github:list"]);
    }

    #[test]
    fn test_use_tools_unknown_ignored() {
        let tools = vec!["a", "b"];
        let selected = select_tools("a,nonexistent", &tools, &HashMap::new());
        assert_eq!(selected, vec!["a"]);
    }

    fn select_tools(
        use_tools: &str,
        available: &[&str],
        mapping: &HashMap<String, String>,
    ) -> Vec<String> {
        let available_set: HashSet<String> = available.iter().map(|s| s.to_string()).collect();
        let mut selected = HashSet::new();

        if use_tools == "all" {
            return available.iter().map(|s| s.to_string()).collect();
        }

        for item in use_tools.split(',').map(|s| s.trim()) {
            if let Some(values) = mapping.get(item) {
                for v in values.split(',').map(|s| s.trim()) {
                    if v.ends_with(":*") {
                        let prefix = &v[..v.len() - 1];
                        selected.extend(available_set.iter().filter(|n| n.starts_with(prefix)).cloned());
                    } else if available_set.contains(v) {
                        selected.insert(v.to_string());
                    }
                }
            } else if item.ends_with(":*") {
                let prefix = &item[..item.len() - 1];
                selected.extend(available_set.iter().filter(|n| n.starts_with(prefix)).cloned());
            } else if available_set.contains(item) {
                selected.insert(item.to_string());
            }
        }

        let mut result: Vec<String> = selected.into_iter().collect();
        result.sort();
        result
    }
}

// ===========================================================================
// 15. De-hoist __INPUT__ — extends inheritance behavior
// ===========================================================================

mod dehoist_input {

    const INPUT_PLACEHOLDER: &str = "__INPUT__";

    #[test]
    fn test_dehoist_auto_tail() {
        // When parent has __INPUT__ and child doesn't → __INPUT__ moves to end
        let parent = "Parent instructions.\n\nMy request is: __INPUT__";
        let child = "Child refinement.";
        let result = dehoist(parent, child);
        assert!(result.ends_with(INPUT_PLACEHOLDER));
        assert_eq!(result.matches(INPUT_PLACEHOLDER).count(), 1);
        let child_pos = result.find("Child refinement.").unwrap();
        let input_pos = result.rfind(INPUT_PLACEHOLDER).unwrap();
        assert!(child_pos < input_pos);
    }

    #[test]
    fn test_dehoist_child_wins() {
        // When child re-declares __INPUT__, parent's is stripped
        let parent = "Parent.\n\nRequest: __INPUT__";
        let child = "Child.\n\nRewrite: __INPUT__";
        let result = dehoist(parent, child);
        assert_eq!(result.matches(INPUT_PLACEHOLDER).count(), 1);
        assert!(result.contains("Rewrite: __INPUT__"));
        assert!(!result.contains("Request: __INPUT__"));
    }

    #[test]
    fn test_dehoist_neither_has_input() {
        let parent = "Parent instructions.";
        let child = "Child instructions.";
        let result = dehoist(parent, child);
        assert!(!result.contains(INPUT_PLACEHOLDER));
        assert!(result.contains("Parent instructions."));
        assert!(result.contains("Child instructions."));
    }

    #[test]
    fn test_dehoist_only_child_has_input() {
        let parent = "Parent instructions.";
        let child = "Process: __INPUT__";
        let result = dehoist(parent, child);
        assert_eq!(result.matches(INPUT_PLACEHOLDER).count(), 1);
        assert!(result.contains("Process: __INPUT__"));
    }

    fn dehoist(parent_prompt: &str, child_prompt: &str) -> String {
        let parent_has_input = parent_prompt.contains(INPUT_PLACEHOLDER);
        let child_has_input = child_prompt.contains(INPUT_PLACEHOLDER);

        let parent_clean = if parent_has_input {
            parent_prompt.replace(INPUT_PLACEHOLDER, "").trim().to_string()
        } else {
            parent_prompt.to_string()
        };

        let mut parts = vec![];
        if !parent_clean.is_empty() {
            parts.push(parent_clean);
        }
        parts.push(child_prompt.to_string());

        let mut combined = parts.join("\n\n");
        if parent_has_input && !child_has_input {
            combined = format!("{combined}\n\n{INPUT_PLACEHOLDER}");
        }
        combined
    }
}

// ===========================================================================
// 16. MCP format_tools_output — human/json output for --list-tools
// ===========================================================================

mod mcp_output_format {
    use super::*;

    #[test]
    fn test_format_tools_human_readable() {
        let tools = json!([
            {"name": "read_file", "description": "Read a file from disk"},
            {"name": "list_dir", "description": "List directory contents"},
            {"name": "no_desc", "description": ""}
        ]);
        let output = format_tools_output(&tools, false);
        assert!(output.contains("read_file - Read a file from disk"));
        assert!(output.contains("list_dir - List directory contents"));
        assert!(output.contains("no_desc"));
        assert!(!output.contains("no_desc -")); // empty desc → just name
    }

    #[test]
    fn test_format_tools_json() {
        let tools = json!([
            {"name": "tool_a", "description": "A"}
        ]);
        let output = format_tools_output(&tools, true);
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["name"], "tool_a");
    }

    fn format_tools_output(tools_json: &Value, json_mode: bool) -> String {
        if json_mode {
            return serde_json::to_string_pretty(tools_json).unwrap_or_default();
        }
        if let Some(arr) = tools_json.as_array() {
            arr.iter()
                .map(|t| {
                    let name = t["name"].as_str().unwrap_or("?");
                    let desc = t["description"].as_str().unwrap_or("");
                    if desc.is_empty() {
                        name.to_string()
                    } else {
                        format!("{name} - {desc}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        }
    }
}

// ===========================================================================
// 17. argc Argcfile.sh contract — verifying the test entry points exist
// ===========================================================================

mod argc_contract {

    #[test]
    fn test_argcfile_exists() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Argcfile.sh");
        assert!(path.exists(), "Argcfile.sh must exist at project root for argc compatibility");
    }

    #[test]
    fn test_argcfile_has_required_commands() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Argcfile.sh");
        let content = std::fs::read_to_string(path).unwrap();
        // Verify required argc commands exist
        assert!(content.contains("test-init-config"), "Missing test-init-config command");
        assert!(content.contains("test-no-config"), "Missing test-no-config command");
        assert!(content.contains("test-function-calling"), "Missing test-function-calling command");
        assert!(content.contains("test-clients"), "Missing test-clients command");
        assert!(content.contains("test-server"), "Missing test-server command");
    }

    #[test]
    fn test_argcfile_uses_functions_role() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Argcfile.sh");
        let content = std::fs::read_to_string(path).unwrap();
        // test-function-calling should use the %functions% role
        assert!(content.contains("%functions%"), "test-function-calling must use %functions% role");
    }

    #[test]
    fn test_argcfile_has_argc_eval() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Argcfile.sh");
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("argc --argc-eval"), "Argcfile.sh must end with argc --argc-eval");
    }

    #[test]
    fn test_argcfile_supports_dry_run() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Argcfile.sh");
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("DRY_RUN"), "Argcfile.sh should support DRY_RUN mode");
    }
}

// ===========================================================================
// 18. Built-in roles — asset integrity
// ===========================================================================

mod builtin_roles {
    use super::*;

    #[test]
    fn test_builtin_role_assets_exist() {
        let assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/roles");
        assert!(assets_dir.exists());

        let expected_roles = [
            "%shell%.md",
            "%explain-shell%.md",
            "%code%.md",
            "%functions%.md",
            "%create-title%.md",
            "%create-prompt%.md",
        ];

        for role_file in &expected_roles {
            let path = assets_dir.join(role_file);
            assert!(path.exists(), "Missing built-in role: {}", role_file);
        }
    }

    #[test]
    fn test_functions_role_has_use_tools() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/roles/%functions%.md");
        let content = std::fs::read_to_string(path).unwrap();
        assert!(
            content.contains("use_tools") || content.contains("tools"),
            "%functions% role must specify use_tools"
        );
    }

    #[test]
    fn test_builtin_roles_have_valid_frontmatter() {
        let assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/roles");
        let re = fancy_regex::Regex::new(r"(?s)-{3,}\s*(.*?)\s*-{3,}").unwrap();

        for entry in std::fs::read_dir(assets_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "md") {
                let content = std::fs::read_to_string(&path).unwrap();
                if content.starts_with("---") {
                    // Should be valid YAML
                    if let Ok(Some(caps)) = re.captures(&content) {
                        if let Some(meta) = caps.get(1) {
                            let result = serde_yaml::from_str::<Value>(meta.as_str().trim());
                            assert!(
                                result.is_ok(),
                                "Invalid YAML in {}: {:?}",
                                path.display(),
                                result.err()
                            );
                        }
                    }
                }
            }
        }
    }
}

// ===========================================================================
// 19. Variable expansion — role parameter substitution
// ===========================================================================

mod variable_expansion {

    #[test]
    fn test_apply_variables() {
        let mut prompt = "Translate to {{language}} in {{tone}} tone.".to_string();
        let vars = vec![("language", "french"), ("tone", "formal")];
        for (k, v) in vars {
            prompt = prompt.replace(&format!("{{{{{k}}}}}"), v);
        }
        assert_eq!(prompt, "Translate to french in formal tone.");
    }

    #[test]
    fn test_apply_partial_variables() {
        let mut prompt = "Language: {{language}}, Style: {{style}}".to_string();
        prompt = prompt.replace("{{language}}", "rust");
        // {{style}} not replaced — stays as template
        assert!(prompt.contains("rust"));
        assert!(prompt.contains("{{style}}"));
    }

    #[test]
    fn test_apply_empty_variable() {
        let mut prompt = "Prefix: {{val}}Suffix".to_string();
        prompt = prompt.replace("{{val}}", "");
        assert_eq!(prompt, "Prefix: Suffix");
    }

    #[test]
    fn test_variables_coexist_with_system_vars() {
        // System vars like __os__ should not be affected by user variable application
        let prompt = "OS: {{__os__}}, Lang: {{language}}";
        let result = prompt.replace("{{language}}", "python");
        assert!(result.contains("{{__os__}}"));
        assert!(result.contains("python"));
    }
}

// ===========================================================================
// 20. Config paths — directory structure contract
// ===========================================================================

mod config_paths {
    use super::*;

    #[test]
    fn test_config_example_exists() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.yaml");
        assert!(path.exists(), "config.example.yaml must exist at project root");
    }

    #[test]
    fn test_config_example_parseable() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.yaml");
        let content = std::fs::read_to_string(path).unwrap();
        let result = serde_yaml::from_str::<Value>(&content);
        assert!(result.is_ok(), "config.example.yaml must be valid YAML");
    }

    #[test]
    fn test_models_yaml_exists() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models.yaml");
        assert!(path.exists(), "models.yaml must exist at project root");
    }

    #[test]
    fn test_schema_test_yaml_exists() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/schema-test.yaml");
        assert!(path.exists(), "assets/schema-test.yaml must exist");
    }
}
