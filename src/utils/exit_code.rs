/// Semantic exit codes for aichat.
///
/// These codes allow scripts and pipelines to react to specific failure
/// categories without parsing stderr.
///
/// | Code | Meaning                  |
/// |------|--------------------------|
/// |  0   | Success                  |
/// |  1   | General / unknown error  |
/// |  2   | Usage / invalid input    |
/// |  3   | Config / role not found  |
/// |  4   | Auth / API key error     |
/// |  5   | Network / connection     |
/// |  6   | API response error       |
/// |  7   | Model error              |
/// |  8   | Schema validation failed |
/// |  9   | Aborted by user          |
/// | 10   | Tool / function error    |

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
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

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn category_name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::GeneralError => "general",
            Self::UsageError => "usage",
            Self::ConfigError => "config",
            Self::AuthError => "auth",
            Self::NetworkError => "network",
            Self::ApiError => "api",
            Self::ModelError => "model",
            Self::SchemaError => "schema_validation",
            Self::Aborted => "aborted",
            Self::ToolError => "tool",
        }
    }
}

// ---------------------------------------------------------------------------
// Structured error types — used at error boundaries for machine-readable detail
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AichatError {
    SchemaValidation {
        direction: String,
        message: String,
    },
    ConfigParse {
        message: String,
        field: Option<String>,
    },
    ToolNotFound {
        name: String,
    },
    ToolSpawnError {
        tool_name: String,
        message: String,
        hint: Option<String>,
    },
    ToolExecutionError {
        tool_name: String,
        exit_code: i32,
        stderr: Option<String>,
        hint: Option<String>,
    },
    ToolTimeout {
        tool_name: String,
        timeout_secs: u64,
    },
    PipelineStage {
        stage: usize,
        total: usize,
        role_name: String,
        model_id: Option<String>,
        message: String,
    },
    McpError {
        message: String,
        server: Option<String>,
        tool: Option<String>,
    },
}

impl std::fmt::Display for AichatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaValidation { direction, message } => {
                write!(f, "Schema {direction} validation failed: {message}")
            }
            Self::ConfigParse { message, field } => match field {
                Some(field) => write!(f, "Config parse error in '{field}': {message}"),
                None => write!(f, "Config parse error: {message}"),
            },
            Self::ToolNotFound { name } => write!(f, "Tool not found: {name}"),
            Self::ToolSpawnError {
                tool_name,
                message,
                hint,
            } => {
                write!(f, "error: tool '{tool_name}' could not be started: {message}")?;
                if let Some(hint) = hint {
                    write!(f, "\n  hint: {hint}")?;
                }
                Ok(())
            }
            Self::ToolExecutionError {
                tool_name,
                exit_code,
                stderr,
                hint,
            } => {
                write!(f, "error: tool '{tool_name}' failed (exit code {exit_code})")?;
                if let Some(stderr) = stderr {
                    if !stderr.is_empty() {
                        write!(f, "\n  stderr: {stderr}")?;
                    }
                }
                if let Some(hint) = hint {
                    write!(f, "\n  hint: {hint}")?;
                }
                Ok(())
            }
            Self::ToolTimeout {
                tool_name,
                timeout_secs,
            } => {
                write!(
                    f,
                    "error: tool '{tool_name}' timed out after {timeout_secs}s\n  \
                     hint: increase timeout with tool_timeout in config or per-tool \"timeout\" in functions.json"
                )
            }
            Self::PipelineStage {
                stage,
                total,
                role_name,
                model_id,
                message,
            } => {
                write!(f, "Pipeline stage {stage}/{total} (role '{role_name}'")?;
                if let Some(model) = model_id {
                    write!(f, ", model '{model}'")?;
                }
                write!(f, ") failed: {message}")
            }
            Self::McpError {
                message,
                server,
                tool,
            } => {
                write!(f, "MCP error")?;
                if let Some(s) = server {
                    write!(f, " [{s}")?;
                    if let Some(t) = tool {
                        write!(f, ":{t}")?;
                    }
                    write!(f, "]")?;
                }
                write!(f, ": {message}")
            }
        }
    }
}

impl std::error::Error for AichatError {}

impl AichatError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::SchemaValidation { .. } => ExitCode::SchemaError,
            Self::ConfigParse { .. } => ExitCode::ConfigError,
            Self::ToolNotFound { .. } => ExitCode::ToolError,
            Self::ToolSpawnError { .. } => ExitCode::ToolError,
            Self::ToolExecutionError { .. } => ExitCode::ToolError,
            Self::ToolTimeout { .. } => ExitCode::ToolError,
            Self::PipelineStage { .. } => ExitCode::ToolError,
            Self::McpError { .. } => ExitCode::ToolError,
        }
    }

    pub fn to_json_context(&self) -> serde_json::Value {
        match self {
            Self::SchemaValidation { direction, message } => {
                serde_json::json!({ "direction": direction, "detail": message })
            }
            Self::ConfigParse { message, field } => {
                serde_json::json!({ "detail": message, "field": field })
            }
            Self::ToolNotFound { name } => {
                serde_json::json!({ "tool": name })
            }
            Self::ToolSpawnError {
                tool_name,
                message,
                hint,
            } => {
                serde_json::json!({
                    "tool": tool_name,
                    "detail": message,
                    "hint": hint,
                })
            }
            Self::ToolExecutionError {
                tool_name,
                exit_code,
                stderr,
                hint,
            } => {
                serde_json::json!({
                    "tool": tool_name,
                    "exit_code": exit_code,
                    "stderr": stderr,
                    "hint": hint,
                })
            }
            Self::ToolTimeout {
                tool_name,
                timeout_secs,
            } => {
                serde_json::json!({
                    "tool": tool_name,
                    "timeout_secs": timeout_secs,
                })
            }
            Self::PipelineStage {
                stage,
                total,
                role_name,
                model_id,
                message,
            } => {
                serde_json::json!({
                    "stage": stage,
                    "total": total,
                    "role": role_name,
                    "model": model_id,
                    "detail": message,
                })
            }
            Self::McpError {
                message,
                server,
                tool,
            } => {
                serde_json::json!({ "detail": message, "server": server, "tool": tool })
            }
        }
    }
}

/// Inspect an `anyhow::Error` chain and return the most specific exit code.
pub fn classify_error(err: &anyhow::Error) -> ExitCode {
    // Fast path: check for typed AichatError first
    if let Some(typed) = err.downcast_ref::<AichatError>() {
        return typed.exit_code();
    }

    // Walk the entire error chain so context wrappers don't hide the cause.
    for cause in err.chain() {
        let msg = cause.to_string();

        if is_aborted(&msg) {
            return ExitCode::Aborted;
        }
        if is_schema_error(&msg) {
            return ExitCode::SchemaError;
        }
        if is_auth_error(&msg) {
            return ExitCode::AuthError;
        }
        if is_model_error(&msg) {
            return ExitCode::ModelError;
        }
        if is_tool_error(&msg) {
            return ExitCode::ToolError;
        }
        if is_network_error(&msg) {
            return ExitCode::NetworkError;
        }
        if is_api_error(&msg) {
            return ExitCode::ApiError;
        }
        if is_config_error(&msg) {
            return ExitCode::ConfigError;
        }
        if is_usage_error(&msg) {
            return ExitCode::UsageError;
        }
    }

    ExitCode::GeneralError
}

fn is_aborted(msg: &str) -> bool {
    msg.starts_with("Aborted")
}

fn is_schema_error(msg: &str) -> bool {
    msg.contains("Schema input validation failed")
        || msg.contains("Schema output validation failed")
        || msg.contains("Invalid input schema")
        || msg.contains("Invalid output schema")
}

fn is_auth_error(msg: &str) -> bool {
    msg.contains("(status: 401)")
        || msg.contains("(status: 403)")
        || msg.contains("api_key")
        || msg.contains("API key")
        || msg.contains("Unauthorized")
        || msg.contains("Access denied")
}

fn is_model_error(msg: &str) -> bool {
    msg.contains("Unknown model")
        || msg.contains("Unknown chat model")
        || msg.contains("Unknown embedding model")
        || msg.contains("Unknown rerank model")
        || msg.contains("No available model")
        || msg.contains("No models")
        || msg.contains("is not a chat model")
        || msg.contains("is not a embedding model")
        || msg.contains("is not a rerank model")
        || msg.contains("Exceed max_input_tokens limit")
        || msg.contains("does not support")
}

fn is_tool_error(msg: &str) -> bool {
    msg.contains("Tool call exit with")
        || msg.contains("Unexpected call:")
        || msg.contains("infinite loop of function calls")
        || msg.contains("ReAct loop exceeded")
        || msg.contains("Failed to load functions")
        || msg.contains("no functions are installed")
        // Phase 7: new error message formats
        || msg.contains("tool '") && msg.contains("' failed")
        || msg.contains("could not be started")
        || msg.contains("binary not found")
        || msg.contains("binary is not executable")
        || msg.contains("timed out after") && msg.contains("tool '")
}

fn is_network_error(msg: &str) -> bool {
    msg.contains("Failed to build client")
        || msg.contains("connection")
        || msg.contains("timed out")
        || msg.contains("dns error")
        || msg.contains("resolve host")
}

fn is_api_error(msg: &str) -> bool {
    msg.contains("(status: 4")
        || msg.contains("(status: 5")
        || msg.contains("Invalid response data")
        || msg.contains("Invalid response event-stream")
        || msg.contains("Blocked due to safety")
        || msg.contains("Failed to reader stream")
}

fn is_config_error(msg: &str) -> bool {
    msg.contains("Unknown role")
        || msg.contains("Unknown agent")
        || msg.contains("Unknown RAG")
        || msg.contains("No role")
        || msg.contains("No agent")
        || msg.contains("No RAG")
        || msg.contains("No session")
        || msg.contains("No macro")
        || msg.contains("Failed to load config")
        || msg.contains("Failed to load session")
        || msg.contains("Failed to load macro")
        || msg.contains("Circular role inheritance")
}

fn is_usage_error(msg: &str) -> bool {
    msg.starts_with("No input")
        || msg.starts_with("No command generated")
        || msg.starts_with("No TTY for REPL")
        || msg.contains("Invalid stdin pipe")
        || msg.contains("Usage:")
        || msg.contains("Unknown command")
        || msg.contains("Unknown key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_classify_schema_error() {
        let err = anyhow!("Schema input validation failed: not valid JSON");
        assert_eq!(classify_error(&err), ExitCode::SchemaError);
    }

    #[test]
    fn test_classify_schema_output_error() {
        let err = anyhow!("Schema output validation failed:\n  - missing field `name`");
        assert_eq!(classify_error(&err), ExitCode::SchemaError);
    }

    #[test]
    fn test_classify_auth_error() {
        let err = anyhow!("Invalid response data: {{}} (status: 401)");
        assert_eq!(classify_error(&err), ExitCode::AuthError);
    }

    #[test]
    fn test_classify_model_error() {
        let err = anyhow!("Unknown chat model 'foo:bar'");
        assert_eq!(classify_error(&err), ExitCode::ModelError);
    }

    #[test]
    fn test_classify_model_not_available() {
        let err = anyhow!("No available model");
        assert_eq!(classify_error(&err), ExitCode::ModelError);
    }

    #[test]
    fn test_classify_tool_error() {
        let err = anyhow!("Tool call exit with 1");
        assert_eq!(classify_error(&err), ExitCode::ToolError);
    }

    #[test]
    fn test_classify_tool_infinite_loop() {
        let err = anyhow!(
            "The request was aborted because an infinite loop of function calls was detected."
        );
        assert_eq!(classify_error(&err), ExitCode::ToolError);
    }

    #[test]
    fn test_classify_network_error() {
        let err = anyhow!("Failed to build client");
        assert_eq!(classify_error(&err), ExitCode::NetworkError);
    }

    #[test]
    fn test_classify_api_error() {
        let err = anyhow!("Invalid response data: rate limit (status: 429)");
        assert_eq!(classify_error(&err), ExitCode::ApiError);
    }

    #[test]
    fn test_classify_api_safety_block() {
        let err = anyhow!("Blocked due to safety");
        assert_eq!(classify_error(&err), ExitCode::ApiError);
    }

    #[test]
    fn test_classify_config_error() {
        let err = anyhow!("Unknown role `foo`");
        assert_eq!(classify_error(&err), ExitCode::ConfigError);
    }

    #[test]
    fn test_classify_config_circular() {
        let err = anyhow!("Circular role inheritance: a -> b -> a");
        assert_eq!(classify_error(&err), ExitCode::ConfigError);
    }

    #[test]
    fn test_classify_usage_error() {
        let err = anyhow!("No input");
        assert_eq!(classify_error(&err), ExitCode::UsageError);
    }

    #[test]
    fn test_classify_aborted() {
        let err = anyhow!("Aborted!");
        assert_eq!(classify_error(&err), ExitCode::Aborted);
    }

    #[test]
    fn test_classify_general_fallback() {
        let err = anyhow!("something completely unexpected");
        assert_eq!(classify_error(&err), ExitCode::GeneralError);
    }

    #[test]
    fn test_classify_wrapped_error() {
        // anyhow context wraps the inner error — classify should still find it
        let inner = anyhow!("Unknown role `test`");
        let wrapped = inner.context("Failed to set up role");
        assert_eq!(classify_error(&wrapped), ExitCode::ConfigError);
    }

    #[test]
    fn test_exit_code_values() {
        assert_eq!(ExitCode::Success.as_i32(), 0);
        assert_eq!(ExitCode::GeneralError.as_i32(), 1);
        assert_eq!(ExitCode::UsageError.as_i32(), 2);
        assert_eq!(ExitCode::ConfigError.as_i32(), 3);
        assert_eq!(ExitCode::AuthError.as_i32(), 4);
        assert_eq!(ExitCode::NetworkError.as_i32(), 5);
        assert_eq!(ExitCode::ApiError.as_i32(), 6);
        assert_eq!(ExitCode::ModelError.as_i32(), 7);
        assert_eq!(ExitCode::SchemaError.as_i32(), 8);
        assert_eq!(ExitCode::Aborted.as_i32(), 9);
        assert_eq!(ExitCode::ToolError.as_i32(), 10);
    }

    #[test]
    fn test_category_names() {
        assert_eq!(ExitCode::SchemaError.category_name(), "schema_validation");
        assert_eq!(ExitCode::ToolError.category_name(), "tool");
        assert_eq!(ExitCode::ConfigError.category_name(), "config");
    }

    #[test]
    fn test_classify_typed_pipeline_error() {
        let err = anyhow::Error::new(AichatError::PipelineStage {
            stage: 2,
            total: 4,
            role_name: "review".to_string(),
            model_id: Some("claude-sonnet-4-6".to_string()),
            message: "Model returned empty output".to_string(),
        });
        assert_eq!(classify_error(&err), ExitCode::ToolError);
        assert!(err.to_string().contains("Pipeline stage 2/4"));
        assert!(err.to_string().contains("role 'review'"));
        assert!(err.to_string().contains("model 'claude-sonnet-4-6'"));
    }

    #[test]
    fn test_classify_typed_schema_error() {
        let err = anyhow::Error::new(AichatError::SchemaValidation {
            direction: "output".to_string(),
            message: "missing field `name`".to_string(),
        });
        assert_eq!(classify_error(&err), ExitCode::SchemaError);
    }

    #[test]
    fn test_classify_typed_tool_not_found() {
        let err = anyhow::Error::new(AichatError::ToolNotFound {
            name: "nonexistent_tool".to_string(),
        });
        assert_eq!(classify_error(&err), ExitCode::ToolError);
    }

    #[test]
    fn test_aichat_error_json_context() {
        let err = AichatError::PipelineStage {
            stage: 2,
            total: 3,
            role_name: "review".to_string(),
            model_id: Some("claude".to_string()),
            message: "timeout".to_string(),
        };
        let ctx = err.to_json_context();
        assert_eq!(ctx["stage"], 2);
        assert_eq!(ctx["total"], 3);
        assert_eq!(ctx["role"], "review");
        assert_eq!(ctx["model"], "claude");
        assert_eq!(ctx["detail"], "timeout");
    }

    #[test]
    fn test_typed_error_takes_priority_over_string_match() {
        // An AichatError::McpError should classify as ToolError via the fast path,
        // even though the string might match other patterns.
        let err = anyhow::Error::new(AichatError::McpError {
            message: "connection timed out".to_string(), // would match NetworkError via string
            server: Some("github".to_string()),
            tool: Some("create-issue".to_string()),
        });
        // Fast path: typed error → ToolError (not NetworkError from string match)
        assert_eq!(classify_error(&err), ExitCode::ToolError);
    }

    // --- Phase 7 tests ---

    #[test]
    fn test_classify_typed_tool_execution_error() {
        let err = anyhow::Error::new(AichatError::ToolExecutionError {
            tool_name: "web_search".to_string(),
            exit_code: 1,
            stderr: Some("curl: (6) Could not resolve host".to_string()),
            hint: Some("check your internet connection".to_string()),
        });
        assert_eq!(classify_error(&err), ExitCode::ToolError);
        assert!(err.to_string().contains("tool 'web_search' failed"));
        assert!(err.to_string().contains("exit code 1"));
        assert!(err.to_string().contains("stderr:"));
        assert!(err.to_string().contains("hint:"));
    }

    #[test]
    fn test_classify_typed_tool_spawn_error() {
        let err = anyhow::Error::new(AichatError::ToolSpawnError {
            tool_name: "analyze_code".to_string(),
            message: "binary not found".to_string(),
            hint: Some("ensure the tool is installed".to_string()),
        });
        assert_eq!(classify_error(&err), ExitCode::ToolError);
        assert!(err.to_string().contains("tool 'analyze_code'"));
        assert!(err.to_string().contains("could not be started"));
        assert!(err.to_string().contains("hint:"));
    }

    #[test]
    fn test_tool_execution_error_json_context() {
        let err = AichatError::ToolExecutionError {
            tool_name: "web_search".to_string(),
            exit_code: 1,
            stderr: Some("connection refused".to_string()),
            hint: Some("check network".to_string()),
        };
        let ctx = err.to_json_context();
        assert_eq!(ctx["tool"], "web_search");
        assert_eq!(ctx["exit_code"], 1);
        assert_eq!(ctx["stderr"], "connection refused");
        assert_eq!(ctx["hint"], "check network");
    }

    #[test]
    fn test_tool_spawn_error_json_context() {
        let err = AichatError::ToolSpawnError {
            tool_name: "my_tool".to_string(),
            message: "binary not found".to_string(),
            hint: Some("install it".to_string()),
        };
        let ctx = err.to_json_context();
        assert_eq!(ctx["tool"], "my_tool");
        assert_eq!(ctx["detail"], "binary not found");
        assert_eq!(ctx["hint"], "install it");
    }

    #[test]
    fn test_tool_execution_error_no_stderr() {
        let err = AichatError::ToolExecutionError {
            tool_name: "silent_tool".to_string(),
            exit_code: 2,
            stderr: None,
            hint: None,
        };
        let display = err.to_string();
        assert!(display.contains("tool 'silent_tool' failed (exit code 2)"));
        assert!(!display.contains("stderr:"));
        assert!(!display.contains("hint:"));
    }

    #[test]
    fn test_classify_new_tool_error_strings() {
        // Test that the string-matching fallback catches new error formats
        let err = anyhow!("error: tool 'web_search' failed (exit code 1)");
        assert_eq!(classify_error(&err), ExitCode::ToolError);

        let err = anyhow!("tool 'my_tool' could not be started: binary not found");
        assert_eq!(classify_error(&err), ExitCode::ToolError);

        let err = anyhow!("binary is not executable");
        assert_eq!(classify_error(&err), ExitCode::ToolError);
    }

    // --- Phase 8 tests ---

    #[test]
    fn test_classify_typed_tool_timeout() {
        let err = anyhow::Error::new(AichatError::ToolTimeout {
            tool_name: "slow_tool".to_string(),
            timeout_secs: 30,
        });
        assert_eq!(classify_error(&err), ExitCode::ToolError);
        assert!(err.to_string().contains("tool 'slow_tool'"));
        assert!(err.to_string().contains("timed out after 30s"));
        assert!(err.to_string().contains("hint:"));
    }

    #[test]
    fn test_tool_timeout_json_context() {
        let err = AichatError::ToolTimeout {
            tool_name: "slow_tool".to_string(),
            timeout_secs: 60,
        };
        let ctx = err.to_json_context();
        assert_eq!(ctx["tool"], "slow_tool");
        assert_eq!(ctx["timeout_secs"], 60);
    }

    #[test]
    fn test_classify_timeout_string_fallback() {
        let err = anyhow!("error: tool 'slow_tool' timed out after 30s");
        assert_eq!(classify_error(&err), ExitCode::ToolError);
    }
}
