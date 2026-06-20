//! End-to-end tests for `--explain-context`.
//!
//! Launches the `aichat` binary against an isolated config dir with a trivial
//! role and a deliberately fake API key, then asserts the assembled-context
//! breakdown is printed and that no provider call is made (the fake key would
//! fail a real request, so a clean exit proves the model was never called).

use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn aichat_binary() -> String {
    std::env::var("AICHAT_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_aichat").to_string())
}

/// Build an isolated config dir with one role and a fake-key client. Unique per
/// test name so parallel test threads don't collide.
fn setup_config(test_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aichat-explain-{}-{}",
        std::process::id(),
        test_name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("roles")).unwrap();
    std::fs::write(
        dir.join("config.yaml"),
        "model: openai:gpt-4o-mini\n\
         clients:\n\
         \x20 - type: openai\n\
         \x20   api_key: sk-fake-never-used\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("roles").join("greeter.md"),
        "---\nmodel: openai:gpt-4o-mini\n---\nYou are a terse greeter.\n",
    )
    .unwrap();
    dir
}

/// Run `aichat -r greeter --explain-context [extra]` with `stdin` piped in.
/// Returns (success, stdout).
fn run_explain(config_dir: &PathBuf, stdin: &str, extra: &[&str]) -> (bool, String) {
    let mut child = Command::new(aichat_binary())
        .env("AICHAT_CONFIG_DIR", config_dir)
        .arg("-r")
        .arg("greeter")
        .arg("--explain-context")
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn aichat");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("aichat did not exit");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

#[test]
fn explain_context_prints_section_breakdown_without_calling_model() {
    let dir = setup_config("human");
    let (ok, stdout) = run_explain(&dir, "hello world", &[]);

    assert!(ok, "explain-context should exit cleanly; stdout: {stdout}");
    assert!(stdout.contains("Context Explain"), "stdout: {stdout}");
    assert!(stdout.contains("system"), "stdout: {stdout}");
    assert!(stdout.contains("user"), "stdout: {stdout}");
    assert!(stdout.contains("TOTAL"), "stdout: {stdout}");
    // The role's system prompt must show up in the assembled context.
    assert!(stdout.contains("terse greeter"), "stdout: {stdout}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explain_context_emits_structured_json() {
    let dir = setup_config("json");
    let (ok, stdout) = run_explain(&dir, "hello world", &["-o", "json"]);

    assert!(ok, "explain-context -o json should exit cleanly; stdout: {stdout}");
    let v: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected JSON, got {stdout:?}: {e}"));

    let sections = v["sections"].as_array().expect("sections array");
    assert!(!sections.is_empty());
    assert!(sections.iter().any(|s| s["label"] == "system"));
    assert!(sections.iter().any(|s| s["label"] == "user"));
    assert!(v["total_tokens"].is_number());

    let _ = std::fs::remove_dir_all(&dir);
}
