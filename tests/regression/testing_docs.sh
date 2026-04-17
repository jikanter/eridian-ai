#!/usr/bin/env bats

@test "testing_docs: TESTING.md exists" {
  [ -f "docs/wiki/TESTING.md" ]
}

@test "testing_docs: mentions bats and rust" {
  run grep -i "BATS" docs/wiki/TESTING.md
  [ "$status" -eq 0 ]
  run grep -i "Rust" docs/wiki/TESTING.md
  [ "$status" -eq 0 ]
}

@test "testing_docs: contains comparison table" {
  run grep "| Feature | Rust Unit/Integration Tests | BATS Regression Tests |" docs/wiki/TESTING.md
  [ "$status" -eq 0 ]
}

@test "testing_docs: contains in-vivo testing section" {
  run grep "## Advanced: In-Vivo Testing" docs/wiki/TESTING.md
  [ "$status" -eq 0 ]
  run grep "AICHAT_TEST_MODEL" docs/wiki/TESTING.md
  [ "$status" -eq 0 ]
}

@test "testing_docs: links to bats documentation" {
  run grep "https://bats-core.readthedocs.io/" docs/wiki/TESTING.md
  [ "$status" -eq 0 ]
}
