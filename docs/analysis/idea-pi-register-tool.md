# Idea: expose aichat bridge verbs to pi's LLM via `registerTool()`

Status: investigation note (2026-07-03). No code yet.

## Context

The aichat↔pi bridge (`pi-extensions/src/index.ts`, bundled to
`assets/pi-extensions/aichat-bridge.js`) exposes aichat entity verbs to pi as
**slash commands only** — `pi.registerCommand()` × 10 (role, aichat-session,
rag, agent, macro, info, exit-context, aichat-flags, aichat-docs, aichat-edit).
Each is a human-typed command that HTTP-calls the aichat `/v1/state/*` or
`/v1/discovery/*` bridge.

Slash commands are **human-typed only**. pi's own agent loop (its LLM) cannot
invoke them mid-turn.

## Finding

pi has a second registration surface — `pi.registerTool()` — which registers
**LLM-callable tools** (pi's native equivalent of MCP tools; pi ships *no* MCP
by design, see `docs/analysis` MCP/ACP feasibility discussion).

- The aichat bridge does **not** use `registerTool()` (grep: 0 hits in
  `pi-extensions/`, `assets/pi-extensions/`; none in any branch history via
  `git log --all -S`).
- A prior assumption that "we already have registerTool() implemented" was
  **wrong** for the bridge.
- `registerTool()` **is** proven working in-tree: `.pi/extensions/searxng-extension/index.ts:7`
  registers a pi tool the LLM calls. So adopting it in the bridge is a
  copy-the-neighboring-pattern job, not a spike. No new dependency, no new
  protocol.

## Why this is the real abstraction win (not MCP/ACP)

- pi is **anti-MCP by design** (README:491 "No MCP", usage.md:306). It will
  never natively consume aichat-as-an-MCP-server; MCP into pi would itself
  require a pi extension — strictly worse than the bridge we already ship.
- pi's "RPC mode" is its own JSONL-over-stdio protocol, **not ACP**. ACP
  bridging stays external (`pi-acp`); native ACP in aichat not worth it (pi is
  already the ACP agent).
- Therefore the pi-native path to "let the model drive aichat context
  autonomously" is `registerTool()`, not a protocol change.
- MCP-over-HTTP remains worthwhile **only** as a *separate, additive* feature:
  expose aichat to *external* MCP hosts (Claude Desktop / Cursor / Zed-MCP).
  Decoupled from pi.

## Verb safety triage (which verbs become tools)

| Verb | Surface today | As LLM tool? |
|---|---|---|
| `info`, `aichat-flags`, `aichat-docs` | command | ✅ safe — read/compute, low blast radius |
| `macro` | command | ✅ likely — runs a named macro, output returned |
| `role`, `agent`, `session`, `rag`, `exit-context` | command | ⚠️ risky autonomous — mutate live context; an LLM flipping its own role mid-turn can derail. Keep human-command-only, or gate behind pi's permission flow |
| `aichat-edit` | command | ❌ never — opens interactive editor, needs human TTY |

Commands and tools are not mutually exclusive: a verb can stay a
`registerCommand` for humans AND add a `registerTool` for the LLM.

## Next steps (deferred)

1. Tool-ify the read-only verbs (`info`, `aichat-flags`, `aichat-docs`,
   maybe `macro`) mirroring the searxng `registerTool` shape.
2. Decide permission/gating for the context-mutating verbs.
3. Separately, spec the "expose aichat to external MCP hosts" feature
   (MCP-over-HTTP on the `--serve` process, reusing the rmcp `AichatMcpServer`
   machinery and the bridge bearer token).