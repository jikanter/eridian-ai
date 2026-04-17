#!/usr/bin/env bats

# Documentation integration tests - Showboat Demos

DEMOS_DOC="docs/wiki/DEMOS.md"

@test "demos-doc: exists" {
  [ -f "$DEMOS_DOC" ]
}

@test "demos-doc: mentions Simon Willison" {
  grep -q "Simon Willison" "$DEMOS_DOC"
}

@test "demos-doc: links to showboat source/docs" {
  grep -q "https://simonwillison.net/2026/Feb/10/showboat/" "$DEMOS_DOC"
}

@test "demos-doc: mentions core showboat commands" {
  grep -q "showboat init" "$DEMOS_DOC"
  grep -q "showboat note" "$DEMOS_DOC"
  grep -q "showboat exec" "$DEMOS_DOC"
}

@test "demos-doc: contains aichat-specific examples" {
  grep -q "aichat --pipe" "$DEMOS_DOC"
  grep -q "aichat --knowledge-compile" "$DEMOS_DOC"
}
