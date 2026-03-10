This document outlines the strategic roadmap for enhancing `aichat`'s metadata front matter framework to support advanced, self-hosted AI use-cases and complex automated workflows.

**Date:** 2026-03-10  
**Status:** Strategic Proposal

#### Executive Summary
`aichat` is uniquely positioned to become the premier CLI tool for AI orchestration by leaning into the "Unix philosophy." While it currently excels as a robust, multi-model chat interface, its strategic value lies in transitioning into a **declarative workflow engine**. By moving complex configurations (context gathering, model chaining, post-processing) into the role's front matter metadata, `aichat` becomes the "`make` for AI workflows"—a lightweight, headless orchestrator that bridges the LLM with the local environment.

---

#### 1. Current State & Recent Refinements
The metadata front matter framework (implemented in `src/config/role.rs`) is already sophisticated, supporting inheritance (`extends`), multi-role mixins (`include`), and parameterization (`variables`).

**Recent Improvement: Intelligent Input De-hoisting**
The inheritance model has been refined to handle the `__INPUT__` placeholder more intuitively. When a role extends another:
- If the child re-declares `__INPUT__`, the parent's `__INPUT__` is stripped (child wins).
- If the child does not declare `__INPUT__`, the parent's `__INPUT__` is automatically moved to the end of the combined prompt.

This ensures that "base instructions" from a parent role always precede "refinements" from a child role, with the user's actual request (`__INPUT__`) positioned logically at the end.

---

#### 2. Strategic Enhancement: Shell-Injective Variables
The most powerful next step for `aichat` is the introduction of **Late-Binding Context** through shell-injective variables.

**The Idea:**
Allow `variables` in the role front matter to define shell commands as their default values.
```yaml
variables:
  - name: "git_diff"
    default: { shell: "git diff --cached" }
  - name: "project_structure"
    default: { shell: "ls -R | head -n 20" }
```

**Impact:**
- **Context Refresh:** Context is gathered *at the moment of invocation*, ensuring the LLM sees the current state of the filesystem or environment.
- **Unix Synergy:** It leverages existing CLI tools (`git`, `grep`, `find`) as "context providers" without building complex plugins.
- **Automation:** It eliminates the need for manual piping (e.g., `git diff | aichat --role reviewer`) by making the role self-contained.

---

#### 3. Strategic Enhancement: Pipeline Orchestration
While `aichat` currently supports multi-stage pipelines via the `--pipe` flag and `src/pipe.rs`, the next evolution is **Declarative Pipelines** defined directly within roles.

**The Idea:**
A single "meta-role" can trigger a sequence of model calls.
```yaml
---
pipeline:
  - role: "extractor"  # Fast model (e.g., Qwen 2.5)
  - role: "analyzer"   # Reasoning model (e.g., DeepSeek R1)
  - role: "formatter"  # Structured output model
---
```

**Impact:**
- **Cost/Speed Optimization:** Different models can be used for different stages of a single task.
- **Headless Workflows:** Complicated multi-step tasks (e.g., "Summarize these logs, find the error, and suggest a fix") can be invoked with a single command.

---

#### 4. Strategic Enhancement: Lifecycle Hooks
Following the "Unix composition" philosophy, roles should define not just how they *start*, but how they *finish*.

**The Idea:**
Introduce `on_success` or `pipe_to` hooks in the front matter.
```yaml
---
pipe_to: "pbcopy"      # Automatically copy LLM response to clipboard
save_to: "./logs/{{timestamp}}.md"
---
```

**Impact:**
- **Streamlined CX:** Reduces the friction of moving LLM output into other tools.
- **Workflow Automation:** Allows `aichat` to act as a trigger for other local scripts or processes.

---

#### 5. Strategic Enhancement: Unified Resource Binding
For self-hosted AI users, managing RAG indices and MCP servers should be as simple as selecting a role.

**The Idea:**
Bind specific local resources to a role definition.
```yaml
---
rag: "internal-docs"
mcp_servers:
  - "sqlite-server"
---
```

---

#### 6. Implementation Roadmap

| Phase | Feature | Component |
| :--- | :--- | :--- |
| **Short-term** | **Shell-Injective Variables** | Extend `src/utils/variables.rs` and `src/config/role.rs` to support `{ shell: "..." }` in variable defaults. |
| **Medium-term** | **Lifecycle Hooks** | Add `pipe_to` logic to the main execution loop (post-completion). |
| **Long-term** | **Declarative Pipelines** | Integrate `src/pipe.rs` logic into the `Role` resolution and execution flow. |

#### Conclusion
By implementing these features, `aichat` stays true to its core identity (minimalist, terminal-focused, fast) while providing the "missing link" for building sophisticated, local AI agents that are fully integrated with the user's shell environment.