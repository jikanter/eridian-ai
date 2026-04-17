#!/usr/bin/env bats

# Regression tests for Demos features described in DEMOS.md

@test "demos: doc mentions showboat" {
  run grep -i "showboat" docs/wiki/DEMOS.md
  [ "$status" -eq 0 ]
}

@test "demos: doc links to simonw/showboat" {
  run grep "github.com/simonw/showboat" docs/wiki/DEMOS.md
  [ "$status" -eq 0 ]
}

@test "demos: doc mentions aichat specific patterns" {
  # Check if it mentions dry-runs or pipelines as suggested in previous issue solution
  run grep -E "dry-run|pipeline|knowledge" docs/wiki/DEMOS.md
  [ "$status" -eq 0 ]
}
