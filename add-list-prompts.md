# Add: aichat --list-prompts flag

*2026-04-06T15:51:47Z by Showboat 0.6.1*
<!-- showboat-id: d02b3143-99ce-48f5-bd48-03f9f148b786 -->

Added a new flag --list-prompts to list prompts from the configuration directory.

It supports both plain text (default) and JSON output (via -o json).

```bash
mkdir -p ~/.config/aichat/prompts && echo 'Test content' > ~/.config/aichat/prompts/demo-prompt.md
```

```output
```

```bash
./target/debug/aichat --list-prompts | grep demo-prompt
```

```output
```
