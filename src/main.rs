mod cli;
mod client;
mod config;
mod function;
mod mcp;
mod mcp_client;
mod pipe;
mod rag;
mod render;
mod repl;
mod serve;
#[macro_use]
mod utils;

#[macro_use]
extern crate log;

use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, call_react, list_models, ModelType,
};
use crate::config::{
    ensure_parent_exists, list_agents, load_env_file, macro_execute, run_lifecycle_hooks,
    validate_schema, Config, GlobalConfig, Input, RoleLike, WorkingMode, CODE_ROLE,
    EXPLAIN_SHELL_ROLE, SHELL_ROLE, TEMP_SESSION_NAME,
};
use crate::render::render_error;
use crate::repl::Repl;
use crate::utils::*;

use anyhow::{bail, Result};
use clap::Parser;
use inquire::Text;
use parking_lot::RwLock;
use simplelog::{format_description, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};
use std::{env, process, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    load_env_file()?;
    let cli = Cli::parse();
    // MCP mode uses stdin as transport — don't consume it here
    let text = if cli.mcp { None } else { cli.text()? };
    let working_mode = if cli.mcp {
        WorkingMode::Mcp
    } else if cli.serve.is_some() {
        WorkingMode::Serve
    } else if cli.mcp_server.is_some() {
        WorkingMode::Cmd
    } else if text.is_none() && cli.file.is_empty() {
        WorkingMode::Repl
    } else {
        WorkingMode::Cmd
    };
    let info_flag = cli.info
        || cli.sync_models
        || cli.list_models
        || cli.list_roles
        || cli.list_agents
        || cli.list_rags
        || cli.list_macros
        || cli.list_sessions
        || cli.list_tools;
    setup_logger(working_mode.is_serve() || working_mode.is_mcp())?;
    let config = Arc::new(RwLock::new(Config::init(working_mode, info_flag).await?));
    let output_format = cli.output_format;
    if let Err(err) = run(config, cli, text).await {
        let code = classify_error(&err);
        render_error(err, output_format, code);
        std::process::exit(code.as_i32());
    }
    Ok(())
}

async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    if cli.mcp {
        return mcp::run(config).await;
    }

    if let Some(ref server_cmd) = cli.mcp_server {
        return mcp_client::run_mcp_client_command(&cli, server_cmd).await;
    }

    if cli.pipe {
        return pipe::run(config, cli, text).await;
    }

    let abort_signal = create_abort_signal();

    if cli.sync_models {
        let url = config.read().sync_models_url();
        return Config::sync_models(&url, abort_signal.clone()).await;
    }

    if cli.list_models {
        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
            let models: Vec<serde_json::Value> = list_models(&config.read(), ModelType::Chat)
                .iter()
                .map(|m| serde_json::json!({ "id": m.id() }))
                .collect();
            println!("{}", serde_json::to_string_pretty(&models)?);
        } else {
            for model in list_models(&config.read(), ModelType::Chat) {
                println!("{}", model.id());
            }
        }
        return Ok(());
    }
    if cli.list_roles {
        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
            let roles = Config::all_roles();
            let json_roles: Vec<serde_json::Value> = roles
                .iter()
                .map(|r| {
                    let tools_str = r.use_tools().unwrap_or_default();
                    let tools: Vec<&str> = tools_str
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect();
                    serde_json::json!({
                        "name": r.name(),
                        "description": r.description_or_derived(),
                        "model": r.model_id().unwrap_or("default"),
                        "tools": tools,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_roles)?);
        } else {
            let roles = Config::list_roles(true).join("\n");
            println!("{roles}");
        }
        return Ok(());
    }
    if cli.list_agents {
        let agents = list_agents().join("\n");
        println!("{agents}");
        return Ok(());
    }
    if cli.list_rags {
        let rags = Config::list_rags().join("\n");
        println!("{rags}");
        return Ok(());
    }
    if cli.list_macros {
        let macros = Config::list_macros().join("\n");
        println!("{macros}");
        return Ok(());
    }
    if cli.list_tools {
        // --list-tools without --mcp-server: list tools from config-based MCP servers
        let cfg = config.read();
        let decls: Vec<_> = cfg
            .functions
            .declarations()
            .iter()
            .filter(|d| matches!(d.source, crate::function::ToolSource::Mcp { .. }))
            .collect();
        if decls.is_empty() {
            println!("No MCP tools configured.");
        } else {
            match cli.output_format {
                Some(crate::cli::OutputFormat::Json) => {
                    let json: Vec<serde_json::Value> = decls
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "name": d.name,
                                "description": d.description,
                                "parameters": d.parameters,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
                }
                _ => {
                    for d in &decls {
                        if d.description.is_empty() {
                            println!("{}", d.name);
                        } else {
                            println!("{} - {}", d.name, d.description);
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    if cli.dry_run {
        config.write().dry_run = true;
    }

    if let Some(agent) = &cli.agent {
        let session = cli.session.as_ref().map(|v| match v {
            Some(v) => v.as_str(),
            None => TEMP_SESSION_NAME,
        });
        if !cli.agent_variable.is_empty() {
            config.write().agent_variables = Some(
                cli.agent_variable
                    .chunks(2)
                    .map(|v| (v[0].to_string(), v[1].to_string()))
                    .collect(),
            );
        }

        let ret = Config::use_agent(&config, agent, session, abort_signal.clone()).await;
        config.write().agent_variables = None;
        ret?;
    } else {
        if !cli.variable.is_empty() {
            config.write().role_variables = Some(
                cli.variable
                    .iter()
                    .filter_map(|v| {
                        v.split_once('=')
                            .map(|(k, val)| (k.to_string(), val.to_string()))
                    })
                    .collect(),
            );
        }
        if let Some(prompt) = &cli.prompt {
            config.write().use_prompt(prompt)?;
        } else if let Some(name) = &cli.role {
            config.write().use_role(name)?;
        } else if cli.execute {
            config.write().use_role(SHELL_ROLE)?;
        } else if cli.code {
            config.write().use_role(CODE_ROLE)?;
        }
        if let Some(session) = &cli.session {
            config
                .write()
                .use_session(session.as_ref().map(|v| v.as_str()))?;
        }
        if let Some(rag) = &cli.rag {
            Config::use_rag(&config, Some(rag), abort_signal.clone()).await?;
        }
        config.write().role_variables = None;
    }
    if cli.list_sessions {
        let sessions = config.read().list_sessions().join("\n");
        println!("{sessions}");
        return Ok(());
    }
    if let Some(model_id) = &cli.model {
        config.write().set_model(model_id)?;
    }
    if cli.no_stream {
        config.write().stream = false;
    }
    if let Some(fmt) = cli.output_format {
        config.write().output_format = Some(fmt);
        if fmt.is_structured() {
            config.write().stream = false;
        }
    }
    if cli.empty_session {
        config.write().empty_session()?;
    }
    if cli.save_session {
        config.write().set_save_session_this_time()?;
    }
    if cli.info {
        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
            let cfg = config.read();
            let mut info = serde_json::Map::new();
            info.insert("model".into(), serde_json::json!(cfg.current_model().id()));
            if let Some(role) = cfg.role.as_ref() {
                info.insert("role".into(), serde_json::json!(role.name()));
                info.insert(
                    "description".into(),
                    serde_json::json!(role.description_or_derived()),
                );
                if let Some(tools) = role.use_tools() {
                    info.insert("tools".into(), serde_json::json!(tools));
                }
                info.insert(
                    "prompt_length".into(),
                    serde_json::json!(role.prompt().len()),
                );
            }
            if let Some(temp) = cfg.temperature {
                info.insert("temperature".into(), serde_json::json!(temp));
            }
            info.insert("stream".into(), serde_json::json!(cfg.stream));
            drop(cfg);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::Value::Object(info))?
            );
        } else {
            let info = config.read().info()?;
            println!("{info}");
        }
        return Ok(());
    }
    if let Some(addr) = cli.serve {
        return serve::run(config, addr).await;
    }
    let is_repl = config.read().working_mode.is_repl();
    if cli.rebuild_rag {
        Config::rebuild_rag(&config, abort_signal.clone()).await?;
        if is_repl {
            return Ok(());
        }
    }
    if let Some(name) = &cli.macro_name {
        macro_execute(&config, name, text.as_deref(), abort_signal.clone()).await?;
        return Ok(());
    }
    if cli.execute && !is_repl {
        let input = create_input(&config, text, &cli.file, abort_signal.clone()).await?;
        shell_execute(&config, &SHELL, input, abort_signal.clone()).await?;
        return Ok(());
    }
    config.write().apply_prelude()?;
    match is_repl {
        false => {
            let mut input = create_input(&config, text, &cli.file, abort_signal.clone()).await?;
            input.use_embeddings(abort_signal.clone()).await?;
            start_directive(&config, input, cli.code, abort_signal).await
        }
        true => {
            if !*IS_STDOUT_TERMINAL {
                bail!("No TTY for REPL")
            }
            start_interactive(&config).await
        }
    }
}

#[async_recursion::async_recursion]
async fn start_directive(
    config: &GlobalConfig,
    mut input: Input,
    _code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<()> {
    if let Some(schema) = input.role().input_schema() {
        validate_schema("input", schema, &input.text())?;
    }

    let has_output_schema = input.role().output_schema().cloned();
    let output_format = config.read().output_format;
    let is_dry_run = config.read().dry_run;

    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;
    let (output, tool_results) =
        call_react(&mut input, client.as_ref(), abort_signal.clone()).await?;

    // Structured output needs explicit printing since call_react suppresses it.
    // In dry_run mode, just print the echoed prompt — no validation.
    if is_dry_run {
        if has_output_schema.is_some() || output_format.map(|f| f.is_structured()).unwrap_or(false)
        {
            print!("{output}");
            std::io::Write::flush(&mut std::io::stdout())?;
            if !output.ends_with('\n') {
                println!();
            }
        }
    } else if let Some(ref schema) = has_output_schema {
        validate_schema("output", schema, &output)?;
        print!("{output}");
        std::io::Write::flush(&mut std::io::stdout())?;
        if !output.ends_with('\n') {
            println!();
        }
    } else if let Some(fmt) = output_format {
        if fmt.is_structured() {
            let cleaned = fmt.clean_output(&output)?;
            print!("{cleaned}");
            std::io::Write::flush(&mut std::io::stdout())?;
            if !cleaned.ends_with('\n') {
                println!();
            }
        }
    }

    // Phase 6B: Run lifecycle hooks (pipe_to, save_to)
    if !is_dry_run {
        run_lifecycle_hooks(input.role(), &output)?;
    }

    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;

    config.write().exit_session()?;
    Ok(())
}

async fn start_interactive(config: &GlobalConfig) -> Result<()> {
    let mut repl: Repl = Repl::init(config)?;
    repl.run().await
}

#[async_recursion::async_recursion]
async fn shell_execute(
    config: &GlobalConfig,
    shell: &Shell,
    mut input: Input,
    abort_signal: AbortSignal,
) -> Result<()> {
    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;
    let (eval_str, _) =
        call_chat_completions(&input, false, true, client.as_ref(), abort_signal.clone()).await?;

    config
        .write()
        .after_chat_completion(&input, &eval_str, &[])?;
    if eval_str.is_empty() {
        bail!("No command generated");
    }
    if config.read().dry_run {
        config.read().print_markdown(&eval_str)?;
        return Ok(());
    }
    if *IS_STDOUT_TERMINAL {
        let options = ["execute", "revise", "describe", "copy", "quit"];
        let command = color_text(eval_str.trim(), nu_ansi_term::Color::Rgb(255, 165, 0));
        let first_letter_color = nu_ansi_term::Color::Cyan;
        let prompt_text = options
            .iter()
            .map(|v| format!("{}{}", color_text(&v[0..1], first_letter_color), &v[1..]))
            .collect::<Vec<String>>()
            .join(&dimmed_text(" | "));
        loop {
            println!("{command}");
            let answer_char =
                read_single_key(&['e', 'r', 'd', 'c', 'q'], 'e', &format!("{prompt_text}: "))?;

            match answer_char {
                'e' => {
                    debug!("{} {:?}", shell.cmd, &[&shell.arg, &eval_str]);
                    let code = run_command(&shell.cmd, &[&shell.arg, &eval_str], None)?;
                    if code == 0 && config.read().save_shell_history {
                        let _ = append_to_shell_history(&shell.name, &eval_str, code);
                    }
                    process::exit(code);
                }
                'r' => {
                    let revision = Text::new("Enter your revision:").prompt()?;
                    let text = format!("{}\n{revision}", input.text());
                    input.set_text(text);
                    return shell_execute(config, shell, input, abort_signal.clone()).await;
                }
                'd' => {
                    let role = config.read().retrieve_role(EXPLAIN_SHELL_ROLE)?;
                    let input = Input::from_str(config, &eval_str, Some(role));
                    if input.stream() {
                        call_chat_completions_streaming(
                            &input,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?;
                    } else {
                        call_chat_completions(
                            &input,
                            true,
                            false,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?;
                    }
                    println!();
                    continue;
                }
                'c' => {
                    set_text(&eval_str)?;
                    println!("{}", dimmed_text("✓ Copied the command."));
                }
                _ => {}
            }
            break;
        }
    } else {
        println!("{eval_str}");
    }
    Ok(())
}

async fn create_input(
    config: &GlobalConfig,
    text: Option<String>,
    file: &[String],
    abort_signal: AbortSignal,
) -> Result<Input> {
    let input = if file.is_empty() {
        Input::from_str(config, &text.unwrap_or_default(), None)
    } else {
        Input::from_files_with_spinner(
            config,
            &text.unwrap_or_default(),
            file.to_vec(),
            None,
            abort_signal,
        )
        .await?
    };
    if input.is_empty() {
        bail!("No input");
    }
    Ok(input)
}

fn setup_logger(is_serve: bool) -> Result<()> {
    let (log_level, log_path) = Config::log_config(is_serve)?;
    if log_level == LevelFilter::Off {
        return Ok(());
    }
    let crate_name = env!("CARGO_CRATE_NAME");
    let log_filter = match std::env::var(get_env_name("log_filter")) {
        Ok(v) => v,
        Err(_) => match is_serve {
            true => format!("{crate_name}::serve"),
            false => crate_name.into(),
        },
    };
    let config = ConfigBuilder::new()
        .add_filter_allow(log_filter)
        .set_time_format_custom(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .set_thread_level(LevelFilter::Off)
        .build();
    match log_path {
        None => {
            SimpleLogger::init(log_level, config)?;
        }
        Some(log_path) => {
            ensure_parent_exists(&log_path)?;
            let log_file = std::fs::File::create(log_path)?;
            WriteLogger::init(log_level, config, log_file)?;
        }
    }
    Ok(())
}
