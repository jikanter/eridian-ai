# Explain context: see what lands in the window

`--explain-context` is a richer dry-run. Where `--dry-run` answers "what config
will this run with?", `--explain-context` answers the context-engineering
question: **what context actually lands in the model's window, and where do the
tokens go?**

It builds the fully assembled context — the role's system prompt, the injected
[auto-memory](auto-memory.md) block, any output-format suffix, the user turn,
and the tool schemas — then prints a per-section token breakdown and exits
**before any provider call**. No client, no credentials, zero tokens spent.

## Usage

```bash
echo "hello there, friend" | aichat -r greeter --explain-context
```

```
--- Context Explain ---
  system                 37 tok   90%  You are a terse greeter... # Project memory - …
  user                    4 tok    9%  hello there, friend
  TOTAL                  41 tok  (218 bytes)
```

Each row is one assembled message (labelled by role: `system`, `user`,
`assistant`, `tool`), showing its estimated tokens, its share of the total, and
a single-line preview. When the role selects tools, a final `tools (N)` row
accounts for the serialized tool schemas the provider receives.

## JSON

Pair with `-o json` for machine consumption — e.g. asserting a token budget in
CI, or diffing context across roles:

```bash
echo "hi" | aichat -r greeter --explain-context -o json
```

```json
{
  "sections": [
    { "label": "system", "tokens": 75, "bytes": 363, "preview": "You are a terse greeter. ..." },
    { "label": "user",   "tokens": 4,  "bytes": 20,  "preview": "hi" }
  ],
  "total_tokens": 79,
  "total_bytes": 383
}
```

## Why it's faithful

The breakdown is computed from the same `ChatCompletionsData` that
`prepare_completion_data` hands the provider — the exact messages and tool
schemas that would be sent. So the injected memory preamble and the `-o json`
output suffix show up in the report precisely where they land at request time;
there is no separate "preview" assembly that could drift from the real one.

Token counts are estimates from the shared `estimate_token_length` heuristic
(the same one the context-budget and knowledge subsystems use), not a
provider-exact tokenizer — read them as proportions, not billing figures.

## See also

- [auto-memory.md](auto-memory.md) — the memory block that appears in the `system` section
- [typed-input.md](typed-input.md) — how slot-based input shapes the user turn
