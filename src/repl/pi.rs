//! Pi coding-agent launcher.
//!
//! Replaces the built-in Reedline REPL with the `pi` open-source coding-agent
//! harness ([github.com/earendil-works/pi]) when invoked. The Rust side
//!
//! 1. probes `pi` on `PATH`,
//! 2. probes `127.0.0.1:8000-9000` for an already-running aichat server
//!    whose `/v1/state/*` bridge an exported `AICHAT_BRIDGE_TOKEN`
//!    authenticates against, and reuses it as the bridge if found;
//!    otherwise binds an ephemeral TCP port and starts aichat's
//!    OpenAI-compatible server via [`crate::serve::run_on`] (set
//!    `AICHAT_NO_SERVER_PROBE` to skip the probe and always start a
//!    private in-process server),
//! 3. mints a per-launch bridge token (in-process server only) and exposes
//!    the URL + token to the child via env vars (`AICHAT_BRIDGE_URL`,
//!    `AICHAT_BRIDGE_TOKEN`),
//! 4. pins pi to aichat's models: stages a throwaway pi agent dir whose
//!    `models.json` registers only the in-process aichat server as a provider
//!    and points pi at it via `PI_CODING_AGENT_DIR`, so pi ignores its own
//!    configured providers/models, AND segregates pi's session store —
//!    `<stage>/sessions` is symlinked to an aichat-owned dir
//!    (`<config>/pi-sessions`, override `AICHAT_PI_SESSIONS_DIR`) rather than
//!    the device-wide `~/.pi/agent/sessions/`, so REPL history stays out of
//!    pi's own store. Both behaviours opt out together with
//!    `AICHAT_PI_NATIVE_MODELS=1`, which leaves pi reading/writing its native
//!    models and its own device session store,
//! 5. stages the shipped extension into that agent dir's `extensions/`
//!    subdir, the only place pi 0.79.1 auto-discovers slash-command bridges,
//! 6. execs `pi` with stdio inherited so the child owns the terminal,
//! 7. on pi exit, removes the staged extension + agent dir (unless
//!    `AICHAT_KEEP_PI_STAGE=1`) and signals the server to shut down (a
//!    reused external server is left running).
//!
//! [github.com/earendil-works/pi]: https://github.com/earendil-works/pi

use anyhow::{bail, Context, Result};
use rust_embed::Embed;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;
use tokio::process::Command;

use crate::client::{list_all_models, Model, ModelType};
use crate::config::GlobalConfig;
use crate::serve;

/// Provider name our staged pi `models.json` registers aichat under. Pi
/// references models as `<provider>/<id>`; we expose a single provider so the
/// model picker shows only aichat's models.
const PI_AICHAT_PROVIDER: &str = "aichat";

/// Env var pi reads to override its agent config dir (`models.json`,
/// `settings.json`, `auth.json`, themes, …). Setting it to our staged dir is
/// how we make pi ignore its own configured models.
const PI_AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";

/// Opt-out: when set, aichat leaves pi's own model config untouched AND lets
/// pi read/write its own device-wide session store (`~/.pi/agent/sessions/`).
/// Unset (the default) pins pi to aichat's models and segregates pi's REPL
/// history into an aichat-owned session store (see [`segregated_pi_sessions_dir`]).
const PI_NATIVE_MODELS_ENV: &str = "AICHAT_PI_NATIVE_MODELS";

/// Env var overriding the segregated aichat-owned pi session store path.
/// When unset, the store defaults to `<aichat config dir>/pi-sessions`.
const PI_SESSIONS_DIR_ENV: &str = "AICHAT_PI_SESSIONS_DIR";

/// Subdir of aichat's config dir holding the segregated pi REPL session store
/// (pi's JSONL session tree) used in the default, model-pinned mode. Keeps pi
/// REPL history out of the device-wide `~/.pi/agent/sessions/` store so the two
/// surfaces never cross-contaminate.
const PI_SESSIONS_DIR_NAME: &str = "pi-sessions";

/// Resolve the segregated, aichat-owned pi session store. This is where pi
/// writes its JSONL session tree when aichat pins it to aichat models (the
/// default). It is deliberately *not* `~/.pi/agent/sessions/`: that device-wide
/// store is only used in native mode (`AICHAT_PI_NATIVE_MODELS=1`). The store
/// is persistent (it outlives the throwaway agent stage) so pi's `/resume`
/// keeps working across launches. Override with `AICHAT_PI_SESSIONS_DIR`.
fn segregated_pi_sessions_dir() -> PathBuf {
    if let Some(v) = std::env::var_os(PI_SESSIONS_DIR_ENV) {
        return PathBuf::from(v);
    }
    crate::config::Config::config_dir().join(PI_SESSIONS_DIR_NAME)
}

/// Hint emitted when `pi` is not discoverable on `PATH`. Keep terse and
/// actionable — the user lost their REPL, they don't want a tutorial.
const PI_INSTALL_HINT: &str = "\
`pi` not found on PATH.

Install the pi coding-agent harness:
  curl -fsSL https://pi.dev/install.sh | sh
  # or: npm install -g @earendil-works/pi-coding-agent

Then re-run with `--pi-repl` or set AICHAT_REPL=pi.
To force the built-in REPL instead, pass --legacy-repl.";

/// File name the bundled extension is staged under inside `.pi/extensions/`.
/// Pi requires extensions to end in `.ts` or `.js`; we ship a pre-bundled
/// `.js` so the launch path has no Node compile step on the hot path.
const STAGED_EXTENSION_NAME: &str = "aichat-bridge.js";

/// Compiled pi extension bundle, embedded into the aichat binary at build
/// time. Source lives in `pi-extensions/` and is bundled by esbuild into the
/// `assets/pi-extensions/` directory; both are checked in so contributors
/// without Node tooling can still build aichat.
#[derive(Embed)]
#[folder = "assets/pi-extensions/"]
struct PiExtensionsAsset;

/// Hint emitted when the `pi-acp` adapter is not discoverable on `PATH`.
const ACP_INSTALL_HINT: &str = "\
`pi-acp` not found on PATH.

The ACP interface delegates protocol translation to the pi-acp adapter:
  npm install -g pi-acp

Or point aichat at a custom adapter with AICHAT_ACP_COMMAND
(e.g. `AICHAT_ACP_COMMAND='npx -y pi-acp'`).";

/// Env var overriding the ACP adapter invocation. Whitespace-split into a
/// program plus leading args (e.g. `npx -y pi-acp`). Defaults to `pi-acp`.
const ACP_ADAPTER_ENV: &str = "AICHAT_ACP_COMMAND";

/// Shared bridge + staging context for launching a pi-backed surface — the
/// interactive REPL ([`launch_pi`]) or the ACP adapter ([`launch_acp`]).
/// Brings up (or reuses) the aichat HTTP bridge, pins pi to aichat's models
/// via a staged agent dir, and stages the slash-command bridge extension.
/// Holds the teardown handles so the caller can drive the child's lifetime.
struct PiBridge {
    bridge_url: String,
    token: Option<String>,
    /// `PI_CODING_AGENT_DIR` the child should read. `Some` when models are
    /// pinned to aichat (the default); `None` in native-models mode, where the
    /// child uses pi's real agent dir and we don't override the env var.
    agent_dir_override: Option<PathBuf>,
    /// Shutdown handle for the in-process server. `None` when we reused an
    /// already-running external server (which we must leave running).
    stop_server: Option<tokio::sync::oneshot::Sender<()>>,
    staging: StagedExtension,
    agent_stage: Option<StagedAgentDir>,
}

impl PiBridge {
    /// Bring up the bridge and stage the agent dir + extension. Identical setup
    /// for both the REPL and ACP surfaces — only the child process differs.
    async fn setup(config: &GlobalConfig) -> Result<Self> {
        // Prefer an aichat server already listening on the conventional port
        // range: reusing it means one inference process instead of two, and the
        // user's live roles/sessions on that server stay addressable. Fall back
        // to a private in-process server on an ephemeral port when none is found.
        let (bridge_url, token, stop_server) = match probe_existing_server().await {
            Some(url) => {
                info!("aichat bridge: reusing existing aichat server at {url}");
                // `probe_existing_server` only returns a URL after a successful
                // authenticated `/v1/state/info` call, so AICHAT_BRIDGE_TOKEN is
                // guaranteed present here and the remote `/v1/state/*` slash
                // commands will accept it. Pass that same token to the child.
                (url, std::env::var("AICHAT_BRIDGE_TOKEN").ok(), None)
            }
            None => {
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .context("aichat bridge: failed to bind ephemeral port on 127.0.0.1")?;
                let addr = listener
                    .local_addr()
                    .context("aichat bridge: listener has no local address")?;
                let url = format!("http://127.0.0.1:{}", addr.port());
                let token = mint_bridge_token();
                // Hand the freshly minted token straight to the in-process server
                // instead of mutating our own process environment. Mutating env at
                // runtime (set_var) is unsafe on the multi-threaded runtime — see
                // the read-once discipline in serve::run. The child process still
                // receives the token explicitly via [`apply_env`].
                let stop = serve::run_on(listener, config, Some(token.clone()))
                    .await
                    .context("aichat bridge: failed to start in-process server")?;
                info!("aichat bridge listening on {url}");
                (url, Some(token), Some(stop))
            }
        };

        // Pin pi to aichat's models: stage a throwaway agent dir whose
        // `models.json` registers only the in-process aichat server as a provider,
        // and point pi at it via `PI_CODING_AGENT_DIR`. pi then ignores its own
        // configured providers/models entirely. Opt out with
        // `AICHAT_PI_NATIVE_MODELS=1` to keep pi's own model config.
        let agent_stage = if std::env::var_os(PI_NATIVE_MODELS_ENV).is_some() {
            None
        } else {
            match stage_pi_agent_dir(config, &bridge_url) {
                Ok(staged) => Some(staged),
                Err(e) => {
                    warn!("aichat bridge: could not pin pi to aichat models ({e}); pi will use its own model config");
                    None
                }
            }
        };

        // Stage the bridge extension into the agent dir pi will actually read.
        // pi auto-discovers extensions from `<PI_CODING_AGENT_DIR>/extensions/`;
        // when we pin models that is our throwaway stage, otherwise it is pi's
        // real agent dir. Staging anywhere else (e.g. `<cwd>/.pi/extensions/`) is
        // silently ignored by pi and our slash-commands never register.
        let agent_dir: PathBuf = match &agent_stage {
            Some(staged) => staged.path.clone(),
            None => pi_real_agent_dir(),
        };
        let staging = StagedExtension::stage(&agent_dir)?;

        Ok(Self {
            bridge_url,
            token,
            agent_dir_override: agent_stage.as_ref().map(|s| s.path.clone()),
            stop_server,
            staging,
            agent_stage,
        })
    }

    /// Export the bridge env onto a child command (pi, or the ACP adapter
    /// which in turn spawns pi). The child inherits `PI_CODING_AGENT_DIR` so
    /// pi-acp's own pi subprocess reads our staged, model-pinned agent dir.
    fn apply_env(&self, command: &mut Command) {
        command.env("AICHAT_BRIDGE_URL", &self.bridge_url);
        if let Some(token) = &self.token {
            command.env("AICHAT_BRIDGE_TOKEN", token);
        }
        if let Some(dir) = &self.agent_dir_override {
            command.env(PI_AGENT_DIR_ENV, dir);
        }
    }

    /// Signal the in-process server to shut down (no-op when we reused one) and
    /// clean up the staged extension + agent dir, unless the user asked us not
    /// to via `AICHAT_KEEP_PI_STAGE` (handy when debugging load failures). No
    /// env cleanup needed: the token was handed to the server and the child
    /// explicitly, never written into our own process environment.
    fn teardown(self) {
        if let Some(stop) = self.stop_server {
            let _ = stop.send(());
        }
        if std::env::var_os("AICHAT_KEEP_PI_STAGE").is_none() {
            self.staging.cleanup();
            if let Some(staged) = self.agent_stage {
                staged.cleanup();
            }
        }
    }
}

/// Launch `pi` as the REPL surface, with aichat's HTTP server running
/// in-process on an ephemeral port. Blocks until pi exits.
pub async fn launch_pi(config: &GlobalConfig) -> Result<()> {
    let pi_bin = match which::which("pi") {
        Ok(p) => p,
        Err(_) => bail!("{PI_INSTALL_HINT}"),
    };

    let bridge = PiBridge::setup(config).await?;

    info!("launching pi from {}", pi_bin.display());

    let mut command = Command::new(&pi_bin);
    command.args(pi_repl_args());
    bridge.apply_env(&mut command);
    let spawn_result = command.status().await;

    bridge.teardown();

    let status = spawn_result
        .with_context(|| format!("failed to spawn pi at {}", pi_bin.display()))?;

    if !status.success() {
        match status.code() {
            Some(code) => bail!("pi exited with status {code}"),
            None => bail!("pi terminated by signal"),
        }
    }
    Ok(())
}

/// Run as an ACP agent over stdio. Reuses the same bridge + model-pinning
/// staging as [`launch_pi`], but instead of execing the interactive pi TUI it
/// spawns the `pi-acp` adapter with stdio inherited: the ACP client that
/// launched `aichat --acp` talks JSON-RPC straight through to the adapter over
/// aichat's stdin/stdout. The adapter spawns `pi --mode rpc` itself, inheriting
/// the staged `PI_CODING_AGENT_DIR`. Blocks until the adapter exits.
///
/// stdout MUST stay clean here — it is the ACP transport. ACP mode routes
/// through `WorkingMode::Serve`, so logging goes to stderr and no banner is
/// printed (`serve::run_on`, unlike `serve::run`, prints nothing).
pub async fn launch_acp(config: &GlobalConfig) -> Result<()> {
    // The adapter spawns pi; fail early with a clear, pi-specific message if
    // it's missing, and pin the adapter to the exact binary we resolved.
    let pi_bin = match which::which("pi") {
        Ok(p) => p,
        Err(_) => bail!("{PI_INSTALL_HINT}"),
    };
    let (adapter_bin, adapter_args) = resolve_acp_adapter()?;

    let bridge = PiBridge::setup(config).await?;

    info!("launching ACP adapter from {}", adapter_bin.display());

    let mut command = Command::new(&adapter_bin);
    command.args(&adapter_args);
    bridge.apply_env(&mut command);
    // Pin the adapter's pi subprocess to the exact binary aichat resolved, so
    // it matches the bridge-staged, model-pinned agent dir. pi-acp reads
    // `PI_ACP_PI_COMMAND` when spawning pi.
    command.env("PI_ACP_PI_COMMAND", &pi_bin);
    // stdio is inherited (tokio's default), making the adapter the ACP endpoint
    // on aichat's stdin/stdout.
    let spawn_result = command.status().await;

    bridge.teardown();

    let status = spawn_result
        .with_context(|| format!("failed to spawn ACP adapter at {}", adapter_bin.display()))?;

    if !status.success() {
        match status.code() {
            Some(code) => bail!("ACP adapter exited with status {code}"),
            None => bail!("ACP adapter terminated by signal"),
        }
    }
    Ok(())
}

/// Resolve the ACP adapter invocation: program path + leading args. Defaults
/// to the `pi-acp` binary on `PATH`; override with `AICHAT_ACP_COMMAND`
/// (whitespace-split, e.g. `npx -y pi-acp`).
fn resolve_acp_adapter() -> Result<(PathBuf, Vec<String>)> {
    if let Some(raw) = std::env::var_os(ACP_ADAPTER_ENV) {
        let raw = raw.to_string_lossy();
        let mut parts = raw.split_whitespace().map(|s| s.to_string());
        let prog = parts
            .next()
            .with_context(|| format!("{ACP_ADAPTER_ENV} is set but empty"))?;
        let path = which::which(&prog).with_context(|| {
            format!("ACP adapter '{prog}' (from {ACP_ADAPTER_ENV}) not found on PATH")
        })?;
        return Ok((path, parts.collect()));
    }
    let path = which::which("pi-acp").map_err(|_| anyhow::anyhow!("{ACP_INSTALL_HINT}"))?;
    Ok((path, Vec::new()))
}

/// CLI args aichat passes on every `pi` launch.
///
/// `--continue` resumes the most recent session for the cwd. Pi keeps no
/// standalone command-history file: the REPL's up-arrow editor history is
/// rebuilt from a resumed session's user messages (pi `interactive-mode`
/// populateHistory). Without `--continue`, pi calls `SessionManager.create`
/// for a fresh empty session and command history never carries across
/// launches. Pi's `continueRecent` falls back to a fresh session when the cwd
/// has none, so this is safe on the very first launch.
fn pi_repl_args() -> &'static [&'static str] {
    &["--continue"]
}

/// Mint a per-launch bridge token used to authenticate slash-command
/// extension calls back into aichat. `uuid::Uuid::new_v4()` is sourced
/// from the OS RNG; 122 bits of entropy is overkill for a localhost,
/// process-lifetime credential but keeps the implementation dependency-free.
fn mint_bridge_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Inclusive TCP port range scanned on `127.0.0.1` for an already-running
/// aichat server before a `pi` launch falls back to its own in-process one.
/// 8000–9000 is the conventional band `aichat --serve` lands in.
const PROBE_PORT_START: u16 = 8000;
const PROBE_PORT_END: u16 = 9000;

/// Per-port HTTP timeout while probing. Localhost ports answer — or refuse —
/// far faster than this; the cap only bounds a pathologically slow listener.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

/// Decide whether an HTTP response to an authenticated `GET /v1/state/info`
/// came from an aichat bridge that this launch can actually drive.
///
/// The probe deliberately fingerprints the *bridge*, not just any aichat
/// server: aichat answers `/v1/state/info` with `200` and a JSON object
/// carrying an `info` string only when the request's bearer token matches
/// the server's. A non-aichat server has no such route (`404`); an aichat
/// server started without a bridge token also `404`s it; an aichat bridge
/// with a *different* token answers `401`. Matching strictly on the `200`
/// + `info` shape means the probe only ever reuses a server whose
/// `/v1/state/*` slash commands will work for this launch.
fn is_authenticated_bridge(status: u16, body: &str) -> bool {
    status == 200
        && serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("info").map(serde_json::Value::is_string))
            .unwrap_or(false)
}

/// Probe one port: authenticated `GET http://127.0.0.1:{port}/v1/state/info`
/// and report whether the response fingerprints as a usable aichat bridge.
/// Any transport failure (connection refused, timeout, reset) counts as
/// "not a usable bridge".
async fn port_has_bridge(client: &reqwest::Client, port: u16, token: &str) -> bool {
    let url = format!("http://127.0.0.1:{port}/v1/state/info");
    let Ok(resp) = client.get(&url).bearer_auth(token).send().await else {
        return false;
    };
    let status = resp.status().as_u16();
    match resp.text().await {
        Ok(body) => is_authenticated_bridge(status, &body),
        Err(_) => false,
    }
}

/// Scan `[start, end]` on `127.0.0.1` concurrently and return the lowest port
/// running an aichat bridge that `token` authenticates against, or `None`
/// when the range holds none. Probes run in parallel so the common case (a
/// closed range that refuses instantly, or a single match) resolves in well
/// under [`PROBE_TIMEOUT`].
async fn probe_port_range(start: u16, end: u16, token: &str) -> Option<u16> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .ok()?;
    let probes = (start..=end).map(|port| {
        let client = client.clone();
        let token = token.to_string();
        async move { port_has_bridge(&client, port, &token).await.then_some(port) }
    });
    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .min()
}

/// Discover an already-running aichat server on the conventional port range
/// so a fresh `pi` launch can share it instead of starting a second
/// in-process server. Returns the bridge URL to point `pi` at.
///
/// Reuse requires authenticating to the remote server's `/v1/state/*`
/// bridge, so it only happens when `AICHAT_BRIDGE_TOKEN` is exported and
/// matches the token the target server was launched with. With no token
/// exported — or with `AICHAT_NO_SERVER_PROBE` set — discovery is skipped
/// and the caller starts a fresh in-process server on an ephemeral port.
async fn probe_existing_server() -> Option<String> {
    if std::env::var_os("AICHAT_NO_SERVER_PROBE").is_some() {
        return None;
    }
    // Reuse hinges on driving the remote `/v1/state/*` bridge, which needs a
    // token. With none exported there is nothing to authenticate with, so a
    // fresh in-process server (which mints its own token) is the only option.
    let token = std::env::var("AICHAT_BRIDGE_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())?;
    let port = probe_port_range(PROBE_PORT_START, PROBE_PORT_END, &token).await?;
    Some(format!("http://127.0.0.1:{port}"))
}

/// Handle for the staged extension file. Holds the path so cleanup can
/// `remove_file` it on drop or explicit `.cleanup()`. We deliberately do
/// not own the `extensions/` directory: another tool (or the user's own
/// global config) may have extensions staged there too.
struct StagedExtension {
    path: PathBuf,
    /// True when we actually wrote a file at `path` (vs. found one already
    /// present that we should leave alone).
    we_created_it: bool,
}

impl StagedExtension {
    /// Stage the bundled bridge into `<agent_dir>/extensions/`. pi 0.79.1
    /// auto-discovers extensions from that subdir of its agent config dir
    /// (`PI_CODING_AGENT_DIR`) — it does NOT scan `<cwd>/.pi/extensions/`, so
    /// staging anywhere else means pi never registers our slash-commands.
    fn stage(agent_dir: &Path) -> Result<Self> {
        let ext_bytes = match PiExtensionsAsset::get(STAGED_EXTENSION_NAME) {
            Some(f) => f.data,
            None => bail!(
                "aichat was built without the pi extension bundle (assets/pi-extensions/{STAGED_EXTENSION_NAME})",
            ),
        };
        let ext_dir = agent_dir.join("extensions");
        std::fs::create_dir_all(&ext_dir)
            .with_context(|| format!("failed to create {}", ext_dir.display()))?;
        let path = ext_dir.join(STAGED_EXTENSION_NAME);

        // If a file is already there (e.g. user-customized fork), don't
        // overwrite it. They get whichever bridge they put on disk.
        let we_created_it = !path.exists();
        if we_created_it {
            std::fs::write(&path, ext_bytes.as_ref())
                .with_context(|| format!("failed to stage {}", path.display()))?;
        }
        Ok(Self {
            path,
            we_created_it,
        })
    }

    fn cleanup(self) {
        if self.we_created_it {
            // Best-effort: a failure here means the next launch will see a
            // stale (but identical) file at the same path, which is fine.
            let _ = std::fs::remove_file(&self.path);
            // Try to prune the `extensions/` dir if we left it empty. Ignore
            // failures — another tool may share it.
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
}

/// Build the contents of the staged pi `models.json`. Registers a single
/// provider — [`PI_AICHAT_PROVIDER`] — pointing at the in-process
/// OpenAI-compatible aichat server, whose model list is aichat's chat models.
/// Pi resolves providers solely from this file, so its own configured
/// providers/models are ignored for the launch.
fn build_pi_models_json(models: &[Model], base_url: &str, api_key: &str) -> Value {
    let entries: Vec<Value> = models
        .iter()
        .filter(|m| m.model_type() == ModelType::Chat)
        .map(|m| {
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), json!(m.id()));
            if let Some(ctx) = m.max_input_tokens() {
                obj.insert("contextWindow".into(), json!(ctx));
            }
            if let Some(out) = m.max_output_tokens() {
                if out > 0 {
                    obj.insert("maxTokens".into(), json!(out));
                }
            }
            Value::Object(obj)
        })
        .collect();
    let mut provider = serde_json::Map::new();
    provider.insert("baseUrl".into(), json!(base_url));
    provider.insert("api".into(), json!("openai-completions"));
    provider.insert("apiKey".into(), json!(api_key));
    provider.insert("models".into(), Value::Array(entries));
    let mut providers = serde_json::Map::new();
    providers.insert(PI_AICHAT_PROVIDER.to_string(), Value::Object(provider));
    let mut root = serde_json::Map::new();
    root.insert("providers".into(), Value::Object(providers));
    Value::Object(root)
}

/// Choose pi's default model id. Prefer aichat's configured default when it is
/// a usable chat model; otherwise fall back to the first chat model. Returns
/// `None` only when aichat exposes no chat models at all.
fn pi_default_model(models: &[Model], configured_id: &str) -> Option<String> {
    let chat: Vec<String> = models
        .iter()
        .filter(|m| m.model_type() == ModelType::Chat)
        .map(|m| m.id())
        .collect();
    if chat.iter().any(|id| id == configured_id) {
        Some(configured_id.to_string())
    } else {
        chat.into_iter().next()
    }
}

/// Build the staged pi `settings.json`, preserving the user's existing prefs
/// (theme, thinking level, …) while overriding only the default provider and
/// model to point at the staged aichat provider.
fn build_pi_settings(existing: Option<&str>, default_model: &str) -> Value {
    let mut obj = existing
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    obj.insert("defaultProvider".into(), json!(PI_AICHAT_PROVIDER));
    obj.insert("defaultModel".into(), json!(default_model));
    Value::Object(obj)
}

/// Resolve the pi agent config dir the same way pi does: honor
/// `PI_CODING_AGENT_DIR` if exported, else `~/.pi/agent`. Used as the symlink
/// source so the staged dir inherits the user's sessions/auth/themes.
fn pi_real_agent_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(PI_AGENT_DIR_ENV) {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_default()
        .join(".pi")
        .join("agent")
}

/// A throwaway pi agent config dir holding our `models.json` + `settings.json`
/// (real files) with every other entry symlinked back to the user's real agent
/// dir, so pi sessions/auth/themes keep working. Pointing pi at this via
/// `PI_CODING_AGENT_DIR` makes pi read models from our file only.
struct StagedAgentDir {
    path: PathBuf,
}

impl StagedAgentDir {
    /// Stage a throwaway pi agent dir. `session_store` is the aichat-owned
    /// directory pi's `sessions/` is pointed at, segregating pi REPL history
    /// from the device-wide `~/.pi/agent/sessions/` store. The real agent dir's
    /// own `sessions/` entry is therefore *not* symlinked through.
    fn stage(real_dir: &Path, models: &Value, settings: &Value, session_store: &Path) -> Result<Self> {
        let path = std::env::temp_dir()
            .join(format!("aichat-pi-agent-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create pi agent stage {}", path.display()))?;

        // Symlink every entry of the real agent dir except the two files we
        // override and `sessions/` (which we segregate below), so auth/themes/
        // prompts stay live. Best-effort: a broken or unreadable entry just
        // won't be available to pi.
        #[cfg(unix)]
        if real_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(real_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name == "models.json" || name == "settings.json" || name == "sessions" {
                        continue;
                    }
                    let _ = std::os::unix::fs::symlink(entry.path(), path.join(&name));
                }
            }
        }

        // Segregate pi's session store: point `<stage>/sessions` at the
        // aichat-owned, persistent dir instead of `~/.pi/agent/sessions/`. The
        // store must outlive the throwaway stage so `/resume` survives across
        // launches; create it up front so pi can write into it immediately.
        #[cfg(unix)]
        {
            std::fs::create_dir_all(session_store).with_context(|| {
                format!("failed to create pi session store {}", session_store.display())
            })?;
            let _ = std::os::unix::fs::symlink(session_store, path.join("sessions"));
        }
        #[cfg(not(unix))]
        let _ = session_store;

        std::fs::write(path.join("models.json"), serde_json::to_vec_pretty(models)?)
            .context("failed to write staged pi models.json")?;
        std::fs::write(
            path.join("settings.json"),
            serde_json::to_vec_pretty(settings)?,
        )
        .context("failed to write staged pi settings.json")?;
        Ok(Self { path })
    }

    /// Remove the stage. `remove_dir_all` unlinks symlinks rather than
    /// recursing through them, so the real agent dir behind them is untouched.
    fn cleanup(self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Stage a pi agent dir that pins pi to aichat's models. Reads the current
/// model set + default from `config`, writes a `models.json`/`settings.json`
/// exposing only the in-process aichat server, and returns the stage handle.
fn stage_pi_agent_dir(
    config: &GlobalConfig,
    bridge_url: &str,
) -> Result<StagedAgentDir> {
    let snapshot = config.read().clone();
    let models: Vec<Model> = list_all_models(&snapshot).into_iter().cloned().collect();
    let base_url = format!("{}/v1", bridge_url.trim_end_matches('/'));
    // `/v1/chat/completions` is open on the in-process server unless
    // `serve_api_key:` is configured; pi still requires a non-empty apiKey for
    // an openai-completions provider, so fall back to a dummy when unset.
    let api_key = snapshot
        .serve_api_key
        .clone()
        .unwrap_or_else(|| "aichat".to_string());

    let default_model = pi_default_model(&models, &snapshot.model.id())
        .context("aichat exposes no chat models to pin pi to")?;
    let models_json = build_pi_models_json(&models, &base_url, &api_key);

    let real_dir = pi_real_agent_dir();
    let existing_settings = std::fs::read_to_string(real_dir.join("settings.json")).ok();
    let settings_json = build_pi_settings(existing_settings.as_deref(), &default_model);

    let session_store = segregated_pi_sessions_dir();
    let staged = StagedAgentDir::stage(&real_dir, &models_json, &settings_json, &session_store)?;
    let chat_count = models
        .iter()
        .filter(|m| m.model_type() == ModelType::Chat)
        .count();
    info!("aichat bridge: pinned pi to {chat_count} aichat model(s); default '{default_model}'");
    info!("aichat bridge: pi REPL sessions segregated to {}", session_store.display());
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Model;
    use serde_json::json;

    /// Build a chat `Model` with the given id parts and optional token limits.
    fn chat_model(client: &str, name: &str, ctx: Option<usize>, out: Option<isize>) -> Model {
        let mut m = Model::new(client, name);
        m.data_mut().max_input_tokens = ctx;
        m.data_mut().max_output_tokens = out;
        m
    }

    /// Build a non-chat model (embedding) to prove it is filtered out.
    fn embed_model(client: &str, name: &str) -> Model {
        let mut m = Model::new(client, name);
        m.data_mut().model_type = "embedding".into();
        m
    }

    #[test]
    fn models_json_registers_only_aichat_provider_with_chat_models() {
        let models = vec![
            chat_model("openai", "gpt-4o", Some(128000), Some(16384)),
            chat_model("ollama", "llama3", Some(8192), None),
            embed_model("openai", "text-embedding-3-small"),
        ];
        let v = build_pi_models_json(&models, "http://127.0.0.1:9999/v1", "aichat");
        let providers = v["providers"].as_object().expect("providers object");
        // Exactly one provider — pi's own google/anthropic/ollama config is ignored.
        assert_eq!(providers.len(), 1);
        let p = &providers["aichat"];
        assert_eq!(p["baseUrl"], "http://127.0.0.1:9999/v1");
        assert_eq!(p["api"], "openai-completions");
        assert_eq!(p["apiKey"], "aichat");
        let list = p["models"].as_array().expect("models array");
        // Embedding model filtered out; two chat models remain.
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["id"], "openai:gpt-4o");
        assert_eq!(list[0]["contextWindow"], 128000);
        assert_eq!(list[0]["maxTokens"], 16384);
        // No maxTokens key when the model has no output limit.
        assert_eq!(list[1]["id"], "ollama:llama3");
        assert_eq!(list[1]["contextWindow"], 8192);
        assert!(list[1].get("maxTokens").is_none());
    }

    #[test]
    fn default_model_prefers_configured_when_present() {
        let models = vec![
            chat_model("openai", "gpt-4o", None, None),
            chat_model("ollama", "llama3", None, None),
        ];
        assert_eq!(
            pi_default_model(&models, "ollama:llama3").as_deref(),
            Some("ollama:llama3")
        );
    }

    #[test]
    fn default_model_falls_back_to_first_chat_when_configured_absent() {
        let models = vec![
            embed_model("openai", "text-embedding-3-small"),
            chat_model("openai", "gpt-4o", None, None),
        ];
        // Configured id is an embedding (not a usable chat model) → first chat model.
        assert_eq!(
            pi_default_model(&models, "openai:text-embedding-3-small").as_deref(),
            Some("openai:gpt-4o")
        );
    }

    #[test]
    fn default_model_none_when_no_chat_models() {
        let models = vec![embed_model("openai", "text-embedding-3-small")];
        assert_eq!(pi_default_model(&models, "whatever"), None);
    }

    #[test]
    fn settings_override_defaults_preserving_other_keys() {
        let existing = r#"{"defaultProvider":"ollama","defaultModel":"gemma4:26b","theme":"dark","defaultThinkingLevel":"high"}"#;
        let v = build_pi_settings(Some(existing), "openai:gpt-4o");
        assert_eq!(v["defaultProvider"], "aichat");
        assert_eq!(v["defaultModel"], "openai:gpt-4o");
        // Unrelated user prefs survive.
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["defaultThinkingLevel"], "high");
    }

    #[test]
    fn settings_minimal_when_no_existing_file() {
        let v = build_pi_settings(None, "openai:gpt-4o");
        assert_eq!(v["defaultProvider"], "aichat");
        assert_eq!(v["defaultModel"], "openai:gpt-4o");
    }

    #[test]
    fn settings_ignores_malformed_existing() {
        let v = build_pi_settings(Some("not json {{{"), "m");
        assert_eq!(v["defaultProvider"], "aichat");
        assert_eq!(v["defaultModel"], "m");
    }

    #[cfg(unix)]
    #[test]
    fn staged_agent_dir_writes_our_files_and_symlinks_the_rest() {
        let real = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        // Pre-existing user agent dir: sessions/, auth.json, and a stale
        // models.json that must NOT leak through.
        std::fs::create_dir_all(real.path().join("sessions")).unwrap();
        std::fs::write(real.path().join("sessions/s1.json"), b"session").unwrap();
        std::fs::write(real.path().join("auth.json"), b"auth").unwrap();
        std::fs::write(real.path().join("models.json"), b"USER-MODELS").unwrap();

        let models = json!({"providers":{"aichat":{}}});
        let settings = json!({"defaultProvider":"aichat"});
        let staged =
            StagedAgentDir::stage(real.path(), &models, &settings, store.path()).unwrap();
        let dir = staged.path.clone();

        // Our models.json is a real file with our content — not the user's.
        let got = std::fs::read_to_string(dir.join("models.json")).unwrap();
        assert!(got.contains("aichat"));
        assert!(!got.contains("USER-MODELS"));
        assert!(!dir.join("models.json").symlink_metadata().unwrap().file_type().is_symlink());
        // settings.json is ours too.
        assert!(std::fs::read_to_string(dir.join("settings.json")).unwrap().contains("aichat"));
        // auth/themes/prompts are symlinked back to the real dir so they survive.
        assert!(dir.join("auth.json").symlink_metadata().unwrap().file_type().is_symlink());

        staged.cleanup();
        assert!(!dir.exists());
        // Cleanup must not touch the real dir behind the symlinks.
        assert!(real.path().join("sessions/s1.json").exists());
        assert!(real.path().join("auth.json").exists());
    }

    /// The session store is segregated: `<stage>/sessions` points at the
    /// aichat-owned store, NOT the device-wide `~/.pi/agent/sessions/`. Pi REPL
    /// history written through the stage lands in the store and never touches
    /// the real pi sessions dir.
    #[cfg(unix)]
    #[test]
    fn staged_agent_dir_segregates_sessions_to_given_store() {
        let real = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        // The real pi store has a session the REPL must NOT see or write to.
        std::fs::create_dir_all(real.path().join("sessions")).unwrap();
        std::fs::write(real.path().join("sessions/device.jsonl"), b"device-history").unwrap();

        let models = json!({"providers":{"aichat":{}}});
        let settings = json!({});
        let staged =
            StagedAgentDir::stage(real.path(), &models, &settings, store.path()).unwrap();
        let dir = staged.path.clone();

        // `sessions/` is a symlink to the aichat-owned store, not the real dir.
        assert!(dir.join("sessions").symlink_metadata().unwrap().file_type().is_symlink());
        let target = std::fs::read_link(dir.join("sessions")).unwrap();
        assert_eq!(target, store.path());
        // The device pi history is invisible through the stage.
        assert!(!dir.join("sessions/device.jsonl").exists());

        // A session pi writes through the stage persists in the store, not the
        // device dir.
        std::fs::write(dir.join("sessions/repl.jsonl"), b"repl-history").unwrap();
        assert_eq!(
            std::fs::read_to_string(store.path().join("repl.jsonl")).unwrap(),
            "repl-history"
        );
        assert!(!real.path().join("sessions/repl.jsonl").exists());

        // Cleanup removes the throwaway stage but leaves the persistent store
        // and the device dir untouched.
        staged.cleanup();
        assert!(!dir.exists());
        assert!(store.path().join("repl.jsonl").exists());
        assert!(real.path().join("sessions/device.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn staged_agent_dir_handles_missing_real_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("does-not-exist");
        let store = tmp.path().join("store");
        let models = json!({"providers":{}});
        let settings = json!({});
        let staged = StagedAgentDir::stage(&real, &models, &settings, &store).unwrap();
        assert!(staged.path.join("models.json").exists());
        assert!(staged.path.join("settings.json").exists());
        // The store is created even when the real agent dir is absent.
        assert!(store.is_dir());
        staged.cleanup();
    }

    #[test]
    fn segregated_sessions_dir_honors_env_override() {
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var(PI_SESSIONS_DIR_ENV, "/tmp/custom-pi-store") };
        let dir = segregated_pi_sessions_dir();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var(PI_SESSIONS_DIR_ENV) };
        assert_eq!(dir, PathBuf::from("/tmp/custom-pi-store"));
    }

    #[test]
    fn pi_launch_continues_recent_session_for_command_history() {
        // Pi keeps no standalone command-history file: the REPL's up-arrow
        // editor history is rebuilt from a *resumed* session's user messages
        // (pi `interactive-mode` populateHistory). A bare `pi` launch calls
        // `SessionManager.create` — a fresh, empty session — so command history
        // never carries across launches. aichat therefore passes `--continue`
        // on every pi launch so the most recent session for the cwd resumes
        // (pi `continueRecent` falls back to a fresh session when none exists,
        // so this is safe on the first launch).
        assert!(
            pi_repl_args().contains(&"--continue"),
            "pi must be launched with --continue so REPL command history persists"
        );
    }

    #[test]
    fn token_is_32_hex_chars() {
        let t = mint_bridge_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tokens_differ_per_call() {
        assert_ne!(mint_bridge_token(), mint_bridge_token());
    }

    #[test]
    fn embedded_bundle_is_present_and_nonempty() {
        let asset = PiExtensionsAsset::get(STAGED_EXTENSION_NAME)
            .expect("aichat-bridge.js must be embedded at build time");
        assert!(asset.data.len() > 0);
        // A few syntactic anchors that should appear in any reasonable
        // build of the extension. If the bundle changes shape, update.
        let text = std::str::from_utf8(asset.data.as_ref()).unwrap();
        assert!(text.contains("registerCommand"));
        assert!(text.contains("AICHAT_BRIDGE_URL"));
    }

    #[test]
    fn stage_writes_into_agent_dir_extensions_subdir() {
        // pi 0.79.1 auto-discovers extensions from `<PI_CODING_AGENT_DIR>/
        // extensions/`, NOT from `<cwd>/.pi/extensions/`. The bridge must land
        // in the agent dir's extensions subdir or pi never registers our
        // slash-commands.
        let agent = tempfile::tempdir().unwrap();
        let staged = StagedExtension::stage(agent.path()).unwrap();
        let expected = agent.path().join("extensions").join(STAGED_EXTENSION_NAME);
        assert_eq!(staged.path, expected);
        assert!(expected.exists());
        // The embedded bundle (with our slash-command registrations) was written.
        assert!(std::fs::read_to_string(&expected)
            .unwrap()
            .contains("registerCommand"));
    }

    #[test]
    fn stage_and_cleanup_round_trip() {
        let agent = tempfile::tempdir().unwrap();
        let staged = StagedExtension::stage(agent.path()).unwrap();
        assert!(staged.path.exists());
        let path = staged.path.clone();
        staged.cleanup();
        assert!(!path.exists());
    }

    #[test]
    fn stage_respects_existing_user_file() {
        let agent = tempfile::tempdir().unwrap();
        let ext_dir = agent.path().join("extensions");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let user_file = ext_dir.join(STAGED_EXTENSION_NAME);
        std::fs::write(&user_file, b"// user fork").unwrap();

        let staged = StagedExtension::stage(agent.path()).unwrap();
        // The user's content must still be there after our "stage" call.
        let after = std::fs::read_to_string(&user_file).unwrap();
        assert_eq!(after, "// user fork");

        // And cleanup must NOT delete a file we didn't write.
        staged.cleanup();
        assert!(user_file.exists());
    }

    #[test]
    fn is_authenticated_bridge_accepts_info_payload() {
        // aichat `/v1/state/info` on a correct token: 200 + {"info": "..."}.
        assert!(is_authenticated_bridge(200, r#"{"info":"model: gpt-4"}"#));
        assert!(is_authenticated_bridge(200, r#"{"info":""}"#));
    }

    #[test]
    fn is_authenticated_bridge_rejects_unauthorized() {
        // aichat bridge with a different token → 401.
        assert!(!is_authenticated_bridge(401, "Unauthorized"));
    }

    #[test]
    fn is_authenticated_bridge_rejects_not_found() {
        // Non-aichat server, or aichat started without a bridge token → 404.
        assert!(!is_authenticated_bridge(404, "Not Found"));
        assert!(!is_authenticated_bridge(404, r#"{"info":"smuggled"}"#));
    }

    #[test]
    fn is_authenticated_bridge_rejects_foreign_200() {
        // 200, but not the aichat bridge shape (e.g. some other JSON API,
        // or aichat's own `/v1/roles` payload).
        assert!(!is_authenticated_bridge(200, r#"{"data":[]}"#));
        assert!(!is_authenticated_bridge(200, r#"{"info":{"not":"a string"}}"#));
        assert!(!is_authenticated_bridge(200, "<html>not json at all</html>"));
    }

    /// Bind a throwaway TCP listener on `127.0.0.1` acting as a minimal
    /// aichat bridge. `GET /v1/state/info` with `Authorization: Bearer
    /// {token}` gets `200` + `{"info": ...}`; a wrong/absent token gets
    /// `401`. `token = None` makes every request `404` — i.e. a non-bridge
    /// server (a plain OpenAI API, or aichat `--serve` with no bridge token).
    async fn spawn_mock_bridge(token: Option<&'static str>) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let (status, body) = match token {
                        None => ("404 Not Found", "Not Found"),
                        Some(t) if req.contains(&format!("bearer {}", t.to_lowercase())) => {
                            ("200 OK", r#"{"info":"model: test"}"#)
                        }
                        Some(_) => ("401 Unauthorized", "Unauthorized"),
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len(),
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.flush().await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn probe_finds_an_authenticated_bridge() {
        let port = spawn_mock_bridge(Some("test-token")).await;
        assert_eq!(probe_port_range(port, port, "test-token").await, Some(port));
    }

    #[tokio::test]
    async fn probe_rejects_a_bridge_with_a_different_token() {
        let port = spawn_mock_bridge(Some("the-real-token")).await;
        assert_eq!(probe_port_range(port, port, "wrong-token").await, None);
    }

    #[tokio::test]
    async fn probe_ignores_a_non_bridge_server() {
        let port = spawn_mock_bridge(None).await;
        assert_eq!(probe_port_range(port, port, "any-token").await, None);
    }

    #[tokio::test]
    async fn probe_returns_none_when_nothing_listening() {
        // Bind then drop to obtain a port nothing is listening on.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert_eq!(probe_port_range(port, port, "any-token").await, None);
    }

    #[tokio::test]
    async fn probe_existing_server_honors_opt_out() {
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("AICHAT_NO_SERVER_PROBE", "1") };
        let result = probe_existing_server().await;
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("AICHAT_NO_SERVER_PROBE") };
        assert_eq!(result, None);
    }
}
