// Phase 23B: side-by-side comparison of two roles run on the same input.
//
// `render_comparison` is pure (no LLM calls, no I/O) so it can be unit-tested
// against fixed `CompareResult` values. The driver in `main.rs` invokes each
// role via `pipe::invoke_role`, evaluates its metrics, and feeds the results
// here.

use crate::config::role::MetricResult;

/// One side of a `--compare` run.
#[derive(Debug, Clone)]
pub struct CompareResult {
    pub role: String,
    pub model: String,
    pub output: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub metrics: Vec<MetricResult>,
}

fn metrics_line(metrics: &[MetricResult]) -> String {
    if metrics.is_empty() {
        return "(none)".to_string();
    }
    metrics
        .iter()
        .map(|m| format!("{}={}", m.name, if m.pass { "PASS" } else { "FAIL" }))
        .collect::<Vec<_>>()
        .join("  ")
}

fn metrics_json(metrics: &[MetricResult]) -> serde_json::Value {
    serde_json::Value::Array(
        metrics
            .iter()
            .map(|m| serde_json::json!({ "name": m.name, "pass": m.pass }))
            .collect(),
    )
}

fn side_json(r: &CompareResult) -> serde_json::Value {
    serde_json::json!({
        "role": r.role,
        "model": r.model,
        "output": r.output,
        "input_tokens": r.input_tokens,
        "output_tokens": r.output_tokens,
        "cost_usd": r.cost_usd,
        "metrics": metrics_json(&r.metrics),
    })
}

/// Cost ratio sentence comparing `b` to `a`. Guards division by zero.
fn cost_ratio_sentence(a: &CompareResult, b: &CompareResult) -> String {
    if a.cost_usd <= 0.0 && b.cost_usd <= 0.0 {
        return "Cost ratio: both roles reported $0.00".to_string();
    }
    if a.cost_usd <= 0.0 {
        return format!("Cost ratio: {} reported $0.00 (no baseline)", a.role);
    }
    let ratio = b.cost_usd / a.cost_usd;
    if ratio > 1.0 {
        format!("Cost ratio: {} is {:.1}x more expensive", b.role, ratio)
    } else if ratio < 1.0 && ratio > 0.0 {
        format!("Cost ratio: {} is {:.1}x cheaper", b.role, 1.0 / ratio)
    } else {
        "Cost ratio: identical cost".to_string()
    }
}

/// Output-token delta sentence (percentage), `b` relative to `a`.
fn token_delta_sentence(a: &CompareResult, b: &CompareResult) -> String {
    if a.output_tokens == 0 {
        return format!("Token ratio: {} produced 0 output tokens (no baseline)", a.role);
    }
    let delta =
        (b.output_tokens as f64 - a.output_tokens as f64) / a.output_tokens as f64 * 100.0;
    if delta > 0.0 {
        format!("Token ratio: {} uses {:.0}% more output tokens", b.role, delta)
    } else if delta < 0.0 {
        format!("Token ratio: {} uses {:.0}% fewer output tokens", b.role, -delta)
    } else {
        "Token ratio: identical output tokens".to_string()
    }
}

/// Metrics-agreement summary across the two sides.
fn metrics_agreement_sentence(a: &CompareResult, b: &CompareResult) -> String {
    let a_all = !a.metrics.is_empty() && a.metrics.iter().all(|m| m.pass);
    let b_all = !b.metrics.is_empty() && b.metrics.iter().all(|m| m.pass);
    if a.metrics.is_empty() && b.metrics.is_empty() {
        return "Metrics: no metrics declared".to_string();
    }
    if a_all && b_all {
        return "Metrics: both pass all metrics".to_string();
    }
    let a_pass = a.metrics.iter().filter(|m| m.pass).count();
    let b_pass = b.metrics.iter().filter(|m| m.pass).count();
    format!(
        "Metrics: {} {}/{}  vs  {} {}/{}",
        a.role,
        a_pass,
        a.metrics.len(),
        b.role,
        b_pass,
        b.metrics.len()
    )
}

/// Render the side-by-side comparison block (or a single JSON object under
/// `-o json`).
pub fn render_comparison(a: &CompareResult, b: &CompareResult, json: bool) -> String {
    if json {
        let obj = serde_json::json!({
            "roleA": side_json(a),
            "roleB": side_json(b),
            "comparison": {
                "cost_ratio": if a.cost_usd > 0.0 { b.cost_usd / a.cost_usd } else { 0.0 },
                "output_token_delta_pct": if a.output_tokens > 0 {
                    (b.output_tokens as f64 - a.output_tokens as f64) / a.output_tokens as f64 * 100.0
                } else { 0.0 },
                "a_metrics_all_pass": !a.metrics.is_empty() && a.metrics.iter().all(|m| m.pass),
                "b_metrics_all_pass": !b.metrics.is_empty() && b.metrics.iter().all(|m| m.pass),
            }
        });
        return serde_json::to_string_pretty(&obj).unwrap_or_default();
    }

    let mut out = String::new();
    for r in [a, b] {
        out.push_str(&format!("--- {} ({}) ---\n", r.role, r.model));
        out.push_str(&format!("  Output: {}\n", r.output.trim()));
        out.push_str(&format!("  Metrics: {}\n", metrics_line(&r.metrics)));
        out.push_str(&format!(
            "  Cost: ${:.4}  ({} input, {} output tokens)\n",
            r.cost_usd, r.input_tokens, r.output_tokens
        ));
        out.push('\n');
    }
    out.push_str("--- Comparison ---\n");
    out.push_str(&format!("  {}\n", cost_ratio_sentence(a, b)));
    out.push_str(&format!("  {}\n", token_delta_sentence(a, b)));
    out.push_str(&format!("  {}\n", metrics_agreement_sentence(a, b)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(role: &str, cost: f64, out_tok: u64, metrics: Vec<MetricResult>) -> CompareResult {
        CompareResult {
            role: role.to_string(),
            model: "deepseek:deepseek-chat".to_string(),
            output: "the output".to_string(),
            input_tokens: 892,
            output_tokens: out_tok,
            cost_usd: cost,
            metrics,
        }
    }

    fn pass(name: &str) -> MetricResult {
        MetricResult { name: name.to_string(), pass: true }
    }
    fn fail(name: &str) -> MetricResult {
        MetricResult { name: name.to_string(), pass: false }
    }

    #[test]
    fn test_render_human_contains_both_roles() {
        let a = side("v1", 0.0004, 341, vec![pass("valid_json")]);
        let b = side("v2", 0.002, 287, vec![pass("valid_json")]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("--- v1 (deepseek:deepseek-chat) ---"));
        assert!(s.contains("--- v2 (deepseek:deepseek-chat) ---"));
        assert!(s.contains("--- Comparison ---"));
    }

    #[test]
    fn test_render_cost_ratio_more_expensive() {
        let a = side("v1", 0.0004, 341, vec![]);
        let b = side("v2", 0.002, 287, vec![]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("v2 is 5.0x more expensive"), "got: {s}");
    }

    #[test]
    fn test_render_cost_ratio_cheaper() {
        let a = side("v1", 0.002, 341, vec![]);
        let b = side("v2", 0.0004, 287, vec![]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("v2 is 5.0x cheaper"), "got: {s}");
    }

    #[test]
    fn test_render_cost_ratio_zero_guard() {
        let a = side("v1", 0.0, 341, vec![]);
        let b = side("v2", 0.0, 287, vec![]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("both roles reported $0.00"), "got: {s}");
    }

    #[test]
    fn test_render_token_delta_fewer() {
        let a = side("v1", 0.0004, 341, vec![]);
        let b = side("v2", 0.002, 287, vec![]);
        let s = render_comparison(&a, &b, false);
        // (287-341)/341 = -15.8% -> "16% fewer"
        assert!(s.contains("v2 uses 16% fewer output tokens"), "got: {s}");
    }

    #[test]
    fn test_render_token_delta_zero_baseline() {
        let a = side("v1", 0.0004, 0, vec![]);
        let b = side("v2", 0.002, 287, vec![]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("no baseline"), "got: {s}");
    }

    #[test]
    fn test_render_metrics_both_pass() {
        let a = side("v1", 0.0004, 341, vec![pass("a"), pass("b")]);
        let b = side("v2", 0.002, 287, vec![pass("a"), pass("b")]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("both pass all metrics"), "got: {s}");
    }

    #[test]
    fn test_render_metrics_mismatch() {
        let a = side("v1", 0.0004, 341, vec![pass("a"), pass("b")]);
        let b = side("v2", 0.002, 287, vec![pass("a"), fail("b")]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("v1 2/2"), "got: {s}");
        assert!(s.contains("v2 1/2"), "got: {s}");
    }

    #[test]
    fn test_render_metrics_line_in_body() {
        let a = side("v1", 0.0004, 341, vec![pass("valid_json"), fail("short")]);
        let b = side("v2", 0.002, 287, vec![]);
        let s = render_comparison(&a, &b, false);
        assert!(s.contains("valid_json=PASS"), "got: {s}");
        assert!(s.contains("short=FAIL"), "got: {s}");
        assert!(s.contains("Metrics: (none)"), "got: {s}");
    }

    #[test]
    fn test_render_json_shape() {
        let a = side("v1", 0.0004, 341, vec![pass("valid_json")]);
        let b = side("v2", 0.002, 287, vec![fail("valid_json")]);
        let s = render_comparison(&a, &b, true);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["roleA"]["role"], "v1");
        assert_eq!(v["roleB"]["role"], "v2");
        assert_eq!(v["roleA"]["output_tokens"], 341);
        assert_eq!(v["roleA"]["metrics"][0]["pass"], true);
        assert_eq!(v["roleB"]["metrics"][0]["pass"], false);
        assert!(v["comparison"]["cost_ratio"].as_f64().unwrap() > 4.9);
        assert!(v["comparison"]["a_metrics_all_pass"].as_bool().unwrap());
        assert!(!v["comparison"]["b_metrics_all_pass"].as_bool().unwrap());
    }
}
