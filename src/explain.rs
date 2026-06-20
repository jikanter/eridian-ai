//! `--explain-context`: a richer dry-run that prints the *assembled* context
//! for an Entity invocation — system prompt, injected memory, user turn, and
//! tool schemas — with a per-section token estimate, without calling the model.
//!
//! The dry-run preview (`emit_dry_run_preview`) answers "what config will this
//! run with?". This answers the orthogonal "what context will actually land in
//! the window, and where do the tokens go?" — the context-engineering view.
//!
//! Pure over the already-assembled `messages` + tool `functions`, so the report
//! is computed at zero token cost from data `prepare_completion_data` already
//! produced.

use crate::client::Message;
use crate::function::FunctionDeclaration;
use crate::utils::estimate_token_length;

/// Max characters shown in a section preview, single-lined.
const PREVIEW_LEN: usize = 80;

/// Single labelled slice of the assembled context with its token/byte weight.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextSection {
    pub label: String,
    pub tokens: usize,
    pub bytes: usize,
    pub preview: String,
}

/// Per-section breakdown of an assembled context plus its totals.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContextExplain {
    pub sections: Vec<ContextSection>,
    pub total_tokens: usize,
    pub total_bytes: usize,
}

impl ContextExplain {
    /// Human-readable table, one row per section plus a TOTAL row. Each row
    /// shows the section's share of the total so the token budget is legible
    /// at a glance.
    pub fn render(&self) -> String {
        let mut out = String::from("--- Context Explain ---\n");
        for s in &self.sections {
            let pct = if self.total_tokens > 0 {
                s.tokens * 100 / self.total_tokens
            } else {
                0
            };
            out.push_str(&format!(
                "  {:<18} {:>6} tok  {:>3}%  {}\n",
                s.label, s.tokens, pct, s.preview
            ));
        }
        out.push_str(&format!(
            "  {:<18} {:>6} tok  ({} bytes)\n",
            "TOTAL", self.total_tokens, self.total_bytes
        ));
        out
    }

    /// Machine-readable form for `-o json`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Collapse text to a single bounded line for table display.
fn preview_of(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= PREVIEW_LEN {
        one_line
    } else {
        let truncated: String = one_line.chars().take(PREVIEW_LEN - 1).collect();
        format!("{truncated}…")
    }
}

/// Build a per-section token/byte breakdown of an assembled context. One section
/// per message (labelled by role), then a single aggregate `tools (N)` section
/// when tool schemas are present.
pub fn explain_context(
    messages: &[Message],
    functions: Option<&[FunctionDeclaration]>,
) -> ContextExplain {
    let mut sections: Vec<ContextSection> = messages
        .iter()
        .map(|m| {
            let text = m.content.to_text();
            ContextSection {
                label: role_label(m.role).to_string(),
                tokens: estimate_token_length(&text),
                bytes: text.len(),
                preview: preview_of(&text),
            }
        })
        .collect();

    if let Some(fns) = functions {
        if !fns.is_empty() {
            // Token weight is the serialized schema the provider actually sees.
            let json = serde_json::to_string(fns).unwrap_or_default();
            let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
            sections.push(ContextSection {
                label: format!("tools ({})", fns.len()),
                tokens: estimate_token_length(&json),
                bytes: json.len(),
                preview: preview_of(&names.join(", ")),
            });
        }
    }

    let total_tokens = sections.iter().map(|s| s.tokens).sum();
    let total_bytes = sections.iter().map(|s| s.bytes).sum();
    ContextExplain {
        sections,
        total_tokens,
        total_bytes,
    }
}

/// Lower-case wire name for a message role, used as the section label.
fn role_label(role: crate::client::MessageRole) -> &'static str {
    use crate::client::MessageRole::*;
    match role {
        System => "system",
        User => "user",
        Assistant => "assistant",
        Tool => "tool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Message, MessageContent, MessageRole};
    use crate::function::{FunctionDeclaration, ToolSource};
    use crate::utils::estimate_token_length;

    fn msg(role: MessageRole, text: &str) -> Message {
        Message::new(role, MessageContent::Text(text.to_string()))
    }

    fn weather_tool() -> FunctionDeclaration {
        FunctionDeclaration {
            name: "get_weather".into(),
            description: "Get the weather for a city".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } }
            }),
            agent: false,
            source: ToolSource::default(),
            examples: None,
            timeout: None,
        }
    }

    #[test]
    fn one_section_per_message_with_role_labels() {
        let messages = vec![
            msg(MessageRole::System, "You are a helpful assistant."),
            msg(MessageRole::User, "hello world"),
        ];

        let report = explain_context(&messages, None);

        assert_eq!(report.sections.len(), 2);
        assert_eq!(report.sections[0].label, "system");
        assert_eq!(report.sections[1].label, "user");
    }

    #[test]
    fn section_tokens_match_estimator_and_total_is_the_sum() {
        let messages = vec![
            msg(MessageRole::System, "You are a helpful assistant."),
            msg(MessageRole::User, "hello world"),
        ];

        let report = explain_context(&messages, None);

        assert_eq!(
            report.sections[0].tokens,
            estimate_token_length("You are a helpful assistant.")
        );
        assert_eq!(
            report.sections[1].tokens,
            estimate_token_length("hello world")
        );
        assert_eq!(
            report.total_tokens,
            report.sections.iter().map(|s| s.tokens).sum::<usize>()
        );
    }

    #[test]
    fn tools_become_a_section_when_present() {
        let messages = vec![msg(MessageRole::System, "sys")];
        let tools = vec![weather_tool()];

        let report = explain_context(&messages, Some(&tools));

        let tool_section = report
            .sections
            .last()
            .expect("expected a tools section appended after messages");
        assert!(
            tool_section.label.starts_with("tools (1)"),
            "label was {:?}",
            tool_section.label
        );
        assert!(tool_section.tokens > 0);
        assert!(tool_section.preview.contains("get_weather"));
    }

    #[test]
    fn no_tools_section_when_functions_absent_or_empty() {
        let messages = vec![msg(MessageRole::System, "sys")];

        assert_eq!(explain_context(&messages, None).sections.len(), 1);
        assert_eq!(explain_context(&messages, Some(&[])).sections.len(), 1);
    }

    #[test]
    fn preview_is_single_line_and_bounded() {
        let long = format!("{}\nsecond line", "x".repeat(500));
        let messages = vec![msg(MessageRole::User, &long)];

        let report = explain_context(&messages, None);
        let preview = &report.sections[0].preview;

        assert!(!preview.contains('\n'), "preview must be single line");
        assert!(preview.chars().count() <= 80, "preview must be bounded");
    }

    #[test]
    fn render_lists_each_label_and_a_total() {
        let messages = vec![
            msg(MessageRole::System, "You are a helpful assistant."),
            msg(MessageRole::User, "hello world"),
        ];

        let out = explain_context(&messages, None).render();

        assert!(out.contains("system"));
        assert!(out.contains("user"));
        assert!(out.contains("TOTAL"));
    }

    #[test]
    fn to_json_exposes_sections_and_totals() {
        let messages = vec![msg(MessageRole::System, "sys")];
        let report = explain_context(&messages, None);

        let json = report.to_json();
        assert!(json["sections"].is_array());
        assert_eq!(json["sections"].as_array().unwrap().len(), 1);
        assert!(json["total_tokens"].is_number());
    }
}
