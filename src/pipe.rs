use crate::cli::Cli;
use crate::client::{call_chat_completions, call_chat_completions_streaming};
use crate::config::{validate_schema, Config, GlobalConfig, Input};
use crate::utils::*;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

struct PipelineStage {
    role_name: String,
    model_id: Option<String>,
}

#[derive(Deserialize)]
struct PipelineDef {
    #[serde(default)]
    stages: Vec<PipelineStageDef>,
}

#[derive(Deserialize)]
struct PipelineStageDef {
    role: String,
    model: Option<String>,
}

pub async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    let stages = if let Some(def_path) = &cli.pipe_def {
        load_pipeline_def(def_path)?
    } else if !cli.stages.is_empty() {
        parse_stages(&cli.stages)?
    } else {
        bail!("Pipeline requires --stage or --pipe-def");
    };

    if stages.is_empty() {
        bail!("Pipeline has no stages");
    }

    let mut input_text = match text {
        Some(t) => t,
        None if !cli.file.is_empty() => {
            let abort_signal = create_abort_signal();
            let input = Input::from_files_with_spinner(
                &config,
                "",
                cli.file.clone(),
                None,
                abort_signal,
            )
            .await?;
            input.text().to_string()
        }
        None => bail!("Pipeline requires input text or files (-f)"),
    };

    let abort_signal = create_abort_signal();

    for (i, stage) in stages.iter().enumerate() {
        let is_last = i == stages.len() - 1;
        input_text =
            run_stage(&config, stage, &input_text, is_last, abort_signal.clone()).await?;
    }

    Ok(())
}

async fn run_stage(
    config: &GlobalConfig,
    stage: &PipelineStage,
    input_text: &str,
    is_last: bool,
    abort_signal: AbortSignal,
) -> Result<String> {
    let role = config
        .read()
        .retrieve_role(&stage.role_name)
        .with_context(|| format!("Failed to load role '{}' for pipeline stage", stage.role_name))?;

    if let Some(model_id) = &stage.model_id {
        config.write().set_model(model_id)?;
    }

    if let Some(schema) = role.input_schema() {
        validate_schema("input", schema, input_text)?;
    }

    let input = Input::from_str(config, input_text, Some(role.clone()));
    let client = input.create_client()?;

    config.write().before_chat_completion(&input)?;

    let (output, tool_results) = if input.stream() && is_last {
        call_chat_completions_streaming(&input, client.as_ref(), abort_signal).await?
    } else {
        call_chat_completions(&input, false, false, client.as_ref(), abort_signal).await?
    };

    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;

    if let Some(schema) = role.output_schema() {
        validate_schema("output", schema, &output)?;
    }

    if is_last && !input.stream() {
        print!("{output}");
        std::io::Write::flush(&mut std::io::stdout())?;
        if !output.ends_with('\n') {
            println!();
        }
    }

    Ok(output)
}

fn parse_stages(stage_specs: &[String]) -> Result<Vec<PipelineStage>> {
    stage_specs
        .iter()
        .map(|spec| {
            let (role_name, model_id) = match spec.split_once('@') {
                Some((role, model)) => (role.to_string(), Some(model.to_string())),
                None => (spec.to_string(), None),
            };
            Ok(PipelineStage { role_name, model_id })
        })
        .collect()
}

fn load_pipeline_def(path: &str) -> Result<Vec<PipelineStage>> {
    let path = Path::new(path);
    let content = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read pipeline definition: {}", path.display()))?
    } else {
        let pipelines_dir = Config::local_path("pipelines");
        let full_path = pipelines_dir.join(format!("{}.yaml", path.display()));
        if full_path.exists() {
            std::fs::read_to_string(&full_path).with_context(|| {
                format!(
                    "Failed to read pipeline definition: {}",
                    full_path.display()
                )
            })?
        } else {
            bail!(
                "Pipeline definition not found: {} or {}",
                path.display(),
                full_path.display()
            );
        }
    };

    let def: PipelineDef =
        serde_yaml::from_str(&content).context("Failed to parse pipeline definition YAML")?;

    Ok(def
        .stages
        .into_iter()
        .map(|s| PipelineStage {
            role_name: s.role,
            model_id: s.model,
        })
        .collect())
}
