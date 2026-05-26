# Claude Code Memory Research

<Discovery>
- Claude Code holds its project memory in `~/.claude/projects/<path>/`, where path is a dash seperated absolute path of the project 
    directory.
</Discovery>

## Directory contents
<Discovery>
The directory contains a set of jsonl files (that appear like traces) of each turn per project,
as well as optional additional project context.

One such kind of context is a folder "memory", which contains 
markdown-with-yaml frontmatter. See the [MemoryReadme](examples/claude/-home-admin-Developer-Scripts/memory/MEMORY.md) 
for such an example.
</Discovery>

## See also

The Claude Code memory discipline documented above is operationalised in the roadmap as Epic 14 "Memory Surface":

- [`../../roadmap/phase-34-overview.md`](../../roadmap/phase-34-overview.md) — Auto-Memory Wiring (freeform `memory/` side; Theme 2 of the 2026-05-24 divergence playbook)
- [`../../roadmap/phase-34-auto-memory.md`](../../roadmap/phase-34-auto-memory.md) — deep design for the dual-writer architecture
- [`../../roadmap/phase-35-overview.md`](../../roadmap/phase-35-overview.md) — Knowledge-MCP Protocol (typed `knowledge/` side exposed via Anthropic's `memory_20250818` op set; Theme 1)
- [`../../roadmap/phase-35-knowledge-mcp.md`](../../roadmap/phase-35-knowledge-mcp.md) — deep design for the op-by-op mapping
