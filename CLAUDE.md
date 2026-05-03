# Project Overview 

AIchat is a command-line swiss army knife for ai applications.

<brief:generated>
# Briefing: Eridian - Your Alien AI Assistant

**Stack:** Rust 1.95.0, Cargo, Bash, argc, llm-functions, python3.13

## Reference Context

Read these files for background before starting work:
- @docs/README.md

## Deliverable
A multi-tool for integrated interactions with AI models.

## Hard

- System is designed to run as optimally on local models as frontier models.
- System is token cost conscious
- When running `showboat note`, output in an evergreen fashion. This is important for `showboat validate` to work.

## Soft

- This tool should function using the "one tool per job" unix ethos
- This tool should use the 'showboat' command to demo its work. Use the output of `showboat --help` to understand how to implement.
- Implement both repl and batch interaction surfaces for functionality
- Read through the files in the https://github.com/simonw/showboat/blob/main/docs/plans/ and add any skills to the project.

## Ask First

- No desktop UI, all Ux work should happen in the terminal.
- Introducing incompatibility with the existing tooling for aichat (argc)
- Reduced compatibility with [llm-functions](https://github.com/sigoden/llm-functions), [argc](https://github.com/sigoden/argc), or [brief](https://github.com/jikanter/brief)
- Introduction of new programming languages
- Significant increase in number of dependencies
</brief:generated>

## Integrated requirements

Requirements that span more than one project (aichat ↔ llm-functions ↔ the future harness interface) live in `docs/roadmap/integrated-architecture/`. Anything purely internal to aichat stays in `docs/roadmap/` or `docs/architecture/`. See `docs/roadmap/integrated-architecture/README.md` for what qualifies.

## About

Made with ❤️ in Chicago by [Jordan Kanter](https://www.jordankanter.com)