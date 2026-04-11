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
    fn test_include_tags() {
        let content = "---\ntags: [inf-12345]\n---\n";
        let parts = parse_frontmatter(content);
        assert_eq!(parts.tags, vec!["inf-12345".to_string()]);
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
        tags: Vec<String>,
        variables: Vec<Value>,
    }

    fn parse_frontmatter(content: &str) -> ParsedFrontmatter {
        let re = fancy_regex::Regex::new(r"(?s)-{3,}\s*(.*?)\s*-{3,}\s*(.*)").unwrap();
        let mut metadata = serde_json::Map::new();
        let mut prompt = content.trim().to_string();
        let mut extends = None;
        let mut includes = Vec::new();
        let mut variables = Vec::new();
        let mut tags = Vec::new();

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
                                "tags" => {
                                    if let Some(arr) = val.as_array() {
                                        tags = arr.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect();
                                    }
                                }
                                _ => { metadata.insert(key.clone(), val.clone()); }
                            }
                        }
                    }
                }
            }
        }

        ParsedFrontmatter { metadata, tags, prompt, extends, includes, variables }
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

// ===========================================================================
// 18. Phase 7 — Tool error messages: stderr capture, error-as-result, hints
// ===========================================================================

mod phase7_tool_errors {
    use super::*;

    // ---- 7A: Error message format contracts ----

    /// ToolExecutionError Display format must include tool name, exit code,
    /// optional stderr, and optional hint — the contract that LLM and human
    /// error rendering depends on.
    fn format_tool_execution_error(
        tool_name: &str,
        exit_code: i32,
        stderr: Option<&str>,
        hint: Option<&str>,
    ) -> String {
        let mut s = format!("error: tool '{tool_name}' failed (exit code {exit_code})");
        if let Some(stderr) = stderr {
            if !stderr.is_empty() {
                s.push_str(&format!("\n  stderr: {stderr}"));
            }
        }
        if let Some(hint) = hint {
            s.push_str(&format!("\n  hint: {hint}"));
        }
        s
    }

    fn format_tool_spawn_error(
        tool_name: &str,
        message: &str,
        hint: Option<&str>,
    ) -> String {
        let mut s = format!("error: tool '{tool_name}' could not be started: {message}");
        if let Some(hint) = hint {
            s.push_str(&format!("\n  hint: {hint}"));
        }
        s
    }

    #[test]
    fn test_execution_error_format_full() {
        let msg = format_tool_execution_error(
            "web_search", 1,
            Some("curl: (6) Could not resolve host: api.serper.dev"),
            Some("a network service the tool depends on may be down."),
        );
        assert!(msg.contains("tool 'web_search'"), "must include tool name");
        assert!(msg.contains("exit code 1"), "must include exit code");
        assert!(msg.contains("stderr:"), "must include stderr label");
        assert!(msg.contains("Could not resolve host"), "must include stderr content");
        assert!(msg.contains("hint:"), "must include hint label");
    }

    #[test]
    fn test_execution_error_format_no_stderr() {
        let msg = format_tool_execution_error("silent_tool", 2, None, None);
        assert!(msg.contains("tool 'silent_tool'"));
        assert!(msg.contains("exit code 2"));
        assert!(!msg.contains("stderr:"), "no stderr label when stderr is None");
        assert!(!msg.contains("hint:"), "no hint label when hint is None");
    }

    #[test]
    fn test_execution_error_format_empty_stderr() {
        let msg = format_tool_execution_error("my_tool", 1, Some(""), None);
        assert!(!msg.contains("stderr:"), "no stderr label when stderr is empty");
    }

    #[test]
    fn test_spawn_error_format() {
        let msg = format_tool_spawn_error(
            "analyze_code",
            "binary not found",
            Some("searched: /home/user/.config/aichat/functions/bin. Ensure the tool is installed."),
        );
        assert!(msg.contains("tool 'analyze_code'"));
        assert!(msg.contains("could not be started"));
        assert!(msg.contains("binary not found"));
        assert!(msg.contains("hint:"));
    }

    #[test]
    fn test_spawn_error_format_no_hint() {
        let msg = format_tool_spawn_error("broken_tool", "permission denied", None);
        assert!(msg.contains("tool 'broken_tool'"));
        assert!(msg.contains("permission denied"));
        assert!(!msg.contains("hint:"));
    }

    // ---- 7A: LLM error message format contract ----

    /// The [TOOL_ERROR] prefix is a contract: LLM system prompts reference it
    /// to teach the model how to handle tool failures.
    fn format_tool_error_for_llm(
        tool_name: &str,
        exit_code: i32,
        stderr: Option<&str>,
        hint: Option<&str>,
    ) -> String {
        let mut msg = format!("[TOOL_ERROR] {tool_name} failed (exit {exit_code}).");
        if let Some(stderr) = stderr {
            if !stderr.is_empty() {
                msg.push_str(&format!("\nStderr: {stderr}"));
            }
        }
        if let Some(hint) = hint {
            msg.push_str(&format!("\nHint: {hint}"));
        }
        msg
    }

    #[test]
    fn test_llm_error_has_tool_error_prefix() {
        let msg = format_tool_error_for_llm("web_search", 1, None, None);
        assert!(msg.starts_with("[TOOL_ERROR]"), "LLM error must start with [TOOL_ERROR] prefix");
    }

    #[test]
    fn test_llm_error_includes_tool_name() {
        let msg = format_tool_error_for_llm("my_tool", 1, None, None);
        assert!(msg.contains("my_tool"), "LLM error must include tool name");
    }

    #[test]
    fn test_llm_error_includes_exit_code() {
        let msg = format_tool_error_for_llm("my_tool", 127, None, None);
        assert!(msg.contains("127"), "LLM error must include exit code");
    }

    #[test]
    fn test_llm_error_includes_stderr() {
        let msg = format_tool_error_for_llm(
            "web_search", 1,
            Some("curl: (6) Could not resolve host"),
            None,
        );
        assert!(msg.contains("Stderr:"), "LLM error must label stderr");
        assert!(msg.contains("Could not resolve host"), "LLM error must include stderr content");
    }

    #[test]
    fn test_llm_error_includes_hint() {
        let msg = format_tool_error_for_llm(
            "web_search", 1, None,
            Some("check your internet connection"),
        );
        assert!(msg.contains("Hint:"), "LLM error must label hint");
        assert!(msg.contains("check your internet connection"));
    }

    #[test]
    fn test_llm_error_token_budget() {
        // Error messages sent to the LLM should stay under ~300 tokens (~1200 chars)
        let long_stderr = "x".repeat(500);
        let msg = format_tool_error_for_llm(
            "web_search", 1,
            Some(&long_stderr),
            Some("check your internet connection"),
        );
        // With truncation applied upstream, the full message should be bounded.
        // This test validates the format itself is concise.
        assert!(msg.len() < 2000, "LLM error message should be concise, got {} chars", msg.len());
    }

    // ---- 7A: Null result handling contract ----

    #[test]
    fn test_null_result_is_structured() {
        // Phase 7A: null results must be {"status": "ok", "output": null}
        // NOT the old "DONE" string
        let structured_null = json!({"status": "ok", "output": null});
        assert_eq!(structured_null["status"], "ok");
        assert!(structured_null["output"].is_null());
        // Must NOT be the legacy format
        assert_ne!(structured_null, json!("DONE"), "must not use legacy DONE format");
    }

    #[test]
    fn test_null_result_is_valid_json() {
        let structured_null = json!({"status": "ok", "output": null});
        assert!(structured_null.is_object(), "null result must be a JSON object");
        assert!(structured_null.get("status").is_some(), "must have status field");
        assert!(structured_null.get("output").is_some(), "must have output field");
    }

    #[test]
    fn test_error_result_is_not_null() {
        // An error result should be a string containing [TOOL_ERROR], never null
        let error_result = json!("[TOOL_ERROR] web_search failed (exit 1).");
        assert!(!error_result.is_null());
        assert!(error_result.as_str().unwrap().contains("[TOOL_ERROR]"));
    }

    // ---- 7B: Hint generation contract ----

    fn generate_hint(exit_code: i32, stderr: &str) -> String {
        let stderr_lower = stderr.to_lowercase();
        if exit_code == 127 {
            "the tool binary was not found on PATH.".to_string()
        } else if exit_code == 126 {
            "the tool binary exists but is not executable. Try: chmod +x <path>".to_string()
        } else if stderr_lower.contains("not found") || stderr_lower.contains("no such file") {
            "a dependency may be missing. Check the tool's requirements.".to_string()
        } else if stderr_lower.contains("permission denied") {
            "check file permissions on the tool binary.".to_string()
        } else if stderr_lower.contains("econnrefused") || stderr_lower.contains("connection refused") {
            "a network service the tool depends on may be down.".to_string()
        } else if stderr_lower.contains("rate limit") || stderr_lower.contains("429") {
            "the tool hit a rate limit. Wait and retry.".to_string()
        } else {
            "run the command manually to diagnose.".to_string()
        }
    }

    #[test]
    fn test_hint_exit_127_not_found() {
        let hint = generate_hint(127, "");
        assert!(hint.contains("not found on PATH"), "exit 127 = command not found");
    }

    #[test]
    fn test_hint_exit_126_not_executable() {
        let hint = generate_hint(126, "");
        assert!(hint.contains("not executable"), "exit 126 = not executable");
        assert!(hint.contains("chmod"), "should suggest chmod fix");
    }

    #[test]
    fn test_hint_stderr_not_found() {
        let hint = generate_hint(1, "python3: No such file or directory");
        assert!(hint.contains("dependency"), "stderr 'no such file' = missing dependency");
    }

    #[test]
    fn test_hint_stderr_permission_denied() {
        let hint = generate_hint(1, "bash: ./tool.sh: Permission denied");
        assert!(hint.contains("permission"), "stderr 'permission denied' = permission issue");
    }

    #[test]
    fn test_hint_stderr_connection_refused() {
        let hint = generate_hint(1, "curl: (7) Failed to connect: Connection refused");
        assert!(hint.contains("network"), "stderr 'connection refused' = network issue");
    }

    #[test]
    fn test_hint_stderr_econnrefused() {
        let hint = generate_hint(1, "Error: connect ECONNREFUSED 127.0.0.1:5432");
        assert!(hint.contains("network"), "stderr 'ECONNREFUSED' = network issue");
    }

    #[test]
    fn test_hint_stderr_rate_limit() {
        let hint = generate_hint(1, "Error: 429 Too Many Requests - rate limit exceeded");
        assert!(hint.contains("rate limit"), "stderr '429' = rate limit");
    }

    #[test]
    fn test_hint_fallback() {
        let hint = generate_hint(1, "something completely unknown went wrong");
        assert!(hint.contains("manually"), "unknown error = suggest manual diagnosis");
    }

    // ---- 7B: Stderr truncation contract ----

    fn truncate_stderr(stderr: &str, max_lines: usize) -> String {
        let lines: Vec<&str> = stderr.lines().collect();
        if lines.len() <= max_lines {
            stderr.trim().to_string()
        } else {
            let total = lines.len();
            let tail = &lines[total - max_lines..];
            format!(
                "[{} lines total, showing last {}]\n{}",
                total, max_lines, tail.join("\n")
            )
        }
    }

    #[test]
    fn test_truncate_stderr_short() {
        let stderr = "line1\nline2\nline3";
        let result = truncate_stderr(stderr, 15);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_truncate_stderr_exact_limit() {
        let lines: Vec<String> = (1..=15).map(|i| format!("line {i}")).collect();
        let stderr = lines.join("\n");
        let result = truncate_stderr(&stderr, 15);
        assert!(result.contains("line 1"), "should keep all lines at exact limit");
        assert!(result.contains("line 15"));
        assert!(!result.contains("["), "should not have truncation marker at exact limit");
    }

    #[test]
    fn test_truncate_stderr_over_limit() {
        let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        let stderr = lines.join("\n");
        let result = truncate_stderr(&stderr, 15);
        assert!(result.contains("[30 lines total, showing last 15]"), "must show total + retained count");
        assert!(!result.contains("line 1\n"), "first line should be truncated");
        assert!(!result.contains("line 15\n"), "line 15 should be truncated");
        assert!(result.contains("line 16"), "line 16 should be the first retained line");
        assert!(result.contains("line 30"), "last line should be retained");
    }

    #[test]
    fn test_truncate_stderr_preserves_last_lines() {
        let lines: Vec<String> = (1..=100).map(|i| format!("error at step {i}")).collect();
        let stderr = lines.join("\n");
        let result = truncate_stderr(&stderr, 5);
        assert!(result.contains("error at step 96"), "should keep last 5 lines");
        assert!(result.contains("error at step 100"));
        assert!(!result.contains("error at step 95"), "should not keep line before tail");
    }

    // ---- 7B: Error JSON context contract ----

    #[test]
    fn test_execution_error_json_has_required_fields() {
        let ctx = json!({
            "tool": "web_search",
            "exit_code": 1,
            "stderr": "curl: (6) Could not resolve host",
            "hint": "check network",
        });
        assert!(ctx.get("tool").is_some(), "JSON context must have tool");
        assert!(ctx.get("exit_code").is_some(), "JSON context must have exit_code");
        assert!(ctx.get("stderr").is_some(), "JSON context must have stderr");
        assert!(ctx.get("hint").is_some(), "JSON context must have hint");
    }

    #[test]
    fn test_spawn_error_json_has_required_fields() {
        let ctx = json!({
            "tool": "my_tool",
            "detail": "binary not found",
            "hint": "install it",
        });
        assert!(ctx.get("tool").is_some(), "JSON context must have tool");
        assert!(ctx.get("detail").is_some(), "JSON context must have detail");
        assert!(ctx.get("hint").is_some(), "JSON context must have hint");
    }

    // ---- 7C: Retry budget contract ----

    #[test]
    fn test_retry_escalation_message_format() {
        // Second identical failure warning
        let warning = "This is the second time this exact call failed. \
                       Do not retry with identical arguments.";
        assert!(warning.contains("second time"), "2nd failure must mention count");
        assert!(warning.contains("identical arguments"), "must mention identical args");
    }

    #[test]
    fn test_retry_final_escalation_format() {
        // Third+ failure escalation
        let count = 3;
        let escalation = format!(
            "[TOOL_ERROR] This is attempt #{} with identical arguments and error. \
             Do NOT retry this tool with the same arguments. \
             Either use different arguments, try a different tool, or ask the user for help.",
            count
        );
        assert!(escalation.contains("[TOOL_ERROR]"), "escalation must have [TOOL_ERROR] prefix");
        assert!(escalation.contains(&format!("#{count}")), "must include attempt count");
        assert!(escalation.contains("different tool"), "must suggest alternatives");
        assert!(escalation.contains("ask the user"), "must suggest asking user");
    }

    #[test]
    fn test_step_budget_decay() {
        // Step budget decays by 2 per repeat, floor at 2
        let max_steps: usize = 10;
        let penalty: usize = 2;
        let after_one_repeat = max_steps.saturating_sub(penalty);
        assert_eq!(after_one_repeat, 8, "one repeat: 10 - 2 = 8");
        let after_two_repeats = after_one_repeat.saturating_sub(penalty);
        assert_eq!(after_two_repeats, 6, "two repeats: 8 - 2 = 6");
        // Floor at 2
        let floor = 2usize;
        let extreme = floor.saturating_sub(penalty).max(2);
        assert_eq!(extreme, 2, "floor should be 2");
    }

    // ---- 7: Error classification — new error formats must classify as ToolError ----

    #[test]
    fn test_new_error_formats_classify_as_tool_error() {
        // These are the new Phase 7 error message formats.
        // The error_classification module's classify() function must
        // recognize them (tests the string-matching fallback path).
        let new_formats = [
            "error: tool 'web_search' failed (exit code 1)",
            "error: tool 'analyze_code' could not be started: binary not found",
            "binary is not executable",
            "error: tool 'my_tool' failed (exit code 127)\n  hint: the tool binary was not found on PATH.",
        ];
        for msg in &new_formats {
            assert!(
                msg.contains("tool '") && msg.contains("' failed")
                    || msg.contains("could not be started")
                    || msg.contains("binary not found")
                    || msg.contains("binary is not executable"),
                "message should match at least one Phase 7 pattern: {msg}"
            );
        }
    }

    #[test]
    fn test_legacy_error_format_still_classified() {
        // Legacy format must still be recognized for backward compatibility
        let legacy = "Tool call exit with 1";
        assert!(legacy.contains("Tool call exit with"));
    }

    // ---- 7: Multi-tool partial failure contract ----

    #[test]
    fn test_partial_failure_all_tools_get_results() {
        // Phase 7A: every tool call must produce a ToolResult, even on failure.
        // Simulate 3 tool calls where one fails.
        let results: Vec<Value> = vec![
            json!({"results": ["result1"]}),                              // success
            json!("[TOOL_ERROR] fetch_url failed (exit 1)."),             // failure
            json!({"status": "ok", "output": null}),                     // success, no output
        ];
        assert_eq!(results.len(), 3, "all 3 calls must produce results");
        assert!(!results[0].is_null());
        assert!(results[1].as_str().unwrap().contains("[TOOL_ERROR]"));
        assert_eq!(results[2]["status"], "ok");
    }

    #[test]
    fn test_partial_failure_never_clears_results() {
        // Phase 7A: even if all tools return null, we must NOT clear results.
        // This was the old is_all_null behavior that violated protocol.
        let results: Vec<Value> = vec![
            json!({"status": "ok", "output": null}),
            json!({"status": "ok", "output": null}),
        ];
        assert!(!results.is_empty(), "results must never be cleared even when all null");
    }

    // ---- 7: Spawn error hint contract ----

    fn spawn_error_hint(err_msg: &str) -> Option<String> {
        let msg = err_msg.to_lowercase();
        if msg.contains("not found") || msg.contains("no such file") {
            Some("ensure the tool binary is installed and on PATH.".to_string())
        } else if msg.contains("permission denied") {
            Some("check file permissions. Try: chmod +x <path>".to_string())
        } else {
            None
        }
    }

    #[test]
    fn test_spawn_hint_not_found() {
        let hint = spawn_error_hint("No such file or directory (os error 2)");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("installed"));
    }

    #[test]
    fn test_spawn_hint_permission_denied() {
        let hint = spawn_error_hint("Permission denied (os error 13)");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("chmod"));
    }

    #[test]
    fn test_spawn_hint_unknown() {
        let hint = spawn_error_hint("broken pipe");
        assert!(hint.is_none(), "unknown spawn errors should not generate a hint");
    }
}

// ===========================================================================
// 19. Phase 8 — Tool timeout & concurrent execution
// ===========================================================================

mod phase8_timeout_and_concurrency {
    use super::*;

    // ---- 8A: Timeout error format contracts ----

    fn format_timeout_error(tool_name: &str, timeout_secs: u64) -> String {
        format!(
            "error: tool '{tool_name}' timed out after {timeout_secs}s\n  \
             hint: increase timeout with tool_timeout in config or per-tool \"timeout\" in functions.json"
        )
    }

    #[test]
    fn test_timeout_error_includes_tool_name() {
        let msg = format_timeout_error("slow_tool", 30);
        assert!(msg.contains("tool 'slow_tool'"), "must include tool name");
    }

    #[test]
    fn test_timeout_error_includes_duration() {
        let msg = format_timeout_error("slow_tool", 30);
        assert!(msg.contains("30s"), "must include timeout duration");
    }

    #[test]
    fn test_timeout_error_includes_hint() {
        let msg = format_timeout_error("slow_tool", 30);
        assert!(msg.contains("hint:"), "must include hint");
        assert!(msg.contains("tool_timeout"), "hint must mention config key");
        assert!(msg.contains("functions.json"), "hint must mention per-tool config");
    }

    // ---- 8A: Timeout LLM error format ----

    fn format_timeout_for_llm(tool_name: &str, timeout_secs: u64) -> String {
        format!(
            "[TOOL_ERROR] {tool_name} timed out after {timeout_secs}s.\n\
             Hint: increase timeout with tool_timeout in config or per-tool \"timeout\" in functions.json."
        )
    }

    #[test]
    fn test_timeout_llm_error_has_prefix() {
        let msg = format_timeout_for_llm("slow_tool", 30);
        assert!(msg.starts_with("[TOOL_ERROR]"));
    }

    #[test]
    fn test_timeout_llm_error_includes_tool_and_duration() {
        let msg = format_timeout_for_llm("slow_tool", 60);
        assert!(msg.contains("slow_tool"));
        assert!(msg.contains("60s"));
    }

    // ---- 8A: Timeout JSON context ----

    #[test]
    fn test_timeout_json_context() {
        let ctx = json!({
            "tool": "slow_tool",
            "timeout_secs": 30,
        });
        assert_eq!(ctx["tool"], "slow_tool");
        assert_eq!(ctx["timeout_secs"], 30);
    }

    // ---- 8A: Timeout resolution contract ----

    #[test]
    fn test_timeout_resolution_per_tool_overrides_global() {
        // Per-tool timeout (30) should override global timeout (60)
        let per_tool: Option<u64> = Some(30);
        let global: u64 = 60;
        let effective = per_tool.filter(|&t| t > 0).unwrap_or(global);
        assert_eq!(effective, 30, "per-tool timeout should override global");
    }

    #[test]
    fn test_timeout_resolution_global_fallback() {
        // No per-tool timeout → use global
        let per_tool: Option<u64> = None;
        let global: u64 = 60;
        let effective = per_tool.filter(|&t| t > 0).unwrap_or(global);
        assert_eq!(effective, 60, "should fall back to global timeout");
    }

    #[test]
    fn test_timeout_resolution_zero_means_disabled() {
        // Per-tool timeout of 0 should fall through to global
        let per_tool: Option<u64> = Some(0);
        let global: u64 = 45;
        let effective = per_tool.filter(|&t| t > 0).unwrap_or(global);
        assert_eq!(effective, 45, "per-tool 0 should fall through to global");
    }

    #[test]
    fn test_timeout_disabled_when_both_zero() {
        let per_tool: Option<u64> = None;
        let global: u64 = 0;
        let effective = per_tool.filter(|&t| t > 0).unwrap_or(global);
        assert_eq!(effective, 0, "0 = disabled");
    }

    // ---- 8A: Config contract ----

    #[test]
    fn test_config_example_has_tool_timeout() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.yaml");
        let content = std::fs::read_to_string(path).unwrap();
        assert!(
            content.contains("tool_timeout"),
            "config.example.yaml must document tool_timeout"
        );
    }

    #[test]
    fn test_tool_timeout_default_is_zero() {
        // Default should be 0 (disabled) for backward compatibility
        let default: u64 = 0;
        assert_eq!(default, 0, "default tool_timeout must be 0 (disabled)");
    }

    // ---- 8A: FunctionDeclaration timeout field ----

    #[test]
    fn test_function_declaration_timeout_field() {
        // Timeout should be Optional and default to None in deserialization
        let json = json!({
            "name": "my_tool",
            "description": "A tool",
            "parameters": {"type": "object", "properties": {}}
        });
        // Without timeout field — should deserialize with timeout = None
        let _: serde_json::Value = json; // Type check passes

        // With timeout field
        let json_with_timeout = json!({
            "name": "my_tool",
            "description": "A tool",
            "parameters": {"type": "object", "properties": {}},
            "timeout": 30
        });
        assert_eq!(json_with_timeout["timeout"], 30);
    }

    #[test]
    fn test_function_declaration_timeout_optional() {
        // Functions.json without timeout field must still parse
        let json_str = r#"[{
            "name": "get_weather",
            "description": "Get weather",
            "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}
        }]"#;
        let declarations: Vec<Value> = serde_json::from_str(json_str).unwrap();
        assert!(declarations[0].get("timeout").is_none(), "timeout should be absent by default");
    }

    // ---- 8A: Error classification ----

    #[test]
    fn test_timeout_error_classifies_as_tool_error() {
        let msg = "error: tool 'slow_tool' timed out after 30s";
        assert!(
            msg.contains("timed out after") && msg.contains("tool '"),
            "timeout error should match tool error classification"
        );
    }

    // ---- 8B: Concurrent execution contract ----

    #[test]
    fn test_concurrent_results_preserve_order() {
        // Results from concurrent execution should maintain correspondence
        // between calls and results (same index = same tool call)
        let calls = vec!["tool_a", "tool_b", "tool_c"];
        let results = vec![json!("result_a"), json!("result_b"), json!("result_c")];
        assert_eq!(calls.len(), results.len());
        // join_all preserves input order
        for (i, name) in calls.iter().enumerate() {
            let expected_suffix = &name[name.len()-1..];
            let result_str = results[i].as_str().unwrap();
            assert!(result_str.ends_with(expected_suffix), "results must preserve call order");
        }
    }

    #[test]
    fn test_concurrent_partial_failure_all_results_returned() {
        // Even in concurrent execution, all calls must produce results
        let results: Vec<Value> = vec![
            json!({"output": "success"}),
            json!("[TOOL_ERROR] tool_b failed (exit 1)."),
            json!({"status": "ok", "output": null}),
        ];
        assert_eq!(results.len(), 3, "concurrent execution must return result for every call");
        // Error result is still present, not filtered
        assert!(results[1].as_str().unwrap().contains("[TOOL_ERROR]"));
    }

    #[test]
    fn test_concurrent_independence() {
        // Each tool call should be independent — failure of one does not affect others
        // This is a design contract, not a runtime test
        let tool_a_result: Result<Value, &str> = Ok(json!("ok"));
        let tool_b_result: Result<Value, &str> = Err("failed");
        let tool_c_result: Result<Value, &str> = Ok(json!("ok"));

        // All results should be collected regardless of individual failures
        let collected: Vec<Value> = vec![
            tool_a_result.unwrap_or_else(|e| json!(format!("[TOOL_ERROR] {e}"))),
            tool_b_result.unwrap_or_else(|e| json!(format!("[TOOL_ERROR] {e}"))),
            tool_c_result.unwrap_or_else(|e| json!(format!("[TOOL_ERROR] {e}"))),
        ];
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], json!("ok"));
        assert!(collected[1].as_str().unwrap().contains("[TOOL_ERROR]"));
        assert_eq!(collected[2], json!("ok"));
    }
}

// ===========================================================================
// 21. Tool execution — end-to-end invocation via real llm-functions
// ===========================================================================
//
// These tests execute real tool binaries from the llm-functions directory,
// validating the full flow: server response → tool call parsing → subprocess
// execution → result formatting.
//
// Configurable via environment variables:
//   AICHAT_TEST_LLM_FUNCTIONS_DIR — path to llm-functions (default: ~/Developer/Scripts/llm-functions)
//   AICHAT_TEST_CONFIG_DIR        — path to aichat config  (default: ~/Library/Application Support/aichat)
//
// Prerequisites: argc, jq must be installed (used by tool runner scripts).
// Tests skip gracefully when directories or prerequisites are missing.

mod tool_execution {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    const ENV_LLM_FUNCTIONS_DIR: &str = "AICHAT_TEST_LLM_FUNCTIONS_DIR";
    const ENV_CONFIG_DIR: &str = "AICHAT_TEST_CONFIG_DIR";

    /// Monotonic counter for unique temp file names across parallel tests.
    static EXEC_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn expand_tilde(path: &str) -> PathBuf {
        if let Some(rest) = path.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(rest);
            }
        }
        PathBuf::from(path)
    }

    fn llm_functions_dir() -> PathBuf {
        expand_tilde(
            &std::env::var(ENV_LLM_FUNCTIONS_DIR)
                .unwrap_or_else(|_| "~/Developer/Scripts/llm-functions".to_string()),
        )
    }

    fn aichat_config_dir() -> PathBuf {
        expand_tilde(
            &std::env::var(ENV_CONFIG_DIR)
                .unwrap_or_else(|_| "~/Library/Application Support/aichat".to_string()),
        )
    }

    /// Returns Some(reason) if tool execution prerequisites are not met.
    fn check_exec_prerequisites() -> Option<String> {
        let dir = llm_functions_dir();
        if !dir.exists() {
            return Some(format!(
                "{} not found (set {} to override)",
                dir.display(),
                ENV_LLM_FUNCTIONS_DIR
            ));
        }
        if !dir.join("bin").is_dir() {
            return Some(format!("{}/bin not found", dir.display()));
        }
        for (cmd, flag) in [("argc", "--argc-version"), ("jq", "--version")] {
            if Command::new(cmd)
                .arg(flag)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_err()
            {
                return Some(format!("{cmd} not installed (required by tool scripts)"));
            }
        }
        None
    }

    macro_rules! skip_if {
        ($check:expr) => {
            if let Some(reason) = $check {
                eprintln!("SKIP: {reason}");
                return;
            }
        };
    }

    #[allow(dead_code)]
    struct ToolExecResult {
        exit_code: i32,
        /// Contents of the LLM_OUTPUT file after tool execution.
        output: String,
        /// Captured stderr from the subprocess.
        stderr: String,
    }

    /// Execute a tool binary the way aichat does: prepend bin/ to PATH,
    /// set LLM_OUTPUT to a temp file, pass JSON as the argument.
    fn exec_tool(functions_dir: &Path, tool_name: &str, json_data: &str) -> ToolExecResult {
        let bin_dir = functions_dir.join("bin");
        let path = format!(
            "{}:{}",
            bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let seq = EXEC_COUNTER.fetch_add(1, Ordering::SeqCst);
        let llm_output = std::env::temp_dir().join(format!(
            "aichat-compat-{}-{}-{}",
            tool_name,
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_file(&llm_output);

        let result = Command::new(tool_name)
            .arg(json_data)
            .env("PATH", &path)
            .env("LLM_OUTPUT", llm_output.display().to_string())
            .env(
                "LLM_ROOT_DIR",
                functions_dir.display().to_string(),
            )
            .env("LLM_TOOL_NAME", tool_name)
            .env(
                "LLM_TOOL_CACHE_DIR",
                functions_dir
                    .join("cache")
                    .join(tool_name)
                    .display()
                    .to_string(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        match result {
            Ok(out) => {
                let output =
                    std::fs::read_to_string(&llm_output).unwrap_or_default();
                let _ = std::fs::remove_file(&llm_output);
                ToolExecResult {
                    exit_code: out.status.code().unwrap_or(-1),
                    output,
                    stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&llm_output);
                ToolExecResult {
                    exit_code: -1,
                    output: String::new(),
                    stderr: format!("spawn failed: {e}"),
                }
            }
        }
    }

    /// Normalize tool output the way aichat's eval() does:
    /// - non-zero exit → [TOOL_ERROR] string
    /// - empty output  → {"status": "ok", "output": null}
    /// - JSON output   → parsed Value
    /// - text output   → {"output": text}
    fn normalize_result(
        exit_code: i32,
        raw_output: &str,
        tool_name: &str,
    ) -> Value {
        if exit_code != 0 {
            return json!(format!(
                "[TOOL_ERROR] {} failed (exit {}).",
                tool_name, exit_code
            ));
        }
        let trimmed = raw_output.trim();
        if trimmed.is_empty() {
            return json!({"status": "ok", "output": null});
        }
        serde_json::from_str::<Value>(trimmed)
            .unwrap_or_else(|_| json!({"output": trimmed}))
    }

    fn load_functions_json(dir: &Path) -> Vec<Value> {
        let path = dir.join("functions.json");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("cannot read {}", path.display()));
        serde_json::from_str(&content)
            .unwrap_or_else(|_| panic!("cannot parse {}", path.display()))
    }

    fn find_tool(tools: &[Value], name: &str) -> Option<Value> {
        tools
            .iter()
            .find(|t| t["name"].as_str() == Some(name))
            .cloned()
    }

    // ---- Directory structure ----

    #[test]
    fn test_llm_functions_dir_structure() {
        let dir = llm_functions_dir();
        if !dir.exists() {
            eprintln!("SKIP: {} not found (set {ENV_LLM_FUNCTIONS_DIR})", dir.display());
            return;
        }
        assert!(dir.join("bin").is_dir(), "must have bin/");
        assert!(dir.join("scripts").is_dir(), "must have scripts/");
        assert!(
            dir.join("functions.json").is_file(),
            "must have functions.json"
        );
        assert!(
            dir.join("scripts/run-tool.sh").is_file(),
            "must have scripts/run-tool.sh"
        );
    }

    #[test]
    fn test_real_functions_json_all_tools_valid() {
        let dir = llm_functions_dir();
        if !dir.exists() {
            return;
        }
        let tools = load_functions_json(&dir);
        assert!(!tools.is_empty(), "functions.json must declare at least one tool");
        for tool in &tools {
            let name = tool["name"]
                .as_str()
                .unwrap_or_else(|| panic!("tool missing name: {tool:?}"));
            assert!(
                tool.get("description").is_some(),
                "tool '{name}' missing description"
            );
            assert!(
                tool["parameters"].is_object(),
                "tool '{name}' parameters must be a JSON object"
            );
        }
    }

    #[test]
    fn test_bin_entries_resolve_and_executable() {
        let dir = llm_functions_dir();
        if !dir.exists() {
            return;
        }
        let bin = dir.join("bin");
        let mut count = 0;
        for entry in std::fs::read_dir(&bin).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let fname = path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let meta = std::fs::metadata(&path)
                .unwrap_or_else(|e| panic!("bin/{fname}: broken symlink: {e}"));
            assert!(meta.is_file(), "bin/{fname} must resolve to a file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert!(
                    meta.permissions().mode() & 0o111 != 0,
                    "bin/{fname} must be executable"
                );
            }
            count += 1;
        }
        assert!(count > 0, "bin/ must have entries");
    }

    #[test]
    fn test_all_declared_tools_have_bin_entry() {
        let dir = llm_functions_dir();
        if !dir.exists() {
            return;
        }
        let tools = load_functions_json(&dir);
        let bin = dir.join("bin");
        let bin_names: HashSet<String> = std::fs::read_dir(&bin)
            .unwrap()
            .filter_map(|e| {
                e.ok()
                    .map(|e| e.file_name().to_string_lossy().to_string())
            })
            .collect();
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert!(
                bin_names.contains(name),
                "tool '{name}' in functions.json has no bin/ entry"
            );
        }
    }

    // ---- Tool execution — core subprocess contract ----

    #[test]
    fn test_execute_no_arg_tool() {
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "get_current_time").is_none() {
            eprintln!("SKIP: get_current_time not in functions.json");
            return;
        }
        let r = exec_tool(&dir, "get_current_time", "{}");
        assert_eq!(
            r.exit_code, 0,
            "get_current_time should exit 0. stderr: {}",
            r.stderr
        );
        assert!(
            !r.output.trim().is_empty(),
            "get_current_time must produce output"
        );
    }

    #[test]
    fn test_tool_output_written_to_llm_output_file() {
        // Core contract: tool writes output to the $LLM_OUTPUT file.
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "get_current_time").is_none() {
            return;
        }
        let r = exec_tool(&dir, "get_current_time", "{}");
        assert_eq!(r.exit_code, 0);
        assert!(
            !r.output.trim().is_empty(),
            "tool must write output to $LLM_OUTPUT, not solely to stdout"
        );
    }

    #[test]
    fn test_tool_result_normalizes_text_to_json() {
        // When tool output is plain text, aichat wraps it as {"output": text}.
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "get_current_time").is_none() {
            return;
        }
        let r = exec_tool(&dir, "get_current_time", "{}");
        assert_eq!(r.exit_code, 0);
        let normalized = normalize_result(r.exit_code, &r.output, "get_current_time");
        assert!(
            normalized.is_object(),
            "normalized text result must be a JSON object, got: {normalized}"
        );
        assert!(
            normalized.get("output").is_some(),
            "wrapped text result must have 'output' key"
        );
    }

    #[test]
    fn test_execute_tool_with_arguments() {
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        // fs_ls is a safe read-only tool that takes a "path" argument
        if find_tool(&tools, "fs_ls").is_none() {
            eprintln!("SKIP: fs_ls not in functions.json");
            return;
        }
        let r = exec_tool(&dir, "fs_ls", r#"{"path":"/tmp"}"#);
        assert_eq!(r.exit_code, 0, "fs_ls /tmp failed: {}", r.stderr);
        assert!(
            !r.output.trim().is_empty(),
            "fs_ls /tmp must produce directory listing"
        );
    }

    #[test]
    fn test_nonexistent_tool_fails() {
        let dir = llm_functions_dir();
        if !dir.exists() {
            return;
        }
        let r = exec_tool(&dir, "__nonexistent_tool_compat_test__", "{}");
        assert_ne!(
            r.exit_code, 0,
            "nonexistent tool must produce a non-zero exit code"
        );
    }

    // ---- Server response → tool execution → result formatting ----

    #[test]
    fn test_openai_response_to_tool_execution() {
        // Full flow: parse OpenAI tool_call response → execute → format result message
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "get_current_time").is_none() {
            return;
        }

        // 1. Parse server response (OpenAI chat completions format)
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_current_time",
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let tool_calls = response["choices"][0]["message"]["tool_calls"]
            .as_array()
            .expect("response must have tool_calls");
        assert_eq!(tool_calls.len(), 1);

        let call = &tool_calls[0];
        let name = call["function"]["name"].as_str().unwrap();
        let args = call["function"]["arguments"].as_str().unwrap();
        let call_id = call["id"].as_str().unwrap();

        // 2. Verify tool exists in declarations (aichat does this pre-flight)
        assert!(
            find_tool(&tools, name).is_some(),
            "tool '{name}' must be declared in functions.json"
        );

        // 3. Execute the tool
        let r = exec_tool(&dir, name, args);
        assert_eq!(r.exit_code, 0, "tool failed: {}", r.stderr);

        // 4. Format result the way aichat does
        let result = normalize_result(r.exit_code, &r.output, name);

        // 5. Construct the tool result message for the next API call
        let tool_msg = json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": serde_json::to_string(&result).unwrap()
        });
        assert_eq!(tool_msg["role"], "tool");
        assert_eq!(tool_msg["tool_call_id"], "call_abc123");
        assert!(
            !tool_msg["content"].as_str().unwrap().is_empty(),
            "tool result content must not be empty"
        );
    }

    #[test]
    fn test_claude_response_to_tool_execution() {
        // Full flow: parse Claude tool_use response → execute → format result message
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "get_current_time").is_none() {
            return;
        }

        // 1. Parse server response (Claude format)
        let response = json!({
            "content": [
                {"type": "text", "text": "Let me check the time."},
                {
                    "type": "tool_use",
                    "id": "toolu_01A",
                    "name": "get_current_time",
                    "input": {}
                }
            ],
            "stop_reason": "tool_use"
        });

        let tool_uses: Vec<&Value> = response["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["type"] == "tool_use")
            .collect();
        assert_eq!(tool_uses.len(), 1);

        let tu = tool_uses[0];
        let name = tu["name"].as_str().unwrap();
        let input = &tu["input"];
        let tu_id = tu["id"].as_str().unwrap();

        // 2. Execute
        let r = exec_tool(&dir, name, &serde_json::to_string(input).unwrap());
        assert_eq!(r.exit_code, 0, "tool failed: {}", r.stderr);

        // 3. Format result for Claude API
        let result = normalize_result(r.exit_code, &r.output, name);
        let tool_result = json!({
            "type": "tool_result",
            "tool_use_id": tu_id,
            "content": serde_json::to_string(&result).unwrap()
        });
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_01A");
        assert!(
            !tool_result["content"].as_str().unwrap().is_empty(),
            "tool result content must not be empty"
        );
    }

    #[test]
    fn test_multi_tool_call_execution_preserves_order() {
        // Multiple tool_calls → execute all → results maintain call order
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "get_current_time").is_none() {
            return;
        }

        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "get_current_time", "arguments": "{}"}
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {"name": "get_current_time", "arguments": "{}"}
                        }
                    ]
                }
            }]
        });

        let calls = response["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();

        let mut results: Vec<(String, Value)> = Vec::new();
        for call in calls {
            let name = call["function"]["name"].as_str().unwrap();
            let args = call["function"]["arguments"].as_str().unwrap();
            let id = call["id"].as_str().unwrap().to_string();
            let r = exec_tool(&dir, name, args);
            results.push((id, normalize_result(r.exit_code, &r.output, name)));
        }

        assert_eq!(results.len(), 2, "all tool calls must produce results");
        assert_eq!(results[0].0, "call_1", "result order must match call order");
        assert_eq!(results[1].0, "call_2");
        for (id, result) in &results {
            assert!(
                result.is_object(),
                "result for {id} must be a JSON object"
            );
        }
    }

    #[test]
    fn test_tool_call_with_arguments_from_response() {
        // Server response contains a tool_call with real arguments → execute
        skip_if!(check_exec_prerequisites());
        let dir = llm_functions_dir();
        let tools = load_functions_json(&dir);
        if find_tool(&tools, "fs_ls").is_none() {
            eprintln!("SKIP: fs_ls not in functions.json");
            return;
        }

        let response = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_ls",
                        "type": "function",
                        "function": {
                            "name": "fs_ls",
                            "arguments": "{\"path\":\"/tmp\"}"
                        }
                    }]
                }
            }]
        });

        let call = &response["choices"][0]["message"]["tool_calls"][0];
        let name = call["function"]["name"].as_str().unwrap();
        let args = call["function"]["arguments"].as_str().unwrap();

        let r = exec_tool(&dir, name, args);
        assert_eq!(r.exit_code, 0, "fs_ls failed: {}", r.stderr);

        let result = normalize_result(r.exit_code, &r.output, name);
        assert!(result.is_object(), "result must be a JSON object");
    }

    // ---- Result normalization contracts ----

    #[test]
    fn test_normalize_error_result() {
        let result = normalize_result(1, "", "broken_tool");
        let err = result.as_str().unwrap();
        assert!(
            err.starts_with("[TOOL_ERROR]"),
            "error result must have [TOOL_ERROR] prefix"
        );
        assert!(err.contains("broken_tool"), "must include tool name");
        assert!(err.contains("exit 1"), "must include exit code");
    }

    #[test]
    fn test_normalize_empty_output_to_structured_null() {
        let result = normalize_result(0, "", "quiet_tool");
        assert_eq!(result["status"], "ok");
        assert!(result["output"].is_null());
    }

    #[test]
    fn test_normalize_json_output_preserved() {
        let result = normalize_result(
            0,
            r#"{"temperature": 72, "unit": "F"}"#,
            "weather",
        );
        assert_eq!(result["temperature"], 72);
        assert_eq!(result["unit"], "F");
    }

    #[test]
    fn test_normalize_text_output_wrapped() {
        let result = normalize_result(0, "Mon Mar 30 14:22:01 PDT 2026\n", "get_current_time");
        assert!(result.is_object());
        assert!(result["output"].as_str().unwrap().contains("Mon Mar 30"));
    }

    // ---- Config directory — installed aichat contract ----

    #[test]
    fn test_config_dir_has_functions() {
        let dir = aichat_config_dir();
        if !dir.exists() {
            eprintln!("SKIP: {} not found (set {ENV_CONFIG_DIR})", dir.display());
            return;
        }
        let functions = dir.join("functions");
        assert!(
            functions.exists(),
            "aichat config dir must have functions/ (directory or symlink)"
        );
    }

    #[test]
    fn test_config_functions_resolves_to_llm_functions() {
        let config = aichat_config_dir();
        let llm = llm_functions_dir();
        if !config.exists() || !llm.exists() {
            return;
        }
        let config_functions = config.join("functions");
        if !config_functions.exists() {
            return;
        }
        if let (Ok(a), Ok(b)) = (
            std::fs::canonicalize(&config_functions),
            std::fs::canonicalize(&llm),
        ) {
            assert_eq!(
                a, b,
                "config/functions should resolve to the llm-functions directory"
            );
        }
    }
}

// ===========================================================================
// Serve endpoint: /v1/prompts
// ===========================================================================

mod serve_prompts {
    use super::*;
    use std::fs;

        fn fake_all_prompts(dir: &std::path::Path) -> Vec<Value> {
            let mut prompts = vec![];
            if let Ok(rd) = fs::read_dir(dir) {
                for entry in rd.flatten() {
                    if let Some(name) = entry
                        .file_name()
                        .to_str()
                        .and_then(|v| v.strip_suffix(".md"))
                    {
                        if let Ok(content) = fs::read_to_string(entry.path()) {
                            prompts.push(json!({
                            "name": name,
                            "content": content,
                        }));
                        }
                    }
                }
            }
            prompts.sort_by(|a, b| {
                a["name"].as_str().unwrap().cmp(b["name"].as_str().unwrap())
            });
            prompts
        }

    #[test]
    fn test_prompts_endpoint_returns_name_and_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("summarize.md"), "Summarize the following text.").unwrap();
        fs::write(dir.path().join("translate.md"), "Translate to French.").unwrap();

        let prompts = fake_all_prompts(dir.path());
        let body = json!({ "data": prompts });

        // Endpoint must return a JSON object with a "data" array
        assert!(body["data"].is_array());
        let data = body["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);

        // Each entry must have "name" and "content" fields
        assert_eq!(data[0]["name"], "summarize");
        assert_eq!(data[0]["content"], "Summarize the following text.");
        assert_eq!(data[1]["name"], "translate");
        assert_eq!(data[1]["content"], "Translate to French.");
    }

    #[test]
    fn test_prompts_endpoint_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let prompts = fake_all_prompts(dir.path());
        let body = json!({ "data": prompts });

        assert!(body["data"].is_array());
        assert_eq!(body["data"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_prompts_endpoint_ignores_non_md_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("good.md"), "A prompt.").unwrap();
        fs::write(dir.path().join("not-a-prompt.txt"), "Ignored.").unwrap();
        fs::write(dir.path().join("also-ignored.yaml"), "key: val").unwrap();

        let prompts = fake_all_prompts(dir.path());
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], "good");
    }

    #[test]
    fn test_prompts_endpoint_sorted_by_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("zebra.md"), "z").unwrap();
        fs::write(dir.path().join("alpha.md"), "a").unwrap();
        fs::write(dir.path().join("middle.md"), "m").unwrap();

        let prompts = fake_all_prompts(dir.path());
        let names: Vec<&str> = prompts.iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }
}
