# AIChat Next-Generation Landscape Analysis

*March 2026*

## The Honest Landscape

### What reasoning models are doing to tools like aichat

The trajectory is clear — models are becoming their own orchestrators. Claude Code doesn't need a CLI wrapper; it *is* the CLI. It reads files, writes code, runs tests, manages git, calls MCP servers directly. Cursor, Aider, Codex — same pattern. The "chat with an LLM from your terminal" use case is being absorbed into tools that have deeper integration with the developer's actual workflow.

AIChat's REPL mode, session compression, and built-in RAG are being commoditized or made irrelevant by 1M+ context windows and purpose-built agents.

### What ISN'T being commoditized

1. **Provider neutrality.** Claude Code only runs Claude. AIChat talks to 20+ providers. That's a real moat.
2. **The llm-functions ecosystem.** The comment-to-JSON-Schema declaration pattern is genuinely elegant — write a bash function with `# @describe` and you get a tool. That's a universal tool library waiting to happen.
3. **The Unix composition model.** Reasoning models are great orchestrators, but they need small, composable, trustworthy tools to orchestrate. The "one tool per job" ethos is *more* relevant, not less.

## Directions Evaluated

### Direction 1: AIChat as a Tool Runtime (serve agents, not compete with them)

Instead of being the interface the human talks to, become the runtime that *agents* talk to.

```
Claude Code → MCP → aichat serve → llm-functions tools
                  → provider-neutral LLM calls
                  → role-based transformations
```

**Key moves:**
- Elevate `--serve` to first-class. Make aichat a headless tool-routing daemon that Claude Code (or any MCP client) calls.
- Expose roles as MCP tools: `aichat_role_execute(role="code-reviewer", input="...")` — now Claude Code can use your curated role library as specialized sub-agents.
- Expose provider switching as a capability: Claude Code could dispatch "use deepseek for this code generation, use claude for the review" through aichat's multi-provider routing.
- llm-functions already has `mcp/server/` — tighten the loop so it's zero-config.

**Why this works:** Claude Code is locked to Anthropic. A provider-neutral tool runtime with a curated function library fills a real gap. You become infrastructure, not interface.

### Direction 2: AIChat as a Pipeline Compositor (the `|` for AI)

Unix gave us `cat file | grep pattern | sort | uniq`. What's the equivalent for LLM operations?

```bash
# Today (single call)
aichat -r code-reviewer -f src/main.rs "review this"

# Next-gen (composable pipeline)
aichat pipe \
  --step "extract:deepseek-r1 -r extract-functions" \
  --step "review:claude -r security-reviewer" \
  --step "format:gpt4o -r markdown-formatter" \
  -f src/main.rs
```

**Key moves:**
- Add a `pipe` or `chain` mode where roles become pipeline stages.
- Each stage can use a different model (provider neutrality shines here).
- llm-functions tools can be stages too — mix LLM reasoning with deterministic code.
- Output schemas (`output_schema` already exists in roles) become the contract between stages.

**Why this works:** Reasoning models are expensive. A pipeline that uses a cheap model for extraction, a reasoning model for analysis, and a fast model for formatting is genuinely more efficient than throwing everything at one frontier model. This is something Claude Code *cannot* do natively.

### Direction 3: llm-functions as the Universal Tool Standard

The sleeper opportunity. The llm-functions pattern — write a function in bash/js/python with comments, get a tool — is simpler than MCP servers, more portable than OpenAI's function format, and already works.

**Key moves:**
- Publish llm-functions tools as a registry (like npm, but for LLM tools).
- Make `argc build` generate MCP server configs, OpenAI function schemas, AND Claude tool_use schemas from the same source.
- Create a `llm-functions init` that scaffolds a new tool in seconds.
- Build a test harness for tool validation.
- Position llm-functions as the "write once, use everywhere" tool authoring standard.

**Why this works:** Every agent framework needs tools. Nobody wants to rewrite their tools for each framework. A universal authoring format with multi-target compilation is a genuine unmet need.

## Chosen Direction: 1 + 3 Combined

### Rationale

The interface layer (REPL, pretty markdown rendering, interactive sessions) is a losing battle against purpose-built IDEs and agents. But the *runtime* layer — provider routing, role-based sub-agents, tool execution — is infrastructure that every agent needs and nobody else provides in this composable Unix-native way.

### Target Architecture

```
aichat serve --mcp
├── tool/*                   # llm-functions tools exposed via MCP
│   ├── execute_command
│   ├── web_search
│   └── ...
├── role/*                   # aichat roles as callable MCP tools
│   ├── code-reviewer
│   ├── security-audit
│   └── ...
├── llm/route                # provider-neutral model dispatch
│   ├── cheapest-for-task
│   ├── specific-provider
│   └── ensemble (ask N models, merge)
└── pipe/chain               # multi-step pipeline execution
```

Claude Code integration:
```json
{
  "mcpServers": {
    "aichat": {
      "command": "aichat",
      "args": ["serve", "--mcp"]
    }
  }
}
```

Now Claude Code gains access to 20+ providers, the entire role library, and all llm-functions tools — through a protocol it already speaks natively.

### The REPL's Future

The aichat REPL survives as a debug/authoring tool for roles and pipelines, not as the primary user-facing interface. The future human interface is either an IDE (Cursor/VS Code) or an agent (Claude Code). aichat's future is as the engine underneath, not the dashboard in front.
