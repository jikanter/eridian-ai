use crate::client::CallMetrics;
use crate::config::role::SchemaValidationResult;
use crate::function::ToolResult;

use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub human_trace: bool,
    pub jsonl_trace: bool,
    pub jsonl_file: Option<PathBuf>,
    pub truncate_at: usize,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            human_trace: false,
            jsonl_trace: false,
            jsonl_file: None,
            truncate_at: 500,
        }
    }
}

pub struct TraceEmitter {
    config: TraceConfig,
    turn: u32,
}

impl TraceEmitter {
    pub fn new(config: TraceConfig) -> Self {
        Self { config, turn: 0 }
    }

    /// Emit trace after each API response in call_react
    pub fn emit_request(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        latency_ms: u64,
        tool_call_names: &[String],
        content_summary: &str,
    ) {
        self.turn += 1;
        let turn = self.turn;

        if self.config.human_trace {
            let secs = latency_ms as f64 / 1000.0;
            eprint!("[{turn}] -> {model}  {input_tokens}tok in  {output_tokens}tok out  {secs:.1}s");
            if !tool_call_names.is_empty() {
                eprintln!();
                for name in tool_call_names {
                    eprintln!("    <- tool_call: {name}");
                }
            } else {
                let summary = truncate(content_summary, self.config.truncate_at);
                if !summary.is_empty() {
                    eprintln!("  \"{summary}\"");
                } else {
                    eprintln!();
                }
            }
        }

        if self.config.jsonl_trace {
            let obj = serde_json::json!({
                "type": "request",
                "turn": turn,
                "model": model,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "latency_ms": latency_ms,
                "tool_calls": tool_call_names,
            });
            self.write_jsonl(&obj);
        }
    }

    /// Emit trace after tool evaluation
    pub fn emit_tool_results(&mut self, results: &[ToolResult], latency_ms: u64) {
        let turn = self.turn;

        if self.config.human_trace {
            let secs = latency_ms as f64 / 1000.0;
            for r in results {
                let output_str = r.output.to_string();
                let chars = output_str.len();
                let has_error = output_str.contains("[TOOL_ERROR]");
                let status = if has_error { "err" } else { "ok" };
                eprintln!(
                    "[{turn}] <- {}  {status}  {secs:.1}s  ({chars} chars)",
                    r.call.name
                );
            }
        }

        if self.config.jsonl_trace {
            let tool_info: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    let output_str = r.output.to_string();
                    serde_json::json!({
                        "tool": r.call.name,
                        "has_error": output_str.contains("[TOOL_ERROR]"),
                        "output_chars": output_str.len(),
                    })
                })
                .collect();
            let obj = serde_json::json!({
                "type": "tool_results",
                "turn": turn,
                "latency_ms": latency_ms,
                "results": tool_info,
            });
            self.write_jsonl(&obj);
        }
    }

    /// Emit summary at the end of call_react
    pub fn emit_summary(&self, metrics: &CallMetrics) {
        if self.config.human_trace {
            eprintln!(
                "total: {} turns  {}tok in  {}tok out  ${:.4}  {:.1}s",
                metrics.turns,
                metrics.input_tokens,
                metrics.output_tokens,
                metrics.cost_usd,
                metrics.latency_ms as f64 / 1000.0,
            );
        }

        if self.config.jsonl_trace {
            let obj = serde_json::json!({
                "type": "summary",
                "turns": metrics.turns,
                "input_tokens": metrics.input_tokens,
                "output_tokens": metrics.output_tokens,
                "cost_usd": metrics.cost_usd,
                "latency_ms": metrics.latency_ms,
                "model": metrics.model_id,
            });
            self.write_jsonl(&obj);
        }
    }

    /// Emit trace for schema validation (input or output).
    pub fn emit_schema_validation(&self, result: &SchemaValidationResult) {
        let pass = result.is_ok();
        let direction = &result.direction;

        if self.config.human_trace {
            if pass {
                eprintln!("[schema] OK {direction}");
            } else {
                let n = result.violations.len();
                eprintln!("[schema] FAIL {direction}  {n} violation{}", if n == 1 { "" } else { "s" });
                let raw = truncate(&result.raw_text, self.config.truncate_at);
                eprintln!("    raw: {raw}");
                for v in &result.violations {
                    if v.instance_path.is_empty() {
                        eprintln!("    - {}", v.message);
                    } else {
                        eprintln!("    - {}: {}", v.instance_path, v.message);
                    }
                }
            }
        }

        if self.config.jsonl_trace {
            let violations: Vec<serde_json::Value> = result
                .violations
                .iter()
                .map(|v| {
                    serde_json::json!({
                        "message": v.message,
                        "instance_path": v.instance_path,
                        "schema_path": v.schema_path,
                    })
                })
                .collect();
            let mut obj = serde_json::json!({
                "type": "schema_validation",
                "direction": direction,
                "pass": pass,
            });
            if !pass {
                obj["raw_output"] = serde_json::Value::String(
                    truncate(&result.raw_text, self.config.truncate_at),
                );
                obj["violations"] = serde_json::Value::Array(violations);
            }
            self.write_jsonl(&obj);
        }
    }

    fn write_jsonl(&self, value: &serde_json::Value) {
        let line = serde_json::to_string(value).unwrap_or_default();
        match &self.config.jsonl_file {
            Some(path) => {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(f, "{line}");
                }
            }
            None => {
                eprintln!("{line}");
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    let trimmed = s.trim().replace('\n', " ");
    if trimmed.len() <= max {
        trimmed
    } else {
        format!("{}...", &trimmed[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::role::{SchemaValidationResult, SchemaViolation};

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let long = "a".repeat(600);
        let result = truncate(&long, 500);
        assert_eq!(result.len(), 503); // 500 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_newlines() {
        assert_eq!(truncate("hello\nworld", 20), "hello world");
    }

    #[test]
    fn test_emit_schema_validation_pass_no_panic() {
        let config = TraceConfig {
            human_trace: true,
            jsonl_trace: true,
            jsonl_file: None,
            truncate_at: 500,
        };
        let emitter = TraceEmitter::new(config);
        let result = SchemaValidationResult {
            direction: "input".to_string(),
            raw_text: r#"{"query": "hello"}"#.to_string(),
            violations: vec![],
        };
        // Should not panic; output goes to stderr
        emitter.emit_schema_validation(&result);
    }

    #[test]
    fn test_emit_schema_validation_fail_no_panic() {
        let config = TraceConfig {
            human_trace: true,
            jsonl_trace: true,
            jsonl_file: None,
            truncate_at: 500,
        };
        let emitter = TraceEmitter::new(config);
        let result = SchemaValidationResult {
            direction: "output".to_string(),
            raw_text: r#"{"count": "bad"}"#.to_string(),
            violations: vec![
                SchemaViolation {
                    message: r#""bad" is not of type "integer""#.to_string(),
                    instance_path: "/count".to_string(),
                    schema_path: "/properties/count/type".to_string(),
                },
            ],
        };
        emitter.emit_schema_validation(&result);
    }

    #[test]
    fn test_emit_schema_validation_jsonl_to_file() {
        let tmp = std::env::temp_dir().join("aichat-test-schema-trace.jsonl");
        let _ = std::fs::remove_file(&tmp);
        let config = TraceConfig {
            human_trace: false,
            jsonl_trace: true,
            jsonl_file: Some(tmp.clone()),
            truncate_at: 500,
        };
        let emitter = TraceEmitter::new(config);
        let result = SchemaValidationResult {
            direction: "output".to_string(),
            raw_text: r#"{"x": "bad"}"#.to_string(),
            violations: vec![
                SchemaViolation {
                    message: "type mismatch".to_string(),
                    instance_path: "/x".to_string(),
                    schema_path: "/properties/x/type".to_string(),
                },
            ],
        };
        emitter.emit_schema_validation(&result);

        let content = std::fs::read_to_string(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["type"], "schema_validation");
        assert_eq!(parsed["direction"], "output");
        assert_eq!(parsed["pass"], false);
        assert!(parsed["raw_output"].as_str().unwrap().contains("bad"));
        assert_eq!(parsed["violations"].as_array().unwrap().len(), 1);
    }
}
