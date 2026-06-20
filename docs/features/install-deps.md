# Install deps: bootstrap the companion tools

`--install-deps` installs the external companion tools aichat's workflows lean
on — [`uv`](https://github.com/astral-sh/uv),
[`showboat`](https://github.com/simonw/showboat), and
[`pi`](https://github.com/earendil-works/pi) — skipping any already on PATH. One
command to get a fresh checkout demo-ready.

## Usage

Preview the plan first (recommended) — `--dry-run` shows exactly what would run
without touching anything:

```bash
aichat --install-deps --dry-run
```

```
--- Install Deps ---
  uv         present, skip
  showboat   install: uv tool install showboat
  pi         install: npm install -g @earendil-works/pi-coding-agent
```

Then run it for real:

```bash
aichat --install-deps
```

Each tool already on PATH is skipped; the rest are installed in order. Progress
and the exact command for each tool print to stderr.

## What it installs

| Tool | Probe | Install command |
|---|---|---|
| `uv` | `uv` | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| `showboat` | `showboat` | `uv tool install showboat` |
| `pi` | `pi` | `npm install -g @earendil-works/pi-coding-agent` |

Order matters: `uv` is installed first because the canonical `showboat` install
(`uv tool install showboat`) needs `uv` present.

## Idempotent and ordered

Presence is probed with `which`, so re-running `--install-deps` only installs
what is actually missing. Installation stops at the first failure with a
non-zero exit and the failing command, rather than pressing on into a
half-installed state.

## Security

`--install-deps` is opt-in and shells out. It runs the tools' **official**
install commands — including `uv`'s documented `curl … | sh` bootstrap. Every
command is printed before it runs, and `--dry-run` lets you review the full plan
without executing anything. Inspect the plan if you have not run these
installers before; pin or vendor them yourself if your environment forbids
network installs.

## See also

- [repl-pi.md](repl-pi.md) — `pi` is aichat's default REPL harness
- The project README's "Use the showboat tool to build evergreen demos" guidance
