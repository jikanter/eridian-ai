#!/usr/bin/env bats

# Regression tests for Command-Line features described in Command-Line-Guide.md
load common.bash

@test "cli: --version prints version" {
  run ./target/debug/aichat --version
  [ "$status" -eq 0 ]
  [[ "$output" == "aichat "* ]]
}

@test "cli: --help prints help" {
  run ./target/debug/aichat --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
}

@test "cli: --list-models lists models" {
  run ./target/debug/aichat --list-models
  [ "$status" -eq 0 ]
}

@test "cli: --list-roles lists roles" {
  run ./target/debug/aichat --list-roles
  [ "$status" -eq 0 ]
}

@test "cli: --info displays system info" {
  run ./target/debug/aichat --info
  [ "$status" -eq 0 ]
  [[ "$output" == *"config_file"* ]]
}

@test "cli: --dry-run 'hello' works" {
  run_aichat --dry-run "hello"
  [ "$status" -eq 0 ]
}

@test "cli: -c --dry-run (code mode) works" {
  run_aichat -c --dry-run "fibonacci in js"
  [ "$status" -eq 0 ]
}

@test "cli: --no-stream works with --dry-run" {
  run_aichat --no-stream --dry-run "test"
  [ "$status" -eq 0 ]
}
