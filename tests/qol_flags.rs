//! End-to-end tests for the `--install-deps` and `--demo` quality-of-life flags.
//!
//! Both are exercised through their `--dry-run` paths so no installer runs and
//! no model is called: `--install-deps --dry-run` prints the plan, and
//! `--demo <feature> --dry-run` prints the assembled selector prompt.

use std::path::PathBuf;
use std::process::Command;

fn aichat_binary() -> String {
    std::env::var("AICHAT_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_aichat").to_string())
}

/// Isolated empty config dir so these tests never read the developer's config.
fn isolated_config(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aichat-qol-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.yaml"),
        "model: openai:gpt-4o-mini\n\
         clients:\n\
         \x20 - type: openai\n\
         \x20   api_key: sk-fake-never-used\n",
    )
    .unwrap();
    dir
}

#[test]
fn install_deps_dry_run_prints_plan_for_all_three_tools() {
    let dir = isolated_config("install");
    let out = Command::new(aichat_binary())
        .env("AICHAT_CONFIG_DIR", &dir)
        .arg("--install-deps")
        .arg("--dry-run")
        .output()
        .expect("spawn aichat");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Install Deps"), "stdout: {stdout}");
    for tool in ["uv", "showboat", "pi"] {
        assert!(stdout.contains(tool), "plan missing {tool}: {stdout}");
    }
    // Every line is either a skip or a concrete install command — never blank.
    assert!(
        stdout.contains("skip") || stdout.contains("install:"),
        "stdout: {stdout}"
    );
}

#[test]
fn demo_dry_run_prints_tuned_prompt_listing_real_demos() {
    let dir = isolated_config("demo");
    // cwd defaults to the crate root, where docs/demos/ lives.
    let out = Command::new(aichat_binary())
        .env("AICHAT_CONFIG_DIR", &dir)
        .arg("--demo")
        .arg("mcp server")
        .arg("--dry-run")
        .output()
        .expect("spawn aichat");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("mcp server"), "feature missing: {stdout}");
    assert!(stdout.contains("docs/demos"), "demos dir missing: {stdout}");
    assert!(stdout.contains("NO MATCH"), "no-match instruction missing");
    // A real demo file from this repo must appear in the listing.
    assert!(
        stdout.contains("demo-mcp-server.md"),
        "expected a scanned demo filename: {stdout}"
    );
}
