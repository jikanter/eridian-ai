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
