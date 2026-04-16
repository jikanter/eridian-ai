use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, call_react, CallMetrics,
};
use crate::config::{
    run_lifecycle_hooks, validate_schema_traced, Config, GlobalConfig, Input, RoleLike,
    RolePipelineStage,
};
use crate::utils::*;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

struct PipelineStage {
    role_name: String,
    model_id: Option<String>,
}

#[derive(Serialize)]
struct StageTrace {
    role: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
    latency_ms: u64,
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

    // Phase 9D: pre-flight validate every stage's role/model before any LLM call
    {
        let stage_tuples: Vec<(String, Option<String>)> = stages
            .iter()
            .map(|s| (s.role_name.clone(), s.model_id.clone()))
            .collect();
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
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
    let output_format = cli.output_format;

    let stage_count = stages.len();
    let mut stage_traces: Vec<StageTrace> = Vec::new();
    for (i, stage) in stages.iter().enumerate() {
        let is_last = i == stage_count - 1;
        let (output, metrics) = run_stage(
            &config,
            stage,
            i,
            stage_count,
            &input_text,
            is_last,
            abort_signal.clone(),
        )
        .await?;
        stage_traces.push(StageTrace {
            role: stage.role_name.clone(),
            model: metrics.model_id.clone(),
            input_tokens: metrics.input_tokens,
            output_tokens: metrics.output_tokens,
            cost_usd: metrics.cost_usd,
            latency_ms: metrics.latency_ms,
        });
        input_text = output;
    }

    // JSON envelope with trace metadata when output format is JSON
    if matches!(output_format, Some(crate::cli::OutputFormat::Json)) {
        let total_cost: f64 = stage_traces.iter().map(|s| s.cost_usd).sum();
        let total_latency: u64 = stage_traces.iter().map(|s| s.latency_ms).sum();
        let envelope = serde_json::json!({
            "output": serde_json::from_str::<serde_json::Value>(&input_text).unwrap_or(serde_json::Value::String(input_text)),
            "trace": {
                "stages": stage_traces,
                "total_cost_usd": total_cost,
                "total_latency_ms": total_latency,
            }
        });
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    }

    Ok(())
}

async fn run_stage(
    config: &GlobalConfig,
    stage: &PipelineStage,
    stage_index: usize,
    stage_count: usize,
    input_text: &str,
    is_last: bool,
    abort_signal: AbortSignal,
) -> Result<(String, CallMetrics)> {
    // Phase 0C: Save model state for restoration after stage
    let saved_model_id = config.read().current_model().id();

    let result = run_stage_inner(config, stage, input_text, is_last, abort_signal).await;

    // Phase 0C: Restore model state regardless of success/failure
    if let Err(e) = config.write().set_model(&saved_model_id) {
        debug!("Failed to restore model after pipeline stage: {e}");
    }

    result.map_err(|e| {
        let model_id = stage.model_id.clone().unwrap_or_else(|| {
            config.read().current_model().id()
        });
        anyhow::Error::new(AichatError::PipelineStage {
            stage: stage_index + 1,
            total: stage_count,
            role_name: stage.role_name.clone(),
            model_id: Some(model_id),
            message: e.to_string(),
        })
    })
}

async fn run_stage_inner(
    config: &GlobalConfig,
    stage: &PipelineStage,
    input_text: &str,
    is_last: bool,
    abort_signal: AbortSignal,
) -> Result<(String, CallMetrics)> {
    let role = config
        .read()
        .retrieve_role(&stage.role_name)
        .with_context(|| format!("Failed to load role '{}' for pipeline stage", stage.role_name))?;

    if let Some(model_id) = &stage.model_id {
        config.write().set_model(model_id)?;
    }

    let trace_emitter = config
        .read()
        .trace_config
        .clone()
        .map(crate::utils::trace::TraceEmitter::new);

    if let Some(schema) = role.input_schema() {
        validate_schema_traced("input", schema, input_text, trace_emitter.as_ref())?;
    }

    let has_tools = role.use_tools().is_some();
    let mut input = Input::from_str(config, input_text, Some(role.clone()));
    let client = input.create_client()?;

    config.write().before_chat_completion(&input)?;

    // Phase 0B: Use call_react when the stage role has tools
    let (output, tool_results, metrics) = if has_tools {
        call_react(&mut input, client.as_ref(), abort_signal).await?
    } else if input.stream() && is_last {
        call_chat_completions_streaming(&input, client.as_ref(), abort_signal).await?
    } else {
        call_chat_completions(&input, false, false, client.as_ref(), abort_signal).await?
    };

    // Only save to message history for the last stage
    if is_last {
        config
            .write()
            .after_chat_completion(&input, &output, &tool_results)?;
    }

    if let Some(schema) = role.output_schema() {
        validate_schema_traced("output", schema, &output, trace_emitter.as_ref())?;
    }

    if is_last && !input.stream() {
        let final_output = if let Some(fmt) = config.read().output_format {
            if fmt.is_structured() {
                fmt.clean_output(&output)?
            } else {
                output.to_string()
            }
        } else {
            output.to_string()
        };
        print!("{final_output}");
        std::io::Write::flush(&mut std::io::stdout())?;
        if !final_output.ends_with('\n') {
            println!();
        }
    }

    // Phase 6B: Run lifecycle hooks on the last stage
    if is_last {
        run_lifecycle_hooks(&role, &output)?;
    }

    // Strip think tags from intermediate output
    let output = if !is_last {
        strip_think_tag(&output).to_string()
    } else {
        output
    };
    Ok((output, metrics))
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

/// Run a pipeline defined in a role's frontmatter. Called from tool dispatch.
/// Returns the final output text.
pub async fn run_pipeline_role(
    config: &GlobalConfig,
    stages: &[RolePipelineStage],
    input_text: &str,
) -> Result<String> {
    if stages.is_empty() {
        bail!("Pipeline role has no stages");
    }

    let pipeline_stages: Vec<PipelineStage> = stages
        .iter()
        .map(|s| PipelineStage {
            role_name: s.role.clone(),
            model_id: s.model.clone(),
        })
        .collect();

    let abort_signal = create_abort_signal();
    let stage_count = pipeline_stages.len();
    let mut current_input = input_text.to_string();

    for (i, stage) in pipeline_stages.iter().enumerate() {
        let (output, _metrics) = run_stage(
            config,
            stage,
            i,
            stage_count,
            &current_input,
            // For pipeline-as-tool, never print output (it's returned to the caller)
            false,
            abort_signal.clone(),
        )
        .await?;
        current_input = output;
    }

    Ok(current_input)
}
