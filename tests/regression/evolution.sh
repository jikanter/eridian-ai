#!/usr/bin/env bats

# Regression tests for Evolution features described in Project-Evolution.md

@test "evolution: doc mentions UNIX Swiss Army Knife" {
  run grep "UNIX Swiss Army Knife" docs/wiki/Project-Evolution.md
  [ "$status" -eq 0 ]
}

@test "evolution: doc mentions local models" {
  run grep -i "local model" docs/wiki/Project-Evolution.md
  [ "$status" -eq 0 ]
}

@test "evolution: doc mentions Epic 9 / Knowledge Compilation" {
  run grep -E "Epic 9|knowledge compilation" docs/wiki/Project-Evolution.md
  [ "$status" -eq 0 ]
}

@test "evolution: doc mentions pipelines" {
  run grep "Pipelines as First-Class Citizens" docs/wiki/Project-Evolution.md
  [ "$status" -eq 0 ]
}
