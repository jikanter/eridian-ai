#!/usr/bin/env bats

# Documentation integration tests - Project Evolution

WIKI_DIR="docs/wiki"
EVOLUTION_DOC="$WIKI_DIR/Project-Evolution.md"

@test "evolution-doc: exists in wiki directory" {
  [ -f "$EVOLUTION_DOC" ]
}

@test "evolution-doc: contains UNIX Swiss Army Knife section" {
  grep -q "UNIX Swiss Army Knife" "$EVOLUTION_DOC"
}

@test "evolution-doc: contains Local-First section" {
  grep -q "Local-First" "$EVOLUTION_DOC"
}

@test "evolution-doc: mentions Ollama integration" {
  grep -qi "Ollama" "$EVOLUTION_DOC"
}

@test "evolution-doc: mentions Pipeline functionality" {
  grep -qi "pipeline" "$EVOLUTION_DOC"
}

@test "evolution-doc: mentions Epic 2 Resilience features" {
  grep -q "Runtime Intelligence" "$EVOLUTION_DOC"
  grep -q "Phase 10B" "$EVOLUTION_DOC"
}

@test "evolution-doc: mentions removal of RAG" {
  grep -qi "Removal of Traditional RAG" "$EVOLUTION_DOC"
}
