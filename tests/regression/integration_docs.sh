#!/usr/bin/env bats

@test "Integration Guide exists" {
  [ -f "docs/wiki/Integration.md" ]
}

@test "Integration Guide mentions argc" {
  grep -i "argc" docs/wiki/Integration.md
}

@test "Integration Guide mentions llm-functions" {
  grep -i "llm-functions" docs/wiki/Integration.md
}

@test "Integration Guide contains Mermaid diagram" {
  grep "mermaid" docs/wiki/Integration.md
}

@test "Integration Guide contains upstream links" {
  grep "https://github.com/sigoden/llm-functions" docs/wiki/Integration.md
  grep "https://github.com/sigoden/argc" docs/wiki/Integration.md
}

@test "Integration Guide describes the orchestrator role of aichat" {
  grep -i "orchestrator" docs/wiki/Integration.md
}
