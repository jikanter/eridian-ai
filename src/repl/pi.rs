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
//! 4. stages the shipped TypeScript extension into `<cwd>/.pi/extensions/`
//!    so pi auto-discovers the slash-command bridge,
//! 5. execs `pi` with stdio inherited so the child owns the terminal,
//! 6. on pi exit, removes the staged extension (unless
//!    `AICHAT_KEEP_PI_STAGE=1`) and signals the server to shut down (a
//!    reused external server is left running).
//!
//! [github.com/earendil-works/pi]: https://github.com/earendil-works/pi

use anyhow::{bail, Context, Result};
use rust_embed::Embed;
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;
use tokio::process::Command;

use crate::config::GlobalConfig;
use crate::serve;

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

/// Launch `pi` as the REPL surface, with aichat's HTTP server running
/// in-process on an ephemeral port. Blocks until pi exits.
pub async fn launch_pi(config: &GlobalConfig) -> Result<()> {
    let pi_bin = match which::which("pi") {
        Ok(p) => p,
        Err(_) => bail!("{PI_INSTALL_HINT}"),
    };

    // Stage the bridge extension under the CWD's `.pi/extensions/` before
    // exec'ing pi so pi's auto-discovery picks it up on startup. Project-
    // scoped staging — rather than `~/.pi/agent/extensions/` — keeps two
    // aichat invocations from racing on the same file and means a stray
    // process never leaves a global extension behind.
    let staging = StagedExtension::stage(std::env::current_dir()?.as_path())?;

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
            // The bridge token must be set in this process's env _before_ we
            // call serve::run_on so the Server picks it up in its
            // constructor. Doing it via Command::env on the child only would
            // mean the in-process server never sees the token and refuses
            // every bridge call.
            std::env::set_var("AICHAT_BRIDGE_TOKEN", &token);
            let stop = serve::run_on(listener, config)
                .await
                .context("aichat bridge: failed to start in-process server")?;
            info!("aichat bridge listening on {url}");
            (url, Some(token), Some(stop))
        }
    };

    info!("launching pi from {}", pi_bin.display());

    let mut command = Command::new(&pi_bin);
    command.env("AICHAT_BRIDGE_URL", &bridge_url);
    if let Some(token) = &token {
        command.env("AICHAT_BRIDGE_TOKEN", token);
    }
    let spawn_result = command.status().await;

    // Signal the in-process server to shut down (no-op when we reused one).
    let we_started_server = stop_server.is_some();
    if let Some(stop) = stop_server {
        let _ = stop.send(());
    }
    // Clear a token we set so a subsequent in-process invocation (e.g. tests
    // reusing the binary) starts clean. When reusing a server we never set
    // it, so a user-exported token is left untouched.
    if we_started_server {
        std::env::remove_var("AICHAT_BRIDGE_TOKEN");
    }
    // Clean up the staged extension unless the user asked us not to. The
    // escape hatch is handy when debugging extension load failures.
    if std::env::var_os("AICHAT_KEEP_PI_STAGE").is_none() {
        staging.cleanup();
    }

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
/// not own the `.pi/` or `.pi/extensions/` directories: another tool may
/// have its own extensions staged there too.
struct StagedExtension {
    path: PathBuf,
    /// True when we actually wrote a file at `path` (vs. found one already
    /// present that we should leave alone).
    we_created_it: bool,
}

impl StagedExtension {
    fn stage(cwd: &Path) -> Result<Self> {
        let ext_bytes = match PiExtensionsAsset::get(STAGED_EXTENSION_NAME) {
            Some(f) => f.data,
            None => bail!(
                "aichat was built without the pi extension bundle (assets/pi-extensions/{STAGED_EXTENSION_NAME})",
            ),
        };
        let ext_dir = cwd.join(".pi").join("extensions");
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
            // Try to prune `.pi/extensions/` and `.pi/` if we left them
            // empty. Ignore failures — another tool may share the dirs.
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::remove_dir(parent);
                if let Some(grandparent) = parent.parent() {
                    let _ = std::fs::remove_dir(grandparent);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn stage_and_cleanup_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = StagedExtension::stage(tmp.path()).unwrap();
        assert!(staged.path.exists());
        let parent = staged.path.parent().unwrap().to_path_buf();
        staged.cleanup();
        assert!(!parent.join(STAGED_EXTENSION_NAME).exists());
    }

    #[test]
    fn stage_respects_existing_user_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ext_dir = tmp.path().join(".pi").join("extensions");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let user_file = ext_dir.join(STAGED_EXTENSION_NAME);
        std::fs::write(&user_file, b"// user fork").unwrap();

        let staged = StagedExtension::stage(tmp.path()).unwrap();
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
        std::env::set_var("AICHAT_NO_SERVER_PROBE", "1");
        let result = probe_existing_server().await;
        std::env::remove_var("AICHAT_NO_SERVER_PROBE");
        assert_eq!(result, None);
    }
}
