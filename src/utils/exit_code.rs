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
}

/// Inspect an `anyhow::Error` chain and return the most specific exit code.
pub fn classify_error(err: &anyhow::Error) -> ExitCode {
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
}
