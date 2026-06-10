mod cache;
mod cli;
mod client;
mod compare;
mod config;
mod context_budget;
mod discovery;
mod function;
mod knowledge;
mod mcp;
mod memory;
mod mcp_client;
mod pipe;
mod rag;
mod render;
mod repl;
mod serve;
mod strip_thinking;
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
    validate_schema, validate_schema_traced, Config, GlobalConfig, Input, Entity, WorkingMode, CODE_ROLE,
    EXPLAIN_SHELL_ROLE, SHELL_ROLE, TEMP_SESSION_NAME,
};
use crate::render::render_error;
use crate::repl::Repl;
use crate::utils::*;

use anyhow::{bail, Context, Result};
use clap::Parser;
use inquire::Text;
use parking_lot::RwLock;
use simplelog::{format_description, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};
use std::{env, process, sync::Arc};

fn main() -> Result<()> {
    // Load the env file while the process is still single-threaded, before the
    // tokio runtime spawns any worker thread. load_env_file() mutates the
    // process environment via set_var, which is only sound with no other
    // threads running — so the runtime must be built *after* it returns.
    load_env_file()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<()> {
    let cli = Cli::parse();

    // Phase 31E: validate a portable mcp.json declarations file. Runs before
    // any aichat config load so a broken config.yaml doesn't mask validation
    // results. Exits with the right code from inside the helper.
    if let Some(ref path_arg) = cli.validate_mcp_config {
        let exit = mcp_client::run_validate_mcp_config(
            path_arg.as_deref(),
            cli.output_format,
        );
        process::exit(exit);
    }

    // Phase 3: one-shot session conversion. Like --validate-mcp-config it
    // short-circuits before the full config init so a broken config.yaml
    // can't mask the conversion path. We load the session through a
    // minimal Config (no models, no agents) since we only need the YAML
    // schema to deserialize, not the runtime model.
    if let Some(ref src) = cli.convert_session {
        let exit = run_convert_session(
            src,
            &cli.convert_to,
            cli.convert_out.as_deref(),
        );
        process::exit(exit);
    }

    // One-shot bulk migration of legacy YAML sessions to pi JSONL. Short-
    // circuits before full config init for the same reason as the converter
    // above: it only needs the sessions directory, not a live model client.
    if cli.migrate_sessions {
        let exit = run_migrate_sessions();
        process::exit(exit);
    }

    // MCP mode uses stdin as transport — don't consume it here. The memory
    // write/reflect subcommands (Phase 34C/D) likewise own stdin: the curator
    // reads accept/skip decisions and the Reflector reads a transcript from it.
    let owns_stdin = cli.mcp || cli.memory_reflect || cli.memory_curate;
    let text = if owns_stdin { None } else { cli.text()? };
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
        || cli.find_role
        || cli.list_prompts
        || cli.list_agents
        || cli.list_rags
        || cli.list_macros
        || cli.list_sessions
        || cli.list_tools
        // Phase 13A/D: --fork-role writes a file and exits; --explain-role
        // reads role frontmatter and exits. Neither needs an LLM client.
        || !cli.fork_role.is_empty()
        || cli.explain_role.is_some()
        // Phase 25E/26E: read-only knowledge ops don't need heavy config setup.
        || cli.knowledge_list
        || cli.knowledge_stat.is_some()
        || cli.knowledge_show.is_some()
        || cli.knowledge_search.is_some()
        // Phase 27B: reflect prints a candidate set and exits; curate mutates
        // a KB but still short-circuits the interactive path.
        || cli.knowledge_reflect.is_some()
        || cli.knowledge_curate.is_some()
        // Phase 34B/C/D: memory load/reflect/curate short-circuit and exit;
        // they need no heavy client setup (mirrors the knowledge ops above).
        || cli.memory_load.is_some()
        || cli.memory_reflect
        || cli.memory_curate;
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

/// Apply CLI/env runtime flags onto the shared config. Must run before any
/// execution short-circuit (notably `--pipe`) so every path observes
/// `--dry-run`, `--trace`, `--cost`, `--no-cache`, and `--knowledge`.
fn apply_runtime_flags(config: &GlobalConfig, cli: &Cli) {
    if cli.no_cache {
        config.write().no_cache = true;
    }
    // Phase 26D: CLI `--knowledge` bindings merge with role-declared ones at
    // retrieval time. Captured here so pipeline stages see them too.
    if !cli.knowledge.is_empty() {
        config.write().cli_knowledge_bindings = cli.knowledge.clone();
    }
    if cli.dry_run {
        config.write().dry_run = true;
    }
    // Phase 34C: opt-in session-exit Reflector trigger. Also honour the env var
    // AICHAT_MEMORY_REFLECT_ON_EXIT so it can be set without a CLI flag.
    let env_reflect_on_exit = std::env::var(get_env_name("memory_reflect_on_exit"))
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    if cli.memory_reflect_on_exit || env_reflect_on_exit {
        config.write().memory_reflect_on_exit = true;
    }
    if cli.cost {
        config.write().show_cost = true;
    }
    // Run log from env var AICHAT_RUN_LOG
    if let Ok(log_path) = std::env::var(get_env_name("run_log")) {
        config.write().run_log = Some(log_path);
    }
    // Phase 23D: per-role invocation ledger directory from AICHAT_ROLE_LEDGER
    if let Ok(dir) = std::env::var(get_env_name("role_ledger")) {
        config.write().role_ledger_dir = Some(dir);
    }
    // Trace config from --trace flag or AICHAT_TRACE env var
    let env_trace = std::env::var(get_env_name("trace"))
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    if cli.trace || env_trace {
        let trace_file = std::env::var(get_env_name("trace_file"))
            .ok()
            .map(std::path::PathBuf::from);
        let jsonl_trace = env_trace || trace_file.is_some();
        config.write().trace_config = Some(crate::utils::trace::TraceConfig {
            human_trace: cli.trace,
            jsonl_trace,
            jsonl_file: trace_file,
            truncate_at: 500,
        });

        // Phase 42D: enable the SPEC-001 keystone emitter under the same
        // `--trace` / `AICHAT_TRACE` opt-in. (SPEC-001 §1's default-on is a
        // user-facing behavior change held back pending explicit opt-in.)
        let base_dir = crate::utils::trace_spec::layout::TraceLayout::from_env()
            .base()
            .to_path_buf();
        config.write().spec_trace = Some(crate::utils::trace_spec::wiring::SpecTraceConfig {
            base_dir,
            parent_session_id: std::env::var("AICHAT_PARENT_SESSION_ID").ok(),
            fixture_id: std::env::var("AICHAT_FIXTURE_ID").ok(),
        });
    }
}

async fn run(config: GlobalConfig, mut cli: Cli, text: Option<String>) -> Result<()> {
    if cli.mcp {
        return mcp::run(config).await;
    }

    if let Some(ref server_cmd) = cli.mcp_server {
        return mcp_client::run_mcp_client_command(&cli, server_cmd).await;
    }

    // Phase 15C: `--check` validates a role/pipeline definition without
    // executing it. Like the other read-only short-circuits it owns its own
    // exit code (0 valid, 3 invalid, 2 usage) and prints its own report.
    if cli.check {
        let code = pipe::run_check(&config, &cli).await?;
        process::exit(code);
    }

    // Phase 19B: unified `-r` resolution. If the name given to `-r` resolves
    // to an agent or macro instead of a role (or carries an `agent:` /
    // `macro:` prefix), reroute to the matching dispatch slot. `-a` and
    // `--macro` remain authoritative — never override an explicitly-set slot.
    if cli.role.is_some() && cli.agent.is_none() && cli.macro_name.is_none() {
        let role_name = cli.role.clone().unwrap();
        let has_prefix = role_name.starts_with("agent:")
            || role_name.starts_with("macro:")
            || role_name.starts_with("remote:")
            || role_name.starts_with("mcp:");
        match config.read().classify_entity(&role_name) {
            Ok(config::EntityRef::Role(_)) => {}
            Ok(config::EntityRef::Agent(name)) => {
                cli.role = None;
                cli.agent = Some(name);
            }
            Ok(config::EntityRef::Macro(name)) => {
                cli.role = None;
                cli.macro_name = Some(name);
            }
            Ok(config::EntityRef::Remote { target, role }) => {
                // Phase 20A: `aichat -r remote:NAME/role "input"` — invoke a
                // remote role directly from the CLI. We dispatch via the
                // pipeline runner (single Remote stage) so token metrics
                // and the X-AIChat-Cost wiring stay consistent with the
                // pipeline path. Keep the rest of `cli` untouched so flags
                // like `-o json` still apply on the way out.
                let input_text = text.clone().unwrap_or_default();
                let stages = vec![crate::pipe::InlineStage {
                    role: format!("remote:{target}/{role}"),
                    model: None,
                }];
                let result = crate::pipe::run_inline_pipeline(
                    &config,
                    &stages,
                    &input_text,
                    create_abort_signal(),
                )
                .await?;
                print!("{}", result.output);
                if !result.output.ends_with('\n') {
                    println!();
                }
                return Ok(());
            }
            Err(e) if has_prefix => {
                // The user's explicit prefix told us exactly what they meant;
                // surface the precise classification error rather than letting
                // `use_role` mask it with a generic "unknown role" message.
                return Err(e);
            }
            Err(_) => {
                // Bare name with no prefix: fall through to `use_role`, which
                // will raise its own role-flavored error if the file isn't
                // there. Preserves the legacy code path.
            }
        }
    }

    // Apply global runtime flags (dry-run, trace, cost, no-cache, knowledge)
    // up front so every execution path — including the `--pipe` short-circuit
    // below — observes them. They previously lived after this return, so
    // `--pipe --dry-run` and `--pipe --trace` were silently dropped.
    apply_runtime_flags(&config, &cli);

    // Phase 23B: `--compare ROLE1 ROLE2` runs the same input through two roles
    // and prints a side-by-side comparison (or a JSON object under `-o json`).
    if cli.compare.len() == 2 {
        let input_text = text.clone().unwrap_or_default();
        return run_compare(&config, &cli.compare, &input_text, cli.output_format).await;
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
        // Phase 14D: an optional `--capability` filter narrows the list.
        let roles = match cli.capability.as_deref() {
            Some(cap) => Config::find_roles_by_capability(cap),
            None => Config::all_roles(),
        };
        render_role_list(&roles, cli.verbose, cli.output_format)?;
        return Ok(());
    }
    // Phase 13A: --fork-role <SOURCE> <NEW_NAME>. Creates a new role file in
    // the user's roles directory pre-populated with `extends: <SOURCE>` and
    // commented-out parent fields so the user can edit them in place.
    if !cli.fork_role.is_empty() {
        return run_fork_role(&cli.fork_role, cli.output_format);
    }
    // Phase 13D: --explain-role <NAME>. Prints a human-readable description
    // of what the role does and how it composes. Pair with `-o json` for
    // machine consumption.
    if let Some(name) = &cli.explain_role {
        return run_explain_role(name, cli.output_format);
    }
    if cli.find_role {
        // At least one filter must be present, otherwise --find-role is just a
        // less-friendly --list-roles.
        if cli.capability.is_none() && cli.accepts.is_none() && cli.produces.is_none() {
            bail!(
                "--find-role requires at least one of --capability, --accepts, --produces"
            );
        }
        // Apply capability filter first (if any), then port filters on the
        // remaining set. find_roles_by_port takes the universe via all_roles,
        // so do capability narrowing first by filtering by name afterwards.
        let mut roles = if let Some(cap) = cli.capability.as_deref() {
            Config::find_roles_by_capability(cap)
        } else {
            Config::all_roles()
        };
        if cli.accepts.is_some() || cli.produces.is_some() {
            let accepts = cli.accepts.as_deref();
            let produces = cli.produces.as_deref();
            roles.retain(|r| {
                accepts.is_none_or(|t| r.port_accepts(t))
                    && produces.is_none_or(|t| r.port_produces(t))
            });
        }
        render_role_list(&roles, cli.verbose, cli.output_format)?;
        return Ok(());
    }
    if cli.list_prompts {
        if matches!(cli.output_format, Some(crate::cli::OutputFormat::Json)) {
            let prompts = Config::all_prompts();
            println!("{}", serde_json::to_string_pretty(&prompts)?);
        } else {
            let prompts = Config::list_prompts().join("\n");
            println!("{prompts}");
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
    // Phase 25E/26E: knowledge subsystem CLI dispatch. These flags
    // short-circuit the main interactive path. Compile is the only one that
    // talks to an LLM.
    if cli.knowledge_list {
        return knowledge::run_list();
    }
    if let Some(ref kb) = cli.knowledge_stat {
        return knowledge::run_stat(kb);
    }
    if let Some(ref spec) = cli.knowledge_show {
        return knowledge::run_show(spec);
    }
    if let Some(ref kb_name) = cli.knowledge_compile {
        return knowledge::run_compile(&config, kb_name, &cli.file).await;
    }
    if let Some(ref query_text) = cli.knowledge_search {
        return knowledge::run_search(&cli.knowledge, query_text, cli.output_format);
    }
    if let Some(ref kb_name) = cli.knowledge_reflect {
        return knowledge::run_reflect(&config, kb_name, cli.knowledge_trace.as_deref()).await;
    }
    if let Some(ref kb_name) = cli.knowledge_curate {
        return knowledge::run_curate(
            &config,
            kb_name,
            cli.knowledge_candidates.as_deref(),
            cli.knowledge_trace.as_deref(),
        )
        .await;
    }
    // Phase 34B/C/D: freeform memory write + lazy-load surface. Like the
    // knowledge subcommands these short-circuit the interactive path.
    if let Some(ref reference) = cli.memory_load {
        match memory::load_topic(reference) {
            Some((_, text)) => {
                println!("{text}");
                return Ok(());
            }
            None => bail!("memory: no topic resolves for reference '{reference}'"),
        }
    }
    if cli.memory_reflect {
        return memory::reflect::run_reflect_cli(&config, cli.memory_transcript.as_deref()).await;
    }
    if cli.memory_curate {
        return memory::curate::run_curate_cli(
            &config,
            cli.memory_transcript.as_deref(),
            cli.memory_candidates.as_deref(),
            cli.memory_auto_curate,
        )
        .await;
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
    if cli.strip_thinking {
        config.write().strip_thinking = true;
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
            // Phase 34A: surface the cached memory/MEMORY.md preamble so the
            // auto-memory read surface is observable without a model call.
            if let Some(mem) = cfg.memory_preamble.as_ref() {
                info.insert("memory_preamble".into(), serde_json::json!(mem));
            }
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
    if cli.each {
        return batch_execute(&config, &cli, text, abort_signal).await;
    }
    match is_repl {
        false => {
            let mut input = create_input(&config, text, &cli.file, abort_signal.clone()).await?;
            input.use_embeddings(abort_signal.clone()).await?;
            // Phase 26D: knowledge injection — no-op unless the role declares
            // `knowledge:` bindings or the user passed `--knowledge`.
            input.use_knowledge()?;
            start_directive(&config, input, cli.code, abort_signal).await
        }
        true => launch_repl(&cli, &config).await,
    }
}

/// Dispatch into the chosen REPL surface. The TTY check guards the
/// built-in Reedline REPL only — pi manages its own terminal lifecycle
/// and is happy to run with stdio piped (e.g. inside an integration test
/// harness), so we let it through.
async fn launch_repl(cli: &Cli, config: &GlobalConfig) -> Result<()> {
    match choose_repl(cli) {
        ReplChoice::Legacy => {
            if !*IS_STDOUT_TERMINAL {
                bail!("No TTY for REPL")
            }
            start_interactive(config).await
        }
        ReplChoice::Pi { strict: true } => crate::repl::pi::launch_pi(config).await,
        ReplChoice::Pi { strict: false } => {
            // Soft default: fall back to the legacy REPL when pi isn't
            // installed, so the upgrade doesn't strand users without an
            // interactive surface. Print a one-line note so the change is
            // visible and discoverable.
            if which::which("pi").is_err() {
                eprintln!(
                    "aichat: `pi` not on PATH; using the built-in REPL. Install pi at\n\
                     https://pi.dev for the new REPL surface, or pass --legacy-repl to\n\
                     silence this message. `--pi-repl` requires pi and will error if missing.",
                );
                if !*IS_STDOUT_TERMINAL {
                    bail!("No TTY for REPL")
                }
                return start_interactive(config).await;
            }
            crate::repl::pi::launch_pi(config).await
        }
    }
}

/// True when the REPL invocation should hand off to the pi coding-agent
/// harness. Driven by the `--pi-repl` flag or the `AICHAT_REPL=pi` env var;
/// suppressed by `--legacy-repl` so users can opt back into the built-in
/// REPL even with the env var set.
/// Phase 3: one-shot session conversion entry point. Resolves `src` to a
/// session file on disk, deserializes it, and writes the chosen output
/// format to either `out_path` or stdout. Returns the process exit code.
fn run_convert_session(src: &str, target: &str, out_path: Option<&str>) -> i32 {
    if target != "pi" {
        eprintln!(
            "--convert-session: target '{target}' is not supported. Only 'pi' is recognised."
        );
        return 2;
    }

    // Resolve the source. A path with a separator or an existing file goes
    // through unchanged; a bare name is looked up against the configured
    // sessions directory (same lookup rule as Config::session_file).
    let path = resolve_session_source(src);
    let yaml = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read session file '{}': {e}", path.display());
            return 1;
        }
    };
    let session: crate::config::Session = match serde_norway::from_str(&yaml) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse session YAML '{}': {e}", path.display());
            return 1;
        }
    };

    // `cwd` recorded in the pi header. Pi uses this to group sessions by
    // working directory; the user's current shell CWD is the right value
    // — they ran the conversion from where they intend to resume.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let write_result: Result<()> = match out_path {
        Some(p) => {
            // Buffered file writer. Pi's loader streams line-by-line so
            // we don't need to commit atomically.
            let f = match std::fs::File::create(p) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to create output file '{p}': {e}");
                    return 1;
                }
            };
            let mut buf = std::io::BufWriter::new(f);
            session.export_to_pi_jsonl(&cwd, &mut buf)
        }
        None => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            session.export_to_pi_jsonl(&cwd, &mut handle)
        }
    };

    if let Err(e) = write_result {
        eprintln!("Conversion failed: {e}");
        return 1;
    }
    0
}

/// Resolve a --convert-session argument to a real path. Mirrors the
/// lookup rule in `Config::session_file`: an explicit path (anything with
/// a separator, or an existing file) wins, otherwise we treat the input
/// as a session name and join against `$AICHAT_SESSIONS_DIR` (or the
/// platform default if unset).
fn resolve_session_source(src: &str) -> std::path::PathBuf {
    let as_path = std::path::PathBuf::from(src);
    if src.contains(std::path::MAIN_SEPARATOR) || as_path.exists() {
        return as_path;
    }
    if let Ok(dir) = std::env::var("AICHAT_SESSIONS_DIR") {
        return std::path::PathBuf::from(dir).join(format!("{src}.yaml"));
    }
    // Fall back to the conventional location under the user's config dir.
    // We don't try to be exhaustive here — anyone with a non-standard
    // layout can pass the absolute path explicitly.
    let cfg_dir = std::env::var_os("AICHAT_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::config_dir().map(|p| p.join("aichat")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    cfg_dir.join("sessions").join(format!("{src}.yaml"))
}

/// Resolve the sessions directory root the same way `resolve_session_source`
/// does for bare names: `$AICHAT_SESSIONS_DIR`, else `<config dir>/sessions`.
fn sessions_root() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("AICHAT_SESSIONS_DIR") {
        return std::path::PathBuf::from(dir);
    }
    let cfg_dir = std::env::var_os("AICHAT_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::config_dir().map(|p| p.join("aichat")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    cfg_dir.join("sessions")
}

/// Collect every `*.yaml` file under `dir`, recursing into subdirectories
/// (so the auto-named `_/` store is included).
fn collect_yaml_sessions(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yaml_sessions(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            out.push(path);
        }
    }
}

/// One-shot bulk migration: convert every legacy `.yaml` session under the
/// sessions directory to native pi `.jsonl`, removing the `.yaml` once the
/// `.jsonl` is written. Parses each session straight from YAML (no model
/// client needed) and writes through `Session::export_to_pi_jsonl`. Returns
/// the process exit code: 0 on full success, 1 if any file failed.
fn run_migrate_sessions() -> i32 {
    let root = sessions_root();
    if !root.exists() {
        eprintln!("No sessions directory at '{}'; nothing to migrate.", root.display());
        return 0;
    }
    let mut yamls = Vec::new();
    collect_yaml_sessions(&root, &mut yamls);
    if yamls.is_empty() {
        eprintln!("No legacy YAML sessions under '{}'.", root.display());
        return 0;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (mut ok, mut failed) = (0usize, 0usize);
    for yaml_path in &yamls {
        let jsonl_path = yaml_path.with_extension("jsonl");
        match migrate_one_session(yaml_path, &jsonl_path, &cwd) {
            Ok(()) => {
                ok += 1;
                eprintln!("migrated {} -> {}", yaml_path.display(), jsonl_path.display());
            }
            Err(e) => {
                failed += 1;
                eprintln!("failed to migrate {}: {e}", yaml_path.display());
            }
        }
    }
    eprintln!("Migrated {ok} session(s); {failed} failed.");
    if failed > 0 {
        1
    } else {
        0
    }
}

/// Convert a single YAML session file to pi JSONL and remove the source.
fn migrate_one_session(
    yaml_path: &std::path::Path,
    jsonl_path: &std::path::Path,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let yaml = std::fs::read_to_string(yaml_path)
        .with_context(|| format!("read '{}'", yaml_path.display()))?;
    let session: crate::config::Session =
        serde_norway::from_str(&yaml).with_context(|| format!("parse '{}'", yaml_path.display()))?;
    let mut buf: Vec<u8> = Vec::new();
    session.export_to_pi_jsonl(cwd, &mut buf)?;
    std::fs::write(jsonl_path, buf)
        .with_context(|| format!("write '{}'", jsonl_path.display()))?;
    std::fs::remove_file(yaml_path)
        .with_context(|| format!("remove '{}'", yaml_path.display()))?;
    Ok(())
}

/// Decision for which REPL to spawn. Pi is the default after the Phase 4
/// cutover; `--legacy-repl` and `AICHAT_REPL=legacy` keep the built-in
/// Reedline REPL available indefinitely so the two surfaces can be tested
/// side-by-side. `--pi-repl` / `AICHAT_REPL=pi` are still accepted but are
/// redundant with the new default — they keep working so existing setups
/// don't break.
///
/// "Strict pi" means: hard-error if pi isn't on PATH (user explicitly
/// asked for pi). "Soft pi" means: warn and fall back to the legacy REPL
/// if pi isn't on PATH (user just ran `aichat`).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ReplChoice {
    Pi { strict: bool },
    Legacy,
}

fn choose_repl(cli: &Cli) -> ReplChoice {
    if cli.legacy_repl {
        return ReplChoice::Legacy;
    }
    if cli.pi_repl {
        return ReplChoice::Pi { strict: true };
    }
    match std::env::var("AICHAT_REPL").as_deref() {
        Ok("legacy") => ReplChoice::Legacy,
        Ok("pi") => ReplChoice::Pi { strict: true },
        _ => ReplChoice::Pi { strict: false },
    }
}

#[async_recursion::async_recursion]
async fn start_directive(
    config: &GlobalConfig,
    mut input: Input,
    _code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<()> {
    // Build a trace emitter (if --trace or AICHAT_TRACE is active) for schema events
    let trace_emitter = config
        .read()
        .trace_config
        .clone()
        .map(crate::utils::trace::TraceEmitter::new);

    // Phase 33C: a role with a `source: stdin` slot takes free-text input routed
    // into that slot, so the raw message is not validated against the object
    // schema (which describes the slots, not the message).
    if let Some(schema) = input.role().input_schema() {
        if !input.role().has_stdin_slot() {
            validate_schema_traced("input", schema, &input.text(), trace_emitter.as_ref())?;
        }
    }

    let has_output_schema = input.role().output_schema().cloned();
    let output_format = config.read().output_format;
    let is_dry_run = config.read().dry_run;

    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;

    // Phase 12A/B: emit a "terraform plan"-style preview to stderr before the
    // assembled prompt hits stdout. Stderr keeps stdout pipeable; the preview
    // shows extends/include, ports, capabilities, and pipeline stages so the
    // caller sees what they're about to run with zero tokens spent.
    if is_dry_run {
        emit_dry_run_preview(input.role());
    }

    // Phase 9C: Schema validation retry budget. Short-circuit to 0 when the
    // provider is enforcing the schema natively (Phase 9A/9B) — a retry in
    // that regime can't buy us anything.
    let native_structured = has_output_schema.is_some()
        && input
            .role()
            .model()
            .data()
            .supports_response_format_json_schema;
    let max_schema_retries = if has_output_schema.is_some() && !is_dry_run && !native_structured {
        input.role().schema_retries().unwrap_or(1)
    } else {
        0
    };
    let original_input = input.clone();

    let (mut output, mut tool_results, mut metrics) =
        call_react(&mut input, client.as_ref(), abort_signal.clone()).await?;

    // Phase 27D: expand `[[fact-id]]` markers in the LLM output into a
    // deterministic provenance table. No-op when the role doesn't declare
    // `attributed_output: true` or no knowledge was retrieved.
    if input.role().attributed_output() && !is_dry_run {
        let hits = config.read().last_knowledge_hits.clone();
        if !hits.is_empty() {
            output = crate::knowledge::query::annotate_output_with_provenance(&output, &hits);
        }
    }

    // Retry loop: on output schema validation failure, re-send the original
    // prompt with the failed assistant output + a corrective user turn.
    let mut schema_retry_attempts: usize = 0;
    if let Some(ref schema) = has_output_schema {
        if !is_dry_run && max_schema_retries > 0 {
            loop {
                match validate_schema_traced("output", schema, &output, trace_emitter.as_ref()) {
                    Ok(()) => break,
                    Err(e) if schema_retry_attempts < max_schema_retries => {
                        schema_retry_attempts += 1;
                        let retry_prompt = format!(
                            "Your previous output failed schema validation:\n{e}\n\nPlease regenerate your response to conform to the required schema. Return ONLY valid JSON."
                        );
                        let mut retry_input = original_input
                            .clone()
                            .with_retry_prompt(&output, &retry_prompt);
                        let (new_output, new_tool_results, new_metrics) = call_react(
                            &mut retry_input,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?;
                        output = new_output;
                        tool_results = new_tool_results;
                        metrics.merge(&new_metrics);
                        input = retry_input;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }

    // Cost display on stderr if --cost flag is set
    if config.read().show_cost {
        eprintln!(
            "tokens: {}in/{}out  cost: ${:.6}  latency: {:.1}s",
            metrics.input_tokens,
            metrics.output_tokens,
            metrics.cost_usd,
            metrics.latency_ms as f64 / 1000.0
        );
    }

    // Phase 23A: evaluate role output metrics. Each metric runs a shell command
    // against the final, schema-validated output. Skipped in dry-run (no real
    // output to score). Results flow into the trace, run log (23C), and per-role
    // ledger (23D).
    let metric_results = if !is_dry_run {
        let metrics_defs = input.role().metrics();
        if metrics_defs.is_empty() {
            Vec::new()
        } else {
            let results = crate::config::evaluate_metrics(metrics_defs, &output);
            if let Some(ref emitter) = trace_emitter {
                emitter.emit_metrics(&results);
            }
            results
        }
    } else {
        Vec::new()
    };
    let metric_records: Vec<crate::utils::ledger::MetricRecord> = metric_results
        .iter()
        .map(|m| crate::utils::ledger::MetricRecord {
            name: m.name.clone(),
            pass: m.pass,
        })
        .collect();

    // One run_id shared by the run log and the per-role ledger for this call.
    let run_id = uuid::Uuid::new_v4().to_string();

    // JSONL run log
    if let Some(ref log_path) = config.read().run_log {
        let log_path = std::path::PathBuf::from(log_path);
        let mut record = serde_json::json!({
            "ts": crate::utils::now(),
            "run_id": run_id,
            "role": input.role().name(),
            "model": metrics.model_id,
            "input_tokens": metrics.input_tokens,
            "output_tokens": metrics.output_tokens,
            "cost_usd": metrics.cost_usd,
            "latency_ms": metrics.latency_ms,
            "schema_retries": schema_retry_attempts,
        });
        if !metric_results.is_empty() {
            record["metrics"] = serde_json::json!(metric_results);
        }
        if let Err(e) = crate::utils::ledger::append_run_log(&log_path, &record) {
            warn!("Failed to write run log: {e}");
        }
    }

    // Phase 23D: per-role invocation-history ledger (opt-in via AICHAT_ROLE_LEDGER).
    if let Some(ref dir) = config.read().role_ledger_dir {
        if !is_dry_run {
            let role_name = input.role().name().to_string();
            let file = std::path::Path::new(dir)
                .join(format!("{}.jsonl", crate::config::sanitize_role_name(&role_name)));
            let record = crate::utils::ledger::role_ledger_record(
                &run_id,
                &role_name,
                &metrics.model_id,
                &input.text(),
                &output,
                metrics.input_tokens,
                metrics.output_tokens,
                metrics.cost_usd,
                metrics.latency_ms,
                schema_retry_attempts,
                &metric_records,
            );
            if let Err(e) = crate::utils::ledger::append_run_log(&file, &record) {
                warn!("Failed to write role ledger: {e}");
            }
        }
    }

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
        // Retry loop above handles retries; when retries were enabled the
        // output is either valid or we already returned the error. When
        // max_schema_retries == 0 (native structured output, or user disabled),
        // do the validation here and propagate failures as before.
        if max_schema_retries == 0 {
            validate_schema_traced("output", schema, &output, trace_emitter.as_ref())?;
        }
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

/// Phase 23B: run `input_text` through both roles, score each role's metrics
/// against its output, and render a side-by-side comparison. The rendering
/// itself lives in `crate::compare::render_comparison` (pure + unit-tested);
/// this driver does the two LLM invocations and metric evaluation.
async fn run_compare(
    config: &GlobalConfig,
    roles: &[String],
    input_text: &str,
    output_format: Option<crate::cli::OutputFormat>,
) -> Result<()> {
    let abort_signal = create_abort_signal();
    let mut sides: Vec<crate::compare::CompareResult> = Vec::with_capacity(2);
    for role_name in roles.iter().take(2) {
        let result =
            crate::pipe::invoke_role(config, role_name, input_text, abort_signal.clone()).await?;
        // Load the role to evaluate its declared metrics against the output.
        let metrics = match config.read().retrieve_role(role_name) {
            Ok(role) => crate::config::evaluate_metrics(role.metrics(), &result.output),
            Err(_) => Vec::new(),
        };
        sides.push(crate::compare::CompareResult {
            role: role_name.clone(),
            model: result.metrics.model_id.clone(),
            output: result.output,
            input_tokens: result.metrics.input_tokens,
            output_tokens: result.metrics.output_tokens,
            cost_usd: result.metrics.cost_usd,
            metrics,
        });

        // Phase 23D: record each compared role's invocation in the per-role ledger.
        if let Some(ref dir) = config.read().role_ledger_dir {
            let side = sides.last().unwrap();
            let metric_records: Vec<crate::utils::ledger::MetricRecord> = side
                .metrics
                .iter()
                .map(|m| crate::utils::ledger::MetricRecord {
                    name: m.name.clone(),
                    pass: m.pass,
                })
                .collect();
            let file = std::path::Path::new(dir).join(format!(
                "{}.jsonl",
                crate::config::sanitize_role_name(role_name)
            ));
            let record = crate::utils::ledger::role_ledger_record(
                &uuid::Uuid::new_v4().to_string(),
                role_name,
                &side.model,
                input_text,
                &side.output,
                side.input_tokens,
                side.output_tokens,
                side.cost_usd,
                result.metrics.latency_ms,
                0,
                &metric_records,
            );
            if let Err(e) = crate::utils::ledger::append_run_log(&file, &record) {
                warn!("Failed to write role ledger: {e}");
            }
        }
    }
    let json = matches!(output_format, Some(crate::cli::OutputFormat::Json));
    let rendered = crate::compare::render_comparison(&sides[0], &sides[1], json);
    println!("{rendered}");
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
    let (eval_str, _, _metrics) =
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
                        let _r = call_chat_completions_streaming(
                            &input,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?;
                    } else {
                        let _r = call_chat_completions(
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

async fn batch_execute(
    config: &GlobalConfig,
    cli: &Cli,
    prompt_text: Option<String>,
    abort_signal: AbortSignal,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let prompt_template = prompt_text.unwrap_or_default();
    let parallel = cli.parallel.max(1);
    let reader = BufReader::new(tokio::io::stdin());
    let mut lines = reader.lines();

    if parallel <= 1 {
        // Sequential mode
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            match process_one_record(config, &prompt_template, &line, abort_signal.clone()).await {
                Ok(output) => println!("{}", output.trim()),
                Err(err) => {
                    let preview = if line.len() > 40 { &line[..40] } else { &line };
                    eprintln!("error: {}: {}", preview, err);
                }
            }
        }
    } else {
        // Parallel mode using buffered futures
        use futures_util::stream::{self, StreamExt};

        let mut records = Vec::new();
        while let Some(line) = lines.next_line().await? {
            if !line.trim().is_empty() {
                records.push(line);
            }
        }

        let results: Vec<_> = stream::iter(records.into_iter().enumerate())
            .map(|(idx, line)| {
                let config = config.clone();
                let template = prompt_template.clone();
                let abort = abort_signal.clone();
                async move {
                    let result = process_one_record(&config, &template, &line, abort).await;
                    (idx, line, result)
                }
            })
            .buffer_unordered(parallel)
            .collect()
            .await;

        // Sort by original index to preserve order
        let mut sorted = results;
        sorted.sort_by_key(|(idx, _, _)| *idx);
        for (_idx, line, result) in sorted {
            match result {
                Ok(output) => println!("{}", output.trim()),
                Err(err) => {
                    let preview = if line.len() > 40 { &line[..40] } else { &line };
                    eprintln!("error: {}: {}", preview, err);
                }
            }
        }
    }

    Ok(())
}

async fn process_one_record(
    config: &GlobalConfig,
    prompt_template: &str,
    record: &str,
    abort_signal: AbortSignal,
) -> Result<String> {
    // Build the prompt: interpolate record fields into the template
    let prompt = if prompt_template.is_empty() {
        record.to_string()
    } else {
        let mut text = prompt_template.to_string();
        crate::utils::interpolate_record_fields(&mut text, record);
        // If template had no placeholders and record isn't embedded, append it
        if text == prompt_template && !prompt_template.contains("{{.") {
            format!("{text}\n{record}")
        } else {
            text
        }
    };

    // Apply role prompt if set
    let role = config.read().role.clone();
    if let Some(ref role) = role {
        if role.has_output_schema() || config.read().output_format.map(|f| f.is_structured()).unwrap_or(false) {
            // keep prompt as is for structured output
        }
    }

    let mut input = Input::from_str(config, &prompt, role);
    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;

    let (output, _tool_results, _metrics) =
        call_react(&mut input, client.as_ref(), abort_signal).await?;

    // Validate and clean output
    let trace_emitter = config
        .read()
        .trace_config
        .clone()
        .map(crate::utils::trace::TraceEmitter::new);
    let output = if let Some(schema) = input.role().output_schema() {
        validate_schema_traced("output", schema, &output, trace_emitter.as_ref())?;
        output
    } else { match config.read().output_format { Some(fmt) => {
        if fmt.is_structured() {
            fmt.clean_output(&output)?
        } else {
            output
        }
    } _ => {
        output
    }}};

    Ok(output)
}

/// Phase 12A/B: emit a one-shot resolved-role preview to stderr. Skipped for
/// the implicit `%%` temp role used by `--prompt`/raw input — there's nothing
/// useful to preview when the user didn't pick a role. The preview is the
/// "what will this run with?" answer at zero token cost.
fn emit_dry_run_preview(role: &config::Role) {
    // The temp role created by `--prompt` (or bare `aichat "text"`) is named
    // `%%` and carries no metadata worth previewing — keep stderr clean.
    if role.name() == "%%" || role.name().is_empty() {
        return;
    }

    eprintln!("--- Resolved Role: {} ---", role.name());
    if let Some(parent) = role.extends() {
        eprintln!("  extends: {parent}");
    }
    if !role.include().is_empty() {
        eprintln!("  includes: [{}]", role.include().join(", "));
    }
    if let Some(model) = role.model_id() {
        eprintln!("  model: {model}");
    }
    let tools_str = role.use_tools().unwrap_or_default();
    let tools: Vec<&str> = tools_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !tools.is_empty() {
        eprintln!("  tools: {} ({})", tools.len(), tools.join(", "));
    }
    eprintln!("  in: {}  out: {}", role.port_input_summary(), role.port_output_summary());
    if !role.capabilities().is_empty() {
        eprintln!("  capabilities: [{}]", role.capabilities().join(", "));
    }
    // Phase 52B: the entity's facet families (Know·Act·Shape·Govern·Compose·
    // Judge), each tagged owned vs referenced per the backing-gates-ownership
    // rule. Omitted when the entity carries no facets.
    let facets = role.facets();
    if !facets.is_empty() {
        eprintln!("  facets: {}", facets.summary());
    }
    if let Some(nodes) = role.pipeline() {
        if !nodes.is_empty() {
            eprintln!("--- Pipeline ---");
            for (i, node) in nodes.iter().enumerate() {
                render_pipeline_node(node, i + 1, 0);
            }
        }
    }
    eprintln!("--- Assembled Prompt ---");
}

/// Phase 21: render a pipeline node tree to stderr with indented children.
fn render_pipeline_node(node: &config::PipelineNode, index: usize, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    match node {
        config::PipelineNode::Stage(s) => {
            let model = s.model.as_deref().unwrap_or("(default model)");
            eprintln!("{indent}{index}. {} ({model})", s.role);
        }
        config::PipelineNode::Parallel(p) => {
            let merge = match &p.merge {
                config::MergeStrategy::Concatenate => "concatenate".to_string(),
                config::MergeStrategy::JsonArray => "json_array".to_string(),
                config::MergeStrategy::CustomRole(r) => format!("custom_role: {r}"),
            };
            eprintln!(
                "{indent}{index}. parallel ({} branches, merge: {merge})",
                p.branches.len()
            );
            for (bi, b) in p.branches.iter().enumerate() {
                render_pipeline_node(b, bi + 1, depth + 1);
            }
        }
        config::PipelineNode::Switch(s) => {
            eprintln!("{indent}{index}. switch ({} branches)", s.branches.len());
            for (bi, b) in s.branches.iter().enumerate() {
                let kind = match &b.predicate {
                    Some(_) => "when",
                    None => "otherwise",
                };
                eprintln!("{indent}  {}. ({kind})", bi + 1);
                render_pipeline_node(&b.node, bi + 1, depth + 2);
            }
        }
    }
}

/// Phase 12C / 14D: render a role list with optional verbose details
/// (port signatures, capabilities, tools count, extends). Honors `-o json`
/// for machine consumption. Always emits one role per line in text mode so
/// it stays grep-friendly.
fn render_role_list(
    roles: &[config::Role],
    verbose: bool,
    output_format: Option<crate::cli::OutputFormat>,
) -> Result<()> {
    if matches!(output_format, Some(crate::cli::OutputFormat::Json)) {
        let json: Vec<serde_json::Value> = roles
            .iter()
            .map(|r| {
                let tools_str = r.use_tools().unwrap_or_default();
                let tools: Vec<&str> = tools_str
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                if verbose {
                    serde_json::json!({
                        "name": r.name(),
                        "description": r.description_or_derived(),
                        "model": r.model_id().unwrap_or("default"),
                        "tools": tools,
                        "capabilities": r.capabilities(),
                        "input": r.port_input_summary(),
                        "output": r.port_output_summary(),
                    })
                } else {
                    serde_json::json!({
                        "name": r.name(),
                        "description": r.description_or_derived(),
                        "model": r.model_id().unwrap_or("default"),
                        "tools": tools,
                    })
                }
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    if !verbose {
        for r in roles {
            println!("{}", r.name());
        }
        return Ok(());
    }

    // Verbose text rendering: align name into a left column, then port
    // signatures and capability tags. Width adapts to the longest name in the
    // current set so a small `--find-role` result stays compact.
    let name_width = roles.iter().map(|r| r.name().len()).max().unwrap_or(0).max(8);
    for r in roles {
        let tools_count = r
            .use_tools()
            .as_deref()
            .map(|s| s.split(',').filter(|t| !t.trim().is_empty()).count())
            .unwrap_or(0);
        let mut parts = vec![format!(
            "in: {}  out: {}",
            r.port_input_summary(),
            r.port_output_summary()
        )];
        if tools_count > 0 {
            parts.push(format!("{} tool{}", tools_count, if tools_count == 1 { "" } else { "s" }));
        }
        if !r.capabilities().is_empty() {
            parts.push(format!("capabilities: [{}]", r.capabilities().join(", ")));
        }
        if r.is_pipeline() {
            let n = r.pipeline().map(|p| p.len()).unwrap_or(0);
            let label = if r.pipeline_has_dag() { "node" } else { "stage" };
            parts.push(format!("pipeline: {n} {label}{}", if n == 1 { "" } else { "s" }));
        }
        println!("  {:<width$}  {}", r.name(), parts.join("  "), width = name_width);
    }
    Ok(())
}

/// Phase 13A: create a new role file that inherits from `SOURCE`.
///
/// The new file ships with `extends: SOURCE` plus a hint comment per
/// field the parent declares (model, temperature, top_p, use_tools,
/// input_schema, output_schema). The user uncomments the line they want
/// to override — a 5-minute editing task collapses to a 5-second command,
/// and the user picks up that `extends:` exists on the way.
fn run_fork_role(args: &[String], output_format: Option<crate::cli::OutputFormat>) -> Result<()> {
    if args.len() != 2 {
        bail!("--fork-role takes exactly two values: <SOURCE> <NEW_NAME>");
    }
    let source = &args[0];
    let new_name = &args[1];

    if new_name.trim().is_empty() {
        bail!("--fork-role: new name must not be empty");
    }
    // Same constraints `Config::role_file` would otherwise paper over —
    // refuse path traversal so the new file always lands in `roles_dir`.
    if new_name.contains('/') || new_name.contains('\\') || new_name.starts_with('.') {
        bail!(
            "--fork-role: invalid new role name '{new_name}' — must be a bare role name with no path separators or leading dot"
        );
    }

    // Resolve the source to validate it. If this fails the user gets a
    // helpful "unknown role" error before any file is created.
    let _resolved = config::Role::resolve(source)
        .with_context(|| format!("--fork-role: source role '{source}' could not be resolved"))?;

    let target_path = config::Config::role_file(new_name);
    if target_path.exists() {
        bail!(
            "--fork-role: target file {} already exists — pick a different NEW_NAME or remove the existing file",
            target_path.display()
        );
    }
    if let Some(parent_dir) = target_path.parent() {
        std::fs::create_dir_all(parent_dir).with_context(|| {
            format!("--fork-role: failed to create roles directory {}", parent_dir.display())
        })?;
    }

    let parent_metadata = config::read_role_raw_metadata(source)
        .with_context(|| format!("--fork-role: failed to read parent metadata for '{source}'"))?;
    let body = config::render_forked_role(source, &parent_metadata);
    std::fs::write(&target_path, &body)
        .with_context(|| format!("--fork-role: failed to write {}", target_path.display()))?;

    match output_format {
        Some(crate::cli::OutputFormat::Json) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "source": source,
                    "new_name": new_name,
                    "path": target_path.display().to_string(),
                }))?
            );
        }
        _ => {
            println!("Created {}", target_path.display());
            println!("  extends: {source}");
            println!("  Uncomment the fields you want to override, then edit the prompt body.");
        }
    }
    Ok(())
}

/// Phase 13D: print a human-readable explanation of a role's composition.
fn run_explain_role(name: &str, output_format: Option<crate::cli::OutputFormat>) -> Result<()> {
    let exp = config::build_role_explanation(name)
        .with_context(|| format!("--explain-role: failed to resolve role '{name}'"))?;
    match output_format {
        Some(crate::cli::OutputFormat::Json) => {
            println!("{}", serde_json::to_string_pretty(&exp)?);
        }
        _ => {
            print!("{}", config::format_role_explanation(&exp));
        }
    }
    Ok(())
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
