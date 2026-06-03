# CLI help text: phase labels removed

*2026-06-03T22:46:39Z by Showboat 0.6.1*
<!-- showboat-id: e7f996d1-b08a-42a2-a1dc-1b1bae628c2f -->

CLI arg help came from `///` doc comments in `src/cli.rs`. Many were prefixed with internal roadmap phase labels (`Phase 23B:`, `(Phase 10B)`). Those leak internal sequencing into user-facing `--help`. Removed; help text now describes behavior only.

```bash
target/debug/aichat --help | grep -c -i "phase"
```

```output
0
```

Zero `phase` mentions remain in rendered help output.

```bash
target/debug/aichat --help | grep -E -- "--compare|--no-cache|--explain-role|--knowledge-compile"
```

```output
      --compare <ROLE1> <ROLE2>
      --no-cache
      --explain-role <NAME>
      --knowledge-compile <KB_NAME>
```
