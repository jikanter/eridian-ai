# Demo: find the showboat demo for a feature

`--demo <feature>` asks a model to locate the [showboat](https://github.com/simonw/showboat)
demo under `docs/demos/` that best matches a feature, and prints its path. It
turns "I know there's a demo for this somewhere" into one command, without
grepping ~50 demo files by hand.

## Usage

```bash
aichat --demo "mcp server"
```

```
path: docs/demos/demo-mcp-server.md
why: It walks through `aichat --mcp` and the mcp_servers config — the server side of MCP.
```

If nothing matches, the model is instructed to say so rather than invent a file:

```
NO MATCH — closest is knowledge compilation (phase-25-knowledge-compilation.md)
```

Run it from the aichat repo root (where `docs/demos/` lives). When the directory
has no demos, the command errors instead of calling the model.

## Preview the prompt

`--dry-run` prints the assembled selector prompt instead of calling the model —
useful for tuning, or to see the full demo inventory:

```bash
aichat --demo "auto memory" --dry-run
```

The prompt lists every demo as `filename — title`, where the title is the demo's
first `# ` heading (falling back to the filename stem).

## How the prompt is tuned

The selector prompt is specialized for this repository:

- It states that demos live in `docs/demos/` as showboat markdown, named
  `demo-<topic>.md` or `phase-<n>-<topic>.md`.
- It includes the live, scanned inventory — so the model only ever sees demos
  that actually exist.
- It **constrains the choice to the listed filenames** ("do not invent
  filenames"), and pins the output shape to `path:` / `why:` or `NO MATCH`.

Because the inventory is scanned at call time from `docs/demos/`, the answer
can't reference a demo that was deleted or miss one that was just added.

## See also

- [discovery.md](discovery.md) — find flags and feature docs (the sibling "what can this do?" surface)
- [install-deps.md](install-deps.md) — install `showboat` itself
