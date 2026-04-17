#!/usr/bin/env bats

# Developer Documentation integration tests

DOC_FILE="docs/wiki/DeveloperDocumentation.md"

@test "developer-doc: exists" {
  [ -f "$DOC_FILE" ]
}

@test "developer-doc: contains mermaid diagrams" {
  grep -q "\`\`\`mermaid" "$DOC_FILE"
}

@test "developer-doc: mentions core modules" {
  grep -q "Pipeline Engine" "$DOC_FILE"
  grep -q "Configuration & Role Management" "$DOC_FILE"
  grep -q "Client & Provider Layer" "$DOC_FILE"
  grep -q "Knowledge Management" "$DOC_FILE"
}

@test "developer-doc: mentions data flows" {
  grep -q "Data Flow: Pipeline Execution" "$DOC_FILE"
  grep -q "Data Flow: Knowledge Compilation" "$DOC_FILE"
}

@test "developer-doc: mentions MCP and hooks" {
  grep -q "Model Context Protocol (MCP)" "$DOC_FILE"
  grep -q "Lifecycle Hooks" "$DOC_FILE"
}
