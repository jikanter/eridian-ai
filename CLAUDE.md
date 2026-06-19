# Project Overview 

AIchat is a command-line swiss army knife for ai applications.

<brief:generated>
# Briefing: Eridian - Your Alien AI Assistant

**Stack:** Rust 1.95.0, Cargo, Bash, argc, llm-functions, python3.13, nodejs, pi, brief, astrophage, javascript

## Reference Context

Read these files for background before starting work:

<context>
- @docs/README.md
- @docs/ROADMAP.md
- @docs/architecture/integrated-architecture/README.md
- @docs/architecture/architecture.md
</context>

## Deliverable

<deliverable>
A multi-tool for integrated interactions with AI models.A useful set of extensions to the pi harness for interaction with the tool.User documentation in docs/featuresAn innovative ways to integrate structured ai workflows
</deliverable>

## Hard

- Use [Roadmap.md](docs/ROADMAP.md) as the source of truth for the roadmap.
- Use the test-driven development skill for all code
- Use the showboat tool to build evergreen demos
- No desktop UI
- Approach each task as accurate and analytical instead of encouraging
- It is ok to say you do not know the answer to a question. Saying you don't know and doing the research is an infinitely better outcome than making up an answer.
- If given information about a problem, use *ONLY* that information to solve the task and not your memory
- System is designed to run as optimally on local models as frontier models.
- Token and cost conscious.
- When running `showboat note`, output in an evergreen fashion. This is important for `showboat validate` to work.
- If asked to implement against a standard (agentskills, mcp, acp, http3, etc.), download and cache that standard in docs/reference/standards
- Batch use-cases only, leverages [pi](https://github.com/earendil-works/pi) wrapper for repl work
- The Entity is the authoring counterpart. Prompt / Role / Agent / Macro are *presets* over one
    `Entity` substrate; the runtime speaks one trait. `resolve Entity → execute → emit Trace`.
- The trace is the keystone Testing, evaluation, training extraction, and observability all
    read one structured artifact — per-tool data models are confined to syntactic sugar (i.e. repl).
- Add integration tests via bats in addition to unit tests. Bats tests should be written alongside any code feature use test driven development.

## Soft

- This tool should function using the "one tool per job" ethos. Unix composition over monolithic features.
- This tool should use the 'showboat' command to demo its work. Use the output of `showboat --help` to understand how to implement.
- Implement both repl and batch interaction surfaces for functionality
- User docs live in docs/features. Docs catering more to agents live in the rest of the docs/ folder.
- Read through the files in the https://github.com/simonw/showboat/blob/main/docs/plans/ and add any skills to the project.
- [SECURITY.md](SECURITY.md) contains security requirements.

## Ask First

- No desktop UI, all Ux work should happen in the terminal.
- Introducing incompatibility with the existing tooling for aichat (argc)
- Breaking changes with [llm-functions](https://github.com/sigoden/llm-functions), [argc](https://github.com/sigoden/argc), [brief](https://github.com/jikanter/brief), or [astophage](https://github.com/jikanter/astrophage)
- Introduction of new programming languages
- Significant increase in number of dependencies
</brief:generated>

## Integrated requirements

- Use the /beetle-git-coordination tool to quickly coordinate work across multiple projects.
- [architecture.md](./docs/architecture/architecture.md) contains cross-platform architecture.

## About

Made with ❤️ in Chicago by [Jordan Kanter](https://www.jordankanter.com)