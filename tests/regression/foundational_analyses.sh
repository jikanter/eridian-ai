#!/usr/bin/env bats

setup() {
    load 'common.bash'
}

@test "foundational analyses: wiki page exists" {
    [ -f "docs/wiki/FOUNDATIONAL-ANALYSES.md" ]
}

@test "foundational analyses: contains epic links with gitea convention" {
    grep -q "http://mldev:3000/admin/aichat-private-sandbox/src/branch/main/docs/analysis/epic-2.md" docs/wiki/FOUNDATIONAL-ANALYSES.md
    grep -q "http://mldev:3000/admin/aichat-private-sandbox/src/branch/main/docs/analysis/epic-10.md" docs/wiki/FOUNDATIONAL-ANALYSES.md
}

@test "foundational analyses: contains specific design doc links" {
    grep -q "http://mldev:3000/admin/aichat-private-sandbox/src/branch/main/docs/analysis/001-model-aware-variables.md" docs/wiki/FOUNDATIONAL-ANALYSES.md
    grep -q "http://mldev:3000/admin/aichat-private-sandbox/src/branch/main/docs/analysis/2026-03-16-simple-planning.md" docs/wiki/FOUNDATIONAL-ANALYSES.md
}

@test "foundational analyses: mentions philosophical context" {
    grep -q "UNIX Swiss Army Knife" docs/wiki/FOUNDATIONAL-ANALYSES.md
}
