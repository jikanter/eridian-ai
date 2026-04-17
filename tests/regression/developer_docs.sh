#!/usr/bin/env bats

# Regression tests for Developer Documentation described in Developer-Documentation.md

@test "dev-docs: mentions Mermaid diagrams" {
  run grep "mermaid" docs/wiki/Developer-Documentation.md
  [ "$status" -eq 0 ]
}

@test "dev-docs: mentions core modules" {
  run grep -E "CLI|Config|Pipeline|Client|Knowledge" docs/wiki/Developer-Documentation.md
  [ "$status" -eq 0 ]
}

@test "dev-docs: mentions MCP" {
  run grep "Model Context Protocol" docs/wiki/Developer-Documentation.md
  [ "$status" -eq 0 ]
}

@test "dev-docs: mentions Lifecycle Hooks" {
  run grep "Lifecycle Hooks" docs/wiki/Developer-Documentation.md
  [ "$status" -eq 0 ]
}

@test "dev-docs: contains source deep-dive section" {
  run grep "Source Code Deep-Dive" docs/wiki/Developer-Documentation.md
  [ "$status" -eq 0 ]
}

@test "dev-docs: uses correct sandbox root URL for source links" {
  run grep "http://mldev:3000/admin/aichat-private-sandbox/src/branch/main/src/" docs/wiki/Developer-Documentation.md
  [ "$status" -eq 0 ]
}

@test "dev-docs: links to critical source files" {
  grep "src/main.rs" docs/wiki/Developer-Documentation.md
  grep "src/pipe.rs" docs/wiki/Developer-Documentation.md
  grep "src/client/mod.rs" docs/wiki/Developer-Documentation.md
  grep "src/knowledge/compile.rs" docs/wiki/Developer-Documentation.md
  grep "src/config/preflight.rs" docs/wiki/Developer-Documentation.md
}
