use crate::client::Model;
use crate::config::{Config, Role, RoleLike};
use crate::function::FunctionDeclaration;

use anyhow::{bail, Result};

/// Pre-flight validation of model capabilities against what the role/input requires.
/// Runs before any API call; all checks are deterministic and zero-token.
///
/// Returns `Err` for hard mismatches (tools vs. non-function-calling model, images vs.
/// non-vision model). The caller should surface the error as a config error.
pub fn validate_model_capabilities(
    model: &Model,
    role: &Role,
    functions: Option<&[FunctionDeclaration]>,
    has_images: bool,
) -> Result<()> {
    let will_send_tools = functions.map(|f| !f.is_empty()).unwrap_or(false);
    if will_send_tools && !model.data().supports_function_calling {
        bail!(
            "Preflight: role '{}' requires tool calling but model '{}' does not support it. \
             Remove `use_tools` from the role or switch to a function-calling model.",
            role.name(),
            model.id()
        );
    }

    if has_images && !model.data().supports_vision {
        bail!(
            "Preflight: input contains images but model '{}' does not support vision. \
             Switch to a vision-capable model.",
            model.id()
        );
    }

    Ok(())
}

/// Pre-flight validation for a pipeline: each stage's role must exist and its model
/// (explicit or inherited) must support the role's requirements.
///
/// Output-schema/input-schema compatibility between stage N and stage N+1 is a
/// deterministic check we _could_ do here, but JSON-schema compatibility is subtle
/// (subset relations, anyOf/oneOf, etc.) — defer to schema validation at runtime
/// rather than duplicate the logic.
pub fn validate_pipeline_stages(
    config: &Config,
    stages: &[(String, Option<String>)],
) -> Result<()> {
    for (index, (role_name, model_id)) in stages.iter().enumerate() {
        let role = config.retrieve_role(role_name).map_err(|e| {
            anyhow::anyhow!(
                "Preflight: pipeline stage {} references unknown role '{}': {}",
                index + 1,
                role_name,
                e
            )
        })?;

        let model = match model_id {
            Some(id) => {
                let listed = crate::client::list_models(config, crate::client::ModelType::Chat);
                match listed.iter().find(|m| m.id() == *id) {
                    Some(m) => (*m).clone(),
                    None => bail!(
                        "Preflight: pipeline stage {} references unknown model '{}'",
                        index + 1,
                        id
                    ),
                }
            }
            None => role.model().clone(),
        };

        if role.use_tools().is_some() && !model.data().supports_function_calling {
            bail!(
                "Preflight: pipeline stage {} role '{}' requires tool calling but model \
                 '{}' does not support it",
                index + 1,
                role_name,
                model.id()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_with(tools: bool, vision: bool) -> Model {
        let mut m = Model::new("test", "m");
        m.data_mut().supports_function_calling = tools;
        m.data_mut().supports_vision = vision;
        m
    }

    fn one_tool() -> Vec<FunctionDeclaration> {
        vec![FunctionDeclaration::tool_search()]
    }

    #[test]
    fn passes_when_no_tools_and_no_images() {
        let m = model_with(false, false);
        let r = Role::default();
        assert!(validate_model_capabilities(&m, &r, None, false).is_ok());
    }

    #[test]
    fn rejects_tools_on_non_function_calling_model() {
        let m = model_with(false, false);
        let r = Role::default();
        let decls = one_tool();
        let err = validate_model_capabilities(&m, &r, Some(&decls), false).unwrap_err();
        assert!(err.to_string().contains("does not support it"));
    }

    #[test]
    fn accepts_tools_on_function_calling_model() {
        let m = model_with(true, false);
        let r = Role::default();
        let decls = one_tool();
        assert!(validate_model_capabilities(&m, &r, Some(&decls), false).is_ok());
    }

    #[test]
    fn rejects_images_on_non_vision_model() {
        let m = model_with(false, false);
        let r = Role::default();
        let err = validate_model_capabilities(&m, &r, None, true).unwrap_err();
        assert!(err.to_string().contains("does not support vision"));
    }

    #[test]
    fn accepts_images_on_vision_model() {
        let m = model_with(false, true);
        let r = Role::default();
        assert!(validate_model_capabilities(&m, &r, None, true).is_ok());
    }
}
