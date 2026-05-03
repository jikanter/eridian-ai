# Integrated Architecture

This directory holds requirements, plans, and design notes that span more than one project. The current set of integrated systems is:

- **aichat** (this repo) — the CLI / runtime / MCP server-and-client. Origin: [github.com/jikanter/aichat-private](https://github.com/jikanter/aichat-private).
- **llm-functions** ([github.com/jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions)) — tool and agent declarations consumed by aichat. Symlinked from `~/Library/Application Support/aichat/functions` in development.
- **harness interface** — a future surface (TBD) that will let other clients (Claude Code, Cursor, etc.) consume aichat's exposed roles, tools, and MCP-pool servers as a single unit.

## Dependency management


A document belongs here when its requirements only make sense across two or more of those systems — e.g., a change in aichat's MCP routing that depends on a tool-naming convention shared with llm-functions, or a harness-side feature that needs both aichat and llm-functions to agree on a registration scheme.

Documents that live entirely inside one project belong in that project's own roadmap or design directory.

## Cross-repo independence

Documents in this directory link between repos via **GitHub URLs**, not local filesystem paths. A user with only one repo cloned must be able to follow the docs to the other repo via GitHub. This rule applies in both directions: aichat docs link to llm-functions on GitHub; llm-functions docs link to this directory on GitHub.

The portable artifacts these documents specify (e.g., `mcp.json` per [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md)) live **outside** both repos, in user-level config paths.

## Index

- [`SPEC-mcp-json-artifact.md`](SPEC-mcp-json-artifact.md) — Schema and discovery rules for the portable `mcp.json` declarations file. Aligned with Claude Code's `.mcp.json` dialect; aichat-specific extensions namespaced under `x-aichat`. Foundational input to bridge retirement.
- [`bridge-retirement.md`](bridge-retirement.md) — Plan to retire the Node HTTP bridge in `llm-functions/mcp/bridge/` in favor of the portable `mcp.json` artifact + aichat's native `mcp_pool`. Status: blocked on two upstream aichat bugs; tests and demo pinned in aichat to track readiness.
- [`MIGRATION-portable-mcp-json.md`](MIGRATION-portable-mcp-json.md) — User-facing, hand-followable migration steps for moving from `llm-functions/mcp.json` + the bridge to the portable `~/.config/mcp/mcp.json`. Includes rollback.