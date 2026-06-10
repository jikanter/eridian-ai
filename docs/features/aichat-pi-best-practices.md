# 🛠️ Pi $\leftrightarrow$ Aichat: Integration Best Practices

This guide outlines how to develop aichat so it functions as a seamless "Capability Suite" for the pi agent.

## Integration of Pi and Aichat

1. The Core Philosophy: "The Tool is the Specialist"

aichat is designed for two distinct modes of existence. To ensure deep integration, every feature must work in both.

- Mode A: The Independent Tool (Batch/Direct)
  - Scenario: The user runs aichat --prompt "...".
  - Requirement: The tool must be self-contained, deterministic, and follow the Unix "one tool per job" ethos.
 - Mode B: The Agentic Tool (REPL/Pi-driven)
  - Scenario: The user is in the pi REPL. pi decides it needs to perform a task and executes aichat [options].
    - Requirement: The tool must be "agent-friendly"—providing structured, predictable output that an LLM can easily parse.

 ────────────────────────────────────────────────────────────────────────────────

 2. RPC Implementation Best Practices

 Since pi interacts with aichat via subprocesses (shelling out), your "RPC" is essentially the command-line interface.

### A. Output is the API

 When pi calls a tool, the "response" is the stdout of that command.
 - Use --output json or --output compact: For tool-to-tool or agent-to-tool communication, always provide a
   machine-readable format.
 - Avoid "Chatter" on stdout: If a command performs a task but also prints status messages (e.g., "Processing..."),
   ensure those messages go to stderr. pi (and the LLM) will typically ignore stderr or treat it as metadata, while
   stdout is treated as the "result."
 - The "Agent-Ready" Format: Use the compact output format for tool calls. This minimizes token usage by stripping
   unnecessary whitespace or decorative elements, making it easier for the LLM to process the result.

### B. Error Handling via Exit Codes

 - Non-zero exits are your friend: If a tool call fails, aichat must exit with a non-zero status. pi uses these
   exit codes to understand that a tool execution failed, allowing it to trigger "retry" or "error handling" logic
   in the agent.
 - Detailed stderr: When a failure occurs, provide a clear, diagnostic error message on stderr. This provides the
   "reason" the agent needs to decide its next move.

 ### C. Idempotency and State

 - Stateless by Default: Since every pi tool call is a fresh process, your tools should ideally be stateless.
 - Session Management: If a tool requires state (e.g., a conversation history), use the --session or --save-session
    flags. pi can manage the lifecycle of these sessions, effectively giving the agent "memory" through your tool.

 ────────────────────────────────────────────────────────────────────────────────

 3. Advanced Integration: The "Agentic" Features

 ### A. Tool Discovery (via --list-tools and --mcp)

 Your tool should be able to describe itself.
 - If you implement MCP (Model Context Protocol) support, pi can automatically discover your aichat capabilities.
 - Ensure that --list-tools or --tool-info provides clear, high-quality descriptions that an LLM can use to
   understand when and how to call your tool.

 ### B. The "Knowledge Base" Loop

 Use the --knowledge flags to bridge the gap between your tool's data and pi's context.
 - A tool that "compiles" a knowledge base (--knowledge-compile) can be used by pi to create a persistent,
   searchable context for the agent.

────────────────────────────────────────────────────────────────────────────────

4. Summary Checklist for New Features

When adding a new command or tool to aichat, ask:

When developing or adding a new feature to aichat, validate it against this checklist:

 ┌─────────────────────────────────────────────────────────────┬─────────────┐
 │ Question                                                    │ Target Mode │
 ├─────────────────────────────────────────────────────────────┼─────────────┤
 │ Can this be used as a standalone command without pi?        │ Batch       │
 ├─────────────────────────────────────────────────────────────┼─────────────┤
 │ Does stdout contain only the requested result?              │ REPL        │
 ├─────────────────────────────────────────────────────────────┼─────────────┤
 │ Are all status updates and logs sent to stderr?             │ REPL        │
 ├─────────────────────────────────────────────────────────────┼─────────────┤
 │ Does a failure result in a non-zero exit code?              │ REPL        │
 ├─────────────────────────────────────────────────────────────┼─────────────┤
 │ Is the output format optimized for an LLM's context window? │ REPL        │
 ├─────────────────────────────────────────────────────────────┼─────────────┤
 │ Can the tool be used to manage its own state via sessions?  │ REPL        │
 └─────────────────────────────────────────────────────────────┴─────────────┘

 By following these rules, you ensure that aichat is not just a collection of scripts, but a professional-grade
 Capability Suite that transforms the pi REPL into an infinitely powerful agentic environment.
