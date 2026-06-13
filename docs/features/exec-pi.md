# Exec pi passthrough (`--exec-pi`)

`--exec-pi` runs [`pi`](https://github.com/earendil-works/pi) in non-interactive
mode (`pi -p`) with aichat's customizations loaded first, then forwards every
remaining argument straight to pi and propagates pi's exit status.

Use it when you want a one-shot pi run that inherits the same environment
aichat would set up — env file, provider config — without dropping into the
full interactive REPL.

## Usage

`--exec-pi` **must be the first argument.** Everything after it is passed to
`pi -p` verbatim:

```bash
aichat --exec-pi "List all .ts files in src/"
aichat --exec-pi --tools read,grep,find,ls "Review the code in src/"
```

These are equivalent to:

```bash
pi -p "List all .ts files in src/"
pi -p --tools read,grep,find,ls "Review the code in src/"
```

The difference: aichat loads its customizations (`.env` file, config) before
handing off, so pi runs in aichat's prepared environment.

## Semantics

- **First-argument only.** `--exec-pi` is honored only in the first position.
  `aichat chat --exec-pi ...` is a normal aichat run, not a passthrough — the
  flag is parsed by aichat (and rejected as unknown).
- **`-p` is implicit.** aichat prepends `-p` (pi's non-interactive flag) before
  your arguments. Do not add it yourself.
- **Exit status is propagated.** aichat exits with pi's exit code, so the
  passthrough composes in scripts and pipelines.
- **stdio is inherited.** pi's stdout/stderr stream straight to your terminal;
  aichat adds no wrapping.

## Requirements

`pi` must be on `PATH`. See [REPL via pi](repl-pi.md) for install instructions.

## When to use which

| Goal | Command |
| --- | --- |
| Interactive REPL on aichat inference | `aichat` (see [repl-pi.md](repl-pi.md)) |
| One-shot pi run in aichat's environment | `aichat --exec-pi "<prompt>"` |
| Raw pi, no aichat setup | `pi -p "<prompt>"` |
