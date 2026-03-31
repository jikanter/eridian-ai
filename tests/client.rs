//! End-to-end HTTP client tests for the aichat serve endpoints.
//!
//! Launches the `aichat` binary with `--serve` on a random port,
//! then validates each endpoint returns the expected JSON shape.

use reqwest::StatusCode;
use serde_json::Value;
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

/// Find an available port by binding to :0 and reading back the assigned port.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Start the aichat server and return (child process, base_url).
/// Waits until the server is accepting connections.
async fn start_server() -> (std::process::Child, String) {
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let base_url = format!("http://{addr}");

    let child = Command::new(env!("CARGO_BIN_EXE_aichat"))
        .arg("--serve")
        .arg(&addr)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to start aichat server");

    // Poll until the server accepts a connection (up to 5 seconds)
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client.get(format!("{base_url}/v1/models")).send().await.is_ok() {
            return (child, base_url);
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("server did not start within 5 seconds");
}

#[tokio::test]
async fn test_models_endpoint() {
    let (mut child, base_url) = start_server().await;
    let resp = reqwest::get(format!("{base_url}/v1/models")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("application/json"));

    let body: Value = resp.json().await.unwrap();
    assert!(body["data"].is_array(), "models response must have a 'data' array");
    // Every model entry must have an "id" and "object" field
    for model in body["data"].as_array().unwrap() {
        assert!(model["id"].is_string(), "model must have 'id': {model}");
        assert_eq!(model["object"], "model", "model 'object' must be 'model': {model}");
    }

    child.kill().ok();
}

#[tokio::test]
async fn test_roles_endpoint() {
    let (mut child, base_url) = start_server().await;
    let resp = reqwest::get(format!("{base_url}/v1/roles")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["data"].is_array(), "roles response must have a 'data' array");
    for role in body["data"].as_array().unwrap() {
        assert!(role["name"].is_string(), "role must have 'name': {role}");
    }

    child.kill().ok();
}

#[tokio::test]
async fn test_prompts_endpoint() {
    let (mut child, base_url) = start_server().await;
    let resp = reqwest::get(format!("{base_url}/v1/prompts")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["data"].is_array(), "prompts response must have a 'data' array");
    for prompt in body["data"].as_array().unwrap() {
        assert!(prompt["name"].is_string(), "prompt must have 'name': {prompt}");
        assert!(prompt["content"].is_string(), "prompt must have 'content': {prompt}");
    }

    child.kill().ok();
}

#[tokio::test]
async fn test_rags_endpoint() {
    let (mut child, base_url) = start_server().await;
    let resp = reqwest::get(format!("{base_url}/v1/rags")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = resp.json().await.unwrap();
    assert!(body["data"].is_array(), "rags response must have a 'data' array");
    // Each entry is a string (rag name)
    for rag in body["data"].as_array().unwrap() {
        assert!(rag.is_string(), "rag entry must be a string: {rag}");
    }

    child.kill().ok();
}

#[tokio::test]
async fn test_playground_endpoint() {
    let (mut child, base_url) = start_server().await;
    let resp = reqwest::get(format!("{base_url}/playground")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("text/html"));

    child.kill().ok();
}

#[tokio::test]
async fn test_not_found_returns_404() {
    let (mut child, base_url) = start_server().await;
    let resp = reqwest::get(format!("{base_url}/v1/nonexistent")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    child.kill().ok();
}
