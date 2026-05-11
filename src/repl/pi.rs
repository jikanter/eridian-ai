//! Pi coding-agent launcher.
//!
//! Replaces the built-in Reedline REPL with the `pi` open-source coding-agent
//! harness ([github.com/earendil-works/pi]) when invoked. The Rust side
//!
//! 1. probes `pi` on `PATH`,
//! 2. binds an ephemeral TCP port on `127.0.0.1` and starts aichat's
//!    OpenAI-compatible server on it via [`crate::serve::run_on`],
//! 3. mints a per-launch bridge token and exposes the URL + token to the
//!    child via env vars (`AICHAT_BRIDGE_URL`, `AICHAT_BRIDGE_TOKEN`),
//! 4. stages the shipped TypeScript extension into `<cwd>/.pi/extensions/`
//!    so pi auto-discovers the slash-command bridge,
//! 5. execs `pi` with stdio inherited so the child owns the terminal,
//! 6. on pi exit, removes the staged extension (unless
//!    `AICHAT_KEEP_PI_STAGE=1`) and signals the server to shut down.
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

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("aichat bridge: failed to bind ephemeral port on 127.0.0.1")?;
    let addr = listener
        .local_addr()
        .context("aichat bridge: listener has no local address")?;
    let bridge_url = format!("http://127.0.0.1:{}", addr.port());
    let token = mint_bridge_token();

    // Stage the bridge extension under the CWD's `.pi/extensions/` before
    // exec'ing pi so pi's auto-discovery picks it up on startup. Project-
    // scoped staging — rather than `~/.pi/agent/extensions/` — keeps two
    // aichat invocations from racing on the same file and means a stray
    // process never leaves a global extension behind.
    let staging = StagedExtension::stage(std::env::current_dir()?.as_path())?;

    // The bridge token must be set in this process's env _before_ we call
    // serve::run_on so the Server picks it up in its constructor. Doing it
    // via Command::env on the child only would mean the in-process server
    // never sees the token and refuses every bridge call.
    std::env::set_var("AICHAT_BRIDGE_TOKEN", &token);

    let stop_server = serve::run_on(listener, config)
        .await
        .context("aichat bridge: failed to start in-process server")?;

    info!(
        "aichat bridge listening on {bridge_url}; launching pi from {}",
        pi_bin.display()
    );

    let spawn_result = Command::new(&pi_bin)
        .env("AICHAT_BRIDGE_URL", &bridge_url)
        .env("AICHAT_BRIDGE_TOKEN", &token)
        .status()
        .await;

    let _ = stop_server.send(());
    // Clear the token from our env so a subsequent in-process invocation
    // (e.g. tests reusing the binary) starts from a clean slate.
    std::env::remove_var("AICHAT_BRIDGE_TOKEN");
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
}
