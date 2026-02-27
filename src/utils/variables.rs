use super::*;
use crate::client::Model;
use fancy_regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::LazyLock;

pub static RE_VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{(\w+)\}\}").unwrap());

static RE_CONDITIONAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\{\{#(if|unless)\s+(\w+)(?:\s*(>=|<=|!=|==|>|<)\s*(\w+))?\s*\}\}(.*?)\{\{/(if|unless)\}\}")
        .unwrap()
});

pub fn interpolate_variables(text: &mut String) {
    interpolate_variables_with_model(text, None);
}

pub fn interpolate_variables_with_model(text: &mut String, model: Option<&Model>) {
    let model_vars = model.map(|m| resolve_model_variables(m));

    // Phase 1: Process conditional blocks
    loop {
        let result = RE_CONDITIONAL
            .replace_all(text, |caps: &Captures<'_>| {
                let block_type = &caps[1];
                let var_name = &caps[2];
                let operator = caps.get(3).map(|m| m.as_str());
                let operand = caps.get(4).map(|m| m.as_str());
                let body = &caps[5];
                let closing_tag = &caps[6];

                // Mismatched tags pass through unchanged
                if block_type != closing_tag {
                    return caps[0].to_string();
                }

                let resolved = resolve_variable(var_name, &model_vars);
                // If the variable is unresolved, keep the block as-is
                if resolved.starts_with("{{") && resolved.ends_with("}}") {
                    return caps[0].to_string();
                }

                let condition = match (operator, operand) {
                    (Some(op), Some(rhs)) => eval_comparison(&resolved, op, rhs),
                    _ => is_truthy(&resolved),
                };

                let keep = match block_type {
                    "if" => condition,
                    "unless" => !condition,
                    _ => false,
                };

                if keep {
                    body.trim_matches('\n').to_string()
                } else {
                    String::new()
                }
            })
            .to_string();

        if result == *text {
            break;
        }
        *text = result;
    }

    // Phase 2: Replace simple variables
    *text = RE_VARIABLE
        .replace_all(text, |caps: &Captures<'_>| {
            let key = &caps[1];
            resolve_variable(key, &model_vars)
        })
        .to_string();
}

fn resolve_variable(key: &str, model_vars: &Option<HashMap<&str, String>>) -> String {
    match key {
        "__os__" => env::consts::OS.to_string(),
        "__os_distro__" => {
            let info = os_info::get();
            if env::consts::OS == "linux" {
                format!("{info} (linux)")
            } else {
                info.to_string()
            }
        }
        "__os_family__" => env::consts::FAMILY.to_string(),
        "__arch__" => env::consts::ARCH.to_string(),
        "__shell__" => SHELL.name.clone(),
        "__locale__" => sys_locale::get_locale().unwrap_or_default(),
        "__now__" => now(),
        "__cwd__" => env::current_dir()
            .map(|v| v.display().to_string())
            .unwrap_or_default(),
        _ => {
            if let Some(vars) = model_vars {
                if let Some(val) = vars.get(key) {
                    return val.clone();
                }
            }
            format!("{{{{{key}}}}}")
        }
    }
}

fn resolve_model_variables(model: &Model) -> HashMap<&'static str, String> {
    let data = model.data();
    let mut vars = HashMap::new();
    vars.insert("__model_id__", model.id());
    vars.insert("__model_name__", model.name().to_string());
    vars.insert("__model_client__", model.client_name().to_string());
    vars.insert(
        "__max_input_tokens__",
        data.max_input_tokens
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    );
    vars.insert(
        "__max_output_tokens__",
        data.max_output_tokens
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    );
    vars.insert(
        "__supports_vision__",
        data.supports_vision.to_string(),
    );
    vars.insert(
        "__supports_function_calling__",
        data.supports_function_calling.to_string(),
    );
    vars.insert("__supports_stream__", (!model.no_stream()).to_string());
    vars
}

fn is_truthy(value: &str) -> bool {
    !matches!(value, "false" | "0" | "" | "unknown")
}

fn eval_comparison(lhs: &str, op: &str, rhs: &str) -> bool {
    // Try numeric comparison first
    if let (Ok(l), Ok(r)) = (lhs.parse::<i64>(), rhs.parse::<i64>()) {
        return match op {
            ">" => l > r,
            ">=" => l >= r,
            "<" => l < r,
            "<=" => l <= r,
            "==" => l == r,
            "!=" => l != r,
            _ => false,
        };
    }
    // Fall back to string comparison
    match op {
        "==" => lhs == rhs,
        "!=" => lhs != rhs,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_test_model() -> Model {
        let mut model = Model::new("openai", "gpt-4o");
        let data = model.data_mut();
        data.max_input_tokens = Some(128000);
        data.max_output_tokens = Some(16384);
        data.supports_vision = true;
        data.supports_function_calling = true;
        model
    }

    fn make_small_model() -> Model {
        let mut model = Model::new("local", "tiny-llm");
        let data = model.data_mut();
        data.max_input_tokens = Some(4096);
        data.max_output_tokens = Some(2048);
        data.supports_vision = false;
        data.supports_function_calling = false;
        model
    }

    #[test]
    fn test_model_variables_resolve() {
        let model = make_test_model();
        let mut text = "Model: {{__model_id__}}, Name: {{__model_name__}}, Client: {{__model_client__}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert_eq!(text, "Model: openai:gpt-4o, Name: gpt-4o, Client: openai");
    }

    #[test]
    fn test_model_variables_without_model() {
        let mut text = "Model: {{__model_id__}}".to_string();
        interpolate_variables_with_model(&mut text, None);
        assert_eq!(text, "Model: {{__model_id__}}");
    }

    #[test]
    fn test_conditional_if_truthy() {
        let model = make_test_model();
        let mut text = "Start\n{{#if __supports_vision__}}\nVision enabled\n{{/if}}\nEnd".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(text.contains("Vision enabled"));
        assert!(text.contains("Start"));
        assert!(text.contains("End"));
    }

    #[test]
    fn test_conditional_if_falsy() {
        let model = make_small_model();
        let mut text = "Start\n{{#if __supports_vision__}}\nVision enabled\n{{/if}}\nEnd".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(!text.contains("Vision enabled"));
    }

    #[test]
    fn test_conditional_unless() {
        let model = make_small_model();
        let mut text = "{{#unless __supports_function_calling__}}\nNo tools available.\n{{/unless}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(text.contains("No tools available."));
    }

    #[test]
    fn test_conditional_unless_truthy_hides() {
        let model = make_test_model();
        let mut text = "{{#unless __supports_function_calling__}}\nNo tools.\n{{/unless}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(!text.contains("No tools."));
    }

    #[test]
    fn test_numeric_comparison_gte() {
        let model = make_test_model();
        let mut text = "{{#if __max_input_tokens__ >= 64000}}\nLarge context\n{{/if}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(text.contains("Large context"));
    }

    #[test]
    fn test_numeric_comparison_lt() {
        let model = make_small_model();
        let mut text = "{{#if __max_output_tokens__ < 4096}}\nBe concise\n{{/if}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(text.contains("Be concise"));
    }

    #[test]
    fn test_numeric_comparison_fails() {
        let model = make_test_model();
        let mut text = "{{#if __max_input_tokens__ < 8000}}\nTiny context\n{{/if}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(!text.contains("Tiny context"));
    }

    #[test]
    fn test_string_equality() {
        let model = make_test_model();
        let mut text = "{{#if __model_client__ == openai}}\nOpenAI model\n{{/if}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(text.contains("OpenAI model"));
    }

    #[test]
    fn test_string_inequality() {
        let model = make_test_model();
        let mut text = "{{#if __model_client__ == anthropic}}\nClaude\n{{/if}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(!text.contains("Claude"));
    }

    #[test]
    fn test_mismatched_tags_pass_through() {
        let model = make_test_model();
        let mut text = "{{#if __supports_vision__}}\nContent\n{{/unless}}".to_string();
        let original = text.clone();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert_eq!(text, original);
    }

    #[test]
    fn test_unresolved_var_in_conditional_passes_through() {
        let mut text = "{{#if __model_id__}}\nContent\n{{/if}}".to_string();
        interpolate_variables_with_model(&mut text, None);
        assert!(text.contains("{{#if __model_id__}}"));
    }

    #[test]
    fn test_system_vars_still_work() {
        let mut text = "OS: {{__os__}}, Arch: {{__arch__}}".to_string();
        interpolate_variables_with_model(&mut text, None);
        assert!(!text.contains("{{__os__}}"));
        assert!(!text.contains("{{__arch__}}"));
    }

    #[test]
    fn test_mixed_system_and_model_vars() {
        let model = make_test_model();
        let mut text = "OS: {{__os__}}, Model: {{__model_name__}}".to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(!text.contains("{{__os__}}"));
        assert!(text.contains("gpt-4o"));
    }

    #[test]
    fn test_combined_conditionals_and_vars() {
        let model = make_test_model();
        let mut text = concat!(
            "Shell: {{__shell__}}\n",
            "{{#if __supports_vision__}}\n",
            "Model {{__model_name__}} supports vision.\n",
            "{{/if}}\n",
            "Done."
        ).to_string();
        interpolate_variables_with_model(&mut text, Some(&model));
        assert!(text.contains("gpt-4o supports vision."));
        assert!(text.contains("Done."));
        assert!(!text.contains("{{"));
    }
}
