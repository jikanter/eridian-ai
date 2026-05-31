use anyhow::{Context, Result};
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// Append one JSONL record to the run log file.
pub fn append_run_log(path: &Path, record: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create run log directory: {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open run log: {}", path.display()))?;
    let line = serde_json::to_string(record)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Phase 23D: truncate a free-form summary string to at most `max` chars,
/// collapsing newlines and trimming, appending an ellipsis when cut. Char-safe
/// (never splits a UTF-8 codepoint).
pub fn truncate_summary(s: &str, max: usize) -> String {
    let trimmed = s.trim().replace('\n', " ");
    if trimmed.chars().count() <= max {
        trimmed
    } else {
        let cut: String = trimmed.chars().take(max).collect();
        format!("{cut}...")
    }
}

/// Phase 23A/23C: metric results serialized for a run-log/ledger record.
#[derive(Debug, Clone)]
pub struct MetricRecord {
    pub name: String,
    pub pass: bool,
}

fn metrics_to_json(metrics: &[MetricRecord]) -> Vec<Value> {
    metrics
        .iter()
        .map(|m| serde_json::json!({ "name": m.name, "pass": m.pass }))
        .collect()
}

/// Phase 23D: build one per-role invocation-history ledger record. Summaries
/// are truncated to ~200 chars. Pure: callers handle the filesystem write.
#[allow(clippy::too_many_arguments)]
pub fn role_ledger_record(
    run_id: &str,
    role: &str,
    model: &str,
    input: &str,
    output: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    latency_ms: u64,
    schema_retries: usize,
    metrics: &[MetricRecord],
) -> Value {
    serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "run_id": run_id,
        "role": role,
        "model": model,
        "input_summary": truncate_summary(input, 200),
        "output_summary": truncate_summary(output, 200),
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost_usd": cost_usd,
        "latency_ms": latency_ms,
        "schema_retries": schema_retries,
        "metrics": metrics_to_json(metrics),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_summary_short() {
        assert_eq!(truncate_summary("hello", 200), "hello");
    }

    #[test]
    fn test_truncate_summary_collapses_newlines() {
        assert_eq!(truncate_summary("a\nb\nc", 200), "a b c");
    }

    #[test]
    fn test_truncate_summary_long() {
        let long = "x".repeat(500);
        let out = truncate_summary(&long, 200);
        assert_eq!(out.chars().count(), 203); // 200 + "..."
        assert!(out.ends_with("..."));
    }

    #[test]
    fn test_truncate_summary_utf8_safe() {
        let s = "é".repeat(300);
        let out = truncate_summary(&s, 200);
        assert!(out.ends_with("..."));
        assert_eq!(out.chars().filter(|c| *c == 'é').count(), 200);
    }

    #[test]
    fn test_role_ledger_record_shape() {
        let metrics = vec![
            MetricRecord { name: "valid_json".into(), pass: true },
            MetricRecord { name: "short".into(), pass: false },
        ];
        let rec = role_ledger_record(
            "run-123",
            "summarizer",
            "openai:gpt-4",
            "long input text",
            "the output",
            100,
            42,
            0.0012,
            850,
            2,
            &metrics,
        );
        assert_eq!(rec["run_id"], "run-123");
        assert_eq!(rec["role"], "summarizer");
        assert_eq!(rec["model"], "openai:gpt-4");
        assert_eq!(rec["input_summary"], "long input text");
        assert_eq!(rec["output_summary"], "the output");
        assert_eq!(rec["input_tokens"], 100);
        assert_eq!(rec["output_tokens"], 42);
        assert_eq!(rec["latency_ms"], 850);
        assert_eq!(rec["schema_retries"], 2);
        let m = rec["metrics"].as_array().unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0]["name"], "valid_json");
        assert_eq!(m[0]["pass"], true);
        assert_eq!(m[1]["pass"], false);
        assert!(rec["ts"].is_string());
    }

    #[test]
    fn test_role_ledger_record_truncates_summaries() {
        let big = "z".repeat(400);
        let rec = role_ledger_record(
            "r", "role", "m", &big, &big, 0, 0, 0.0, 0, 0, &[],
        );
        assert!(rec["input_summary"].as_str().unwrap().ends_with("..."));
        assert!(rec["output_summary"].as_str().unwrap().ends_with("..."));
        assert!(rec["metrics"].as_array().unwrap().is_empty());
    }
}
