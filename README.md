# Rocky: All-in-one LLM CLI Tool

> *"Amaze!" — Rocky, upon discovering you can pipe seventeen LLM providers through a single CLI*

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/mr3ZZUB9hG)

Rocky is a multi-target command-line tool for AI models — like having a five-armed Eridian engineer who can talk to every LLM at once. Shell Assistant, CMD & REPL Mode, RAG, AI Tools & Agents, MCP, composable pipelines, and more.

> This project is a fork of [sigoden/aichat](https://github.com/sigoden/aichat), a fantastic foundation for LLM CLI tooling. Rocky builds on that work with a focus on token efficiency, composable multi-model pipelines, and MCP integration — because even Eridians know you don't build a interstellar tunnel without a good blueprint to fork from.

## Install

### Package Managers

- **Rust Developers:** `cargo install aichat`
- **Homebrew/Linuxbrew Users:** `brew install aichat`
- **Pacman Users**: `pacman -S aichat`
- **Windows Scoop Users:** `scoop install aichat`
- **Android Termux Users:** `pkg install aichat`

### Pre-built Binaries

Download pre-built binaries for macOS, Linux, and Windows from [GitHub Releases](https://github.com/sigoden/aichat/releases), extract them, and add the binary to your `$PATH`.

## Features

### Multi-Provider Integration

Integrate seamlessly with over 20 leading LLM providers through a unified interface. One CLI, all the models — Rocky would call this "good good good engineering."

Supported providers include OpenAI, Claude, Gemini (Google AI Studio), Ollama, Groq, Azure-OpenAI, VertexAI, Bedrock, Github Models, Mistral, Deepseek, AI21, XAI Grok, Cohere, Perplexity, Cloudflare, OpenRouter, Ernie, Qianwen, Moonshot, ZhipuAI, MiniMax, Deepinfra, VoyageAI, and any OpenAI-Compatible API provider.

### CMD Mode

Single-shot command-line invocations for quick queries and scripted workflows.

![aichat-cmd](https://github.com/user-attachments/assets/6c58c549-1564-43cf-b772-e1c9fe91d19c)

### REPL Mode

Interactive Chat-REPL with tab autocompletion, multi-line input, history search, configurable keybindings, and custom prompts.

![aichat-repl](https://github.com/user-attachments/assets/218fab08-cdae-4c3b-bcf8-39b6651f1362)

### Shell Assistant

Describe tasks in natural language and get precise shell commands, intelligently adjusted for your OS and shell environment.

![aichat-execute](https://github.com/user-attachments/assets/0c77e901-0da2-4151-aefc-a2af96bbb004)

### Multi-Form Input

Accept diverse input forms — stdin, local files and directories, remote URLs, external commands, and combinations.

| Input             | CMD                                  | REPL                             |
| ----------------- | ------------------------------------ | -------------------------------- |
| CMD               | `aichat hello`                       |                                  |
| STDIN             | `cat data.txt \| aichat`            |                                  |
| Last Reply        |                                      | `.file %%`                       |
| Local files       | `aichat -f image.png -f data.txt`    | `.file image.png data.txt`       |
| Local directories | `aichat -f dir/`                     | `.file dir/`                     |
| Remote URLs       | `aichat -f https://example.com`      | `.file https://example.com`      |
| External commands | ```aichat -f '`git diff`'```        | ```.file `git diff` ```          |
| Combine Inputs    | `aichat -f dir/ -f data.txt explain` | `.file dir/ data.txt -- explain` |

### Roles

Customize roles to tailor LLM behavior with YAML frontmatter configuration. Roles support model pinning, temperature control, input/output schema validation, variables (plain and shell-injective), lifecycle hooks, MCP server binding, inheritance, and multi-stage pipelines.

![aichat-role](https://github.com/user-attachments/assets/023df6d2-409c-40bd-ac93-4174fd72f030)

> The role consists of a prompt and model configuration — the fundamental unit of composition, like xenonite is to Eridian engineering.

### Session

Maintain context-aware conversations through sessions with automatic compression at configurable token thresholds.

![aichat-session](https://github.com/user-attachments/assets/56583566-0f43-435f-95b3-730ae55df031)

> The left side uses a session, while the right side does not use a session.

### Macro

Streamline repetitive tasks by combining a series of REPL commands into a custom macro with YAML-based workflow orchestration and positional variables.

![aichat-macro](https://github.com/user-attachments/assets/23c2a08f-5bd7-4bf3-817c-c484aa74a651)

### RAG

Integrate external documents into your LLM conversations for more accurate and contextually relevant responses. Supports configurable embedding models, chunk parameters, and custom document loaders.

![aichat-rag](https://github.com/user-attachments/assets/359f0cb8-ee37-432f-a89f-96a2ebab01f6)

### Function Calling & Tools

Function calling supercharges LLMs by connecting them to external tools and data sources. Rocky uses deferred tool loading to keep token costs low — when you have 15+ tools, only a `tool_search` meta-function is loaded initially, cutting token overhead by ~85%.

> Rocky's approach to tools: "Why carry all five arms' worth of equipment when you can fetch what you need?" *Amaze!*

We have created a new repository [https://github.com/sigoden/llm-functions](https://github.com/sigoden/llm-functions) to help you make the most of this feature.

#### AI Tools & MCP

Integrate external tools to automate tasks, retrieve information, and perform actions directly within your workflow. Consume MCP (Model Context Protocol) servers both locally (stdio) and remotely (HTTP/SSE) with config-based server management.

![aichat-tool](https://github.com/user-attachments/assets/7459a111-7258-4ef0-a2dd-624d0f1b4f92)

#### AI Agents (CLI version of OpenAI GPTs)

AI Agent = Instructions (Prompt) + Tools (Function Callings) + Documents (RAG). Directory-based entities with their own tool functions, RAG documents, dynamic instructions, and composable role inheritance.

![aichat-agent](https://github.com/user-attachments/assets/0b7e687d-e642-4e8a-b1c1-d2d9b2da2b6b)

### Composable Pipelines

Multi-stage pipelines where roles can call other roles as tools — Rocky's answer to Anthropic's Programmatic Tool Calling. Chain models together, validate schemas between stages, and build complex workflows from simple, composable pieces.

> If Rocky taught a masterclass in software architecture, lesson one would be: "Is same same as tunnel building. Many small piece, good good good. One big piece, bad bad bad."

### Token-Efficient Output Formats

Multiple output formats optimized for different consumers:

- `-o json` — Validated JSON output
- `-o jsonl` — JSON Lines (one object per line)
- `-o tsv` — Tab-separated values
- `-o csv` — Comma-separated values
- `-o text` — Plain text (default)
- `-o compact` — Minimal tokens for agent consumption

### Semantic Exit Codes

11 distinct exit codes for agent-friendly error classification. When something goes wrong, Rocky doesn't just say "is bad" — you get structured error results with hints, stderr capture, and retry budgeting. Machines deserve good error messages too.

### Lifecycle Hooks

`pipe_to` streams output to external commands; `save_to` writes results to files with timestamp templating. Wire your LLM outputs into any Unix pipeline.

### Shell-Injective Variables

Execute shell commands at role invocation time for dynamic context injection. Variables like `{{$ENV_VAR}}` bridge environment state into your prompts.

### Local Server Capabilities

Includes a lightweight built-in HTTP server with CORS restrictions for local deployment.

```
$ aichat --serve
Chat Completions API: http://127.0.0.1:8000/v1/chat/completions
Embeddings API:       http://127.0.0.1:8000/v1/embeddings
Rerank API:           http://127.0.0.1:8000/v1/rerank
LLM Playground:       http://127.0.0.1:8000/playground
LLM Arena:            http://127.0.0.1:8000/arena?num=2
```

#### Proxy LLM APIs

Test with curl:

```sh
curl -X POST -H "Content-Type: application/json" -d '{
  "model":"claude:claude-3-5-sonnet-20240620",
  "messages":[{"role":"user","content":"hello"}],
  "stream":true
}' http://127.0.0.1:8000/v1/chat/completions
```

#### LLM Playground

A web application to interact with supported LLMs directly from your browser.

![aichat-llm-playground](https://github.com/user-attachments/assets/aab1e124-1274-4452-b703-ef15cda55439)

#### LLM Arena

A web platform to compare different LLMs side-by-side — because even Rocky would want to benchmark Eridian computation against Earth's finest models before committing to a design.

![aichat-llm-arena](https://github.com/user-attachments/assets/edabba53-a1ef-4817-9153-38542ffbfec6)

## Custom Themes

Supports custom dark and light themes, which highlight response text and code blocks.

![aichat-themes](https://github.com/sigoden/aichat/assets/4012553/29fa8b79-031e-405d-9caa-70d24fa0acf8)

## Documentation

- [Chat-REPL Guide](https://github.com/sigoden/aichat/wiki/Chat-REPL-Guide)
- [Command-Line Guide](https://github.com/sigoden/aichat/wiki/Command-Line-Guide)
- [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide)
- [Macro Guide](https://github.com/sigoden/aichat/wiki/Macro-Guide)
- [RAG Guide](https://github.com/sigoden/aichat/wiki/RAG-Guide)
- [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables)
- [Configuration Guide](https://github.com/sigoden/aichat/wiki/Configuration-Guide)
- [Custom Theme](https://github.com/sigoden/aichat/wiki/Custom-Theme)
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)
- [FAQ](https://github.com/sigoden/aichat/wiki/FAQ)

## Why "Rocky"?

Named after the Eridian engineer from Andy Weir's *Project Hail Mary* — a five-armed, rock-shelled alien who doesn't speak your language but will absolutely out-engineer you while communicating in musical chords. Rocky solves impossible problems with whatever tools are available, composes elegant solutions from simple parts, and does it all with relentless optimism.

That's the vibe. This CLI doesn't care which LLM you speak — it'll bridge the gap, compose the pipeline, and get the job done. *Good good good.*

> "You are my friend. I will never abandon you." — Rocky, who clearly never had to debug a YAML frontmatter parser

## License

Copyright (c) 2023-2025 Rocky contributors. Forked from [sigoden/aichat](https://github.com/sigoden/aichat).

Rocky is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
