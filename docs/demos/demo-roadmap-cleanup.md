# Roadmap Sub-Document Cleanup

*2026-04-08T02:56:25Z by Showboat 0.6.1*
<!-- showboat-id: 1b306578-c79a-4500-a09b-77199a74767d -->

The unified roadmap expanded AIChat from 5 epics / 19 phases to 10 epics / 29 phases. This demo verifies that the sub-documents (epic design docs and phase detail files) are consistent with the new numbering in ROADMAP.md.

Verify epic design docs exist for all 10 epics (2-10) and that their titles match the roadmap.

```bash
head -1 docs/analysis/epic-{2,3,4,5,6,7,8,9,10}.md
```

```output
==> docs/analysis/epic-2.md <==
# Epic 2: Runtime Intelligence Layer

==> docs/analysis/epic-3.md <==
# Epic 3: Composition UX

==> docs/analysis/epic-4.md <==
# Epic 4: Typed Ports & Capabilities

==> docs/analysis/epic-5.md <==
# Epic 5: Server Pipeline Engine

==> docs/analysis/epic-6.md <==
# Epic 6: Universal Addressing

==> docs/analysis/epic-7.md <==
# Epic 7: DAG Execution

==> docs/analysis/epic-8.md <==
# Epic 8: Feedback Loop

==> docs/analysis/epic-9.md <==
# Epic 9: RAG Evolution — Structured Retrieval & Composability

==> docs/analysis/epic-10.md <==
# Epic 10: Entity Evolution & Agent Dynamism
```

Verify phase detail files exist with correct numbering (gaps at 12-15, 19-24 are expected — those are new phases without detail files yet).

```bash
ls docs/roadmap/phase-*.md | sed 's|docs/roadmap/||' | sort -t- -k2 -n
```

```output
phase-0-prerequisites.md
phase-1-token-efficiency.md
phase-2-pipeline-output.md
phase-3-mcp-consumption.md
phase-4-error-handling.md
phase-5-remote-mcp.md
phase-6-metadata-framework.md
phase-7-error-messages.md
phase-8-data-observability.md
phase-9-schema-fidelity.md
phase-10-resilience.md
phase-11-context-budget.md
phase-16-server-hardening.md
phase-17-server-execution.md
phase-18-server-discovery.md
phase-25-rag-structured.md
phase-26-rag-composability.md
phase-27-rag-graph.md
phase-28-agent-composability.md
phase-29-agent-dynamism.md
```

Verify epic numbers in phase files match the roadmap (Epic 5=Server, Epic 9=RAG, Epic 10=Entity).

```bash
grep '^\*\*Epic:' docs/roadmap/phase-{16,17,18,25,26,27,28,29}*.md | sed 's|docs/roadmap/||'
```

```output
phase-16-server-hardening.md:**Epic:** 5 — Server Pipeline Engine
phase-17-server-execution.md:**Epic:** 5 — Server Pipeline Engine
phase-18-server-discovery.md:**Epic:** 5 — Server Pipeline Engine
phase-25-rag-structured.md:**Epic:** 9 — RAG Evolution
phase-26-rag-composability.md:**Epic:** 9 — RAG Evolution
phase-27-rag-graph.md:**Epic:** 9 — RAG Evolution
phase-28-agent-composability.md:**Epic:** 10 — Entity Evolution & Agent Dynamism
phase-29-agent-dynamism.md:**Epic:** 10 — Entity Evolution & Agent Dynamism
```

Verify moved features in epic-10.md are marked as moved to Epic 6.

```bash
grep 'Moved to Epic 6' docs/analysis/epic-10.md
```

```output
## ~~Feature 4: Unified Entity Resolution~~ → Moved to Epic 6 (Universal Addressing), Phase 19B
## ~~Feature 6: Agent-in-Pipeline~~ → Moved to Epic 6 (Universal Addressing), Phase 19C
## ~~Feature 7: Agent MCP Binding~~ → Moved to Epic 6 (Universal Addressing), Phase 19D
| F4 (unified resolution) | -- | **Moved to Epic 6 Phase 19B** |
| F6 (agent-in-pipeline) | -- | **Moved to Epic 6 Phase 19C** |
| F7 (agent MCP binding) | -- | **Moved to Epic 6 Phase 19D** |
```

Verify no stale references to old numbering exist in updated files. Search for old phase numbers that should have been remapped.

```bash
echo 'Old server refs (12A-14C) in new files:' && grep -r '12[A-H]\.\|13[A-E]\.\|14[A-C]\.' docs/roadmap/phase-16*.md docs/roadmap/phase-17*.md docs/roadmap/phase-18*.md docs/analysis/epic-5.md || echo '  None found (clean)'
```

```output
Old server refs (12A-14C) in new files:
  None found (clean)
```

Summary of changes:

```bash
cat <<'SUMMARY'
Epic design docs updated:
  epic-2.md  — Added note about 10D/10E reorder, 11D addition, epic cross-refs
  epic-5.md  — Renumbered from Epic 3 (Server Pipeline Engine), phases 12-14 → 16-18
  epic-9.md  — Renumbered from Epic 4 (RAG Evolution), phases 15-17 → 25-27
  epic-10.md — Renumbered from Epic 5 (Entity Evolution), phases 18-19 → 28-29
               Features F4/F6/F7 marked as moved to Epic 6 Phase 19

New epic design docs created (thin stubs, reference ROADMAP.md):
  epic-3.md  — Composition UX (Phases 12-13)
  epic-4.md  — Typed Ports & Capabilities (Phases 14-15)
  epic-6.md  — Universal Addressing (Phases 19-20)
  epic-7.md  — DAG Execution (Phases 21-22)
  epic-8.md  — Feedback Loop (Phases 23-24)

Phase detail files renamed and updated:
  phase-12 → phase-16 (Server Hardening, Epic 5)
  phase-13 → phase-17 (Server Execution, Epic 5)
  phase-14 → phase-18 (Server Discovery, Epic 5)
  phase-15 → phase-25 (RAG Structured, Epic 9)
  phase-16 → phase-26 (RAG Composability, Epic 9)
  phase-17 → phase-27 (RAG Graph, Epic 9)
  phase-18 → phase-28 (Agent Composability, Epic 10)
  phase-19 → phase-29 (Agent Dynamism, Epic 10)

Phase 28 items trimmed: 28B/28D/28E moved to Epic 6 Phase 19.
SUMMARY
```

```output
Epic design docs updated:
  epic-2.md  — Added note about 10D/10E reorder, 11D addition, epic cross-refs
  epic-5.md  — Renumbered from Epic 3 (Server Pipeline Engine), phases 12-14 → 16-18
  epic-9.md  — Renumbered from Epic 4 (RAG Evolution), phases 15-17 → 25-27
  epic-10.md — Renumbered from Epic 5 (Entity Evolution), phases 18-19 → 28-29
               Features F4/F6/F7 marked as moved to Epic 6 Phase 19

New epic design docs created (thin stubs, reference ROADMAP.md):
  epic-3.md  — Composition UX (Phases 12-13)
  epic-4.md  — Typed Ports & Capabilities (Phases 14-15)
  epic-6.md  — Universal Addressing (Phases 19-20)
  epic-7.md  — DAG Execution (Phases 21-22)
  epic-8.md  — Feedback Loop (Phases 23-24)

Phase detail files renamed and updated:
  phase-12 → phase-16 (Server Hardening, Epic 5)
  phase-13 → phase-17 (Server Execution, Epic 5)
  phase-14 → phase-18 (Server Discovery, Epic 5)
  phase-15 → phase-25 (RAG Structured, Epic 9)
  phase-16 → phase-26 (RAG Composability, Epic 9)
  phase-17 → phase-27 (RAG Graph, Epic 9)
  phase-18 → phase-28 (Agent Composability, Epic 10)
  phase-19 → phase-29 (Agent Dynamism, Epic 10)

Phase 28 items trimmed: 28B/28D/28E moved to Epic 6 Phase 19.
```
