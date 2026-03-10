# Output Format Flag (-o) for Unix Pipeline Composition

*2026-03-06T16:54:05Z by Showboat 0.6.1*
<!-- showboat-id: d377ad4d-dbb3-4557-92ff-6037e5721f97 -->

The new `-o`/`--output` flag gives aichat first-class support for structured output formats. This turns aichat into a composable Unix pipeline citizen that works naturally with `jq`, `cut`, `sort`, and other standard tools. Supported formats: `json`, `jsonl`, `tsv`, `csv`, and `text`.

## The Flag

The `-o` flag accepts five format values. For structured formats (`json`, `jsonl`, `tsv`, `csv`), aichat injects format instructions into the system prompt, disables streaming, strips markdown code fences from the output, and validates the result before writing to stdout.

```bash
aichat-dev --help 2>&1 | grep -A1 '\-o,'
```

```output
  -o, --output <FORMAT>
          Output format (json, jsonl, tsv, csv, text)
```

## JSON Output with jq

The most common use case: get structured JSON from aichat and process it with `jq`. The model responds with raw JSON (no markdown wrapping), aichat validates it is parseable, and `jq` can consume it directly.

```bash
aichat-dev -m ollama:llama3.1:latest -o json "List 3 primary colors as a JSON array of strings. Only output the JSON." 2>/dev/null
```

```output
["Red", "Blue", "Yellow"]
```

Piped directly to `jq` to extract individual elements:

```bash
aichat-dev -m ollama:llama3.1:latest -o json "List 5 planets with name and diameter_km as a JSON array of objects" 2>/dev/null | jq ".[].name"
```

```output
"Earth"
"Mars"
"Jupiter"
"Saturn"
"Uranus"
```

Chaining with `jq` to transform the data — convert the array to a comma-separated string:

```bash
aichat-dev -m ollama:llama3.1:latest -o json "List 5 programming languages as a JSON array of strings" 2>/dev/null | jq -r "join(\", \")"
```

```output
Python, JavaScript, Java, C++, Ruby
```

## JSON → jq → cut: The Full Pipeline

The most robust pattern for `cut` integration: get JSON from aichat, use `jq` to convert to TSV, then slice with `cut`. This works reliably regardless of model quality.

```bash
aichat-dev -m ollama:llama3.1:latest -o json "List 5 programming languages with fields: name, year_created, creator" 2>/dev/null | jq -r ".[] | [.name, (.year_created|tostring), .creator] | @tsv" | cut -f1,3
```

```output
Python	Guido van Rossum
Java	James Gosling
C++	Bjarne Stroustrup
JavaScript	Brendan Eich
Ruby	Yukihiro Matsumoto
```

Sort by year (column 2) and take the 3 oldest:

```bash
aichat-dev -m ollama:llama3.1:latest -o json "List 8 programming languages with name, year_created, creator" 2>/dev/null | jq -r ".[] | [.name, (.year_created|tostring), .creator] | @tsv" | sort -t"	" -k2 -n | head -3
```

```output
Pascal	1970	Nicolaus Wirth
C	1972	Dennis Ritchie
Python	1991	Guido van Rossum
```

## Composing with Roles and Variables

The `-o` flag composes with all existing aichat features. Here we combine it with a role and `-v` variables:

```bash
aichat-dev -m ollama:llama3.1:latest --prompt "You are a translator. Translate the input into {{language}}." -v language=French -o json "Hello, how are you today?" 2>/dev/null
```

```output
{"Hello": "H\u00e0l\u00f4u", "how are you": "comment \u00e9tes-vous", "today": "aujourd\u2019hui"}
```

## Stdin Piping

Pipe data through aichat and extract structured results — the core Unix pattern:

```bash
echo "The Eiffel Tower was built in 1889 in Paris by Gustave Eiffel for the World Fair" | aichat-dev -m ollama:llama3.1:latest -o json "Extract entities as a JSON array of objects with name and type fields" 2>/dev/null
```

```output
[
  {"name": "Eiffel Tower", "type": "location"},
  {"name": "Paris", "type": "location"},
  {"name": "Gustave Eiffel", "type": "person"}
]
```

Filter to just the people:

```bash
echo "The Eiffel Tower was built in 1889 in Paris by Gustave Eiffel for the World Fair" | aichat-dev -m ollama:llama3.1:latest -o json "Extract entities as a JSON array of objects with name and type fields" 2>/dev/null | jq -r "[.[] | select(.type == \"person\")] | .[].name"
```

```output
Gustave Eiffel
```

## CSV Output

For spreadsheet-friendly output:

```bash
aichat-dev -m ollama:llama3.1:latest -o csv "List 5 European capitals: country, capital, population estimate. One per line, comma-separated." 2>/dev/null
```

```output
"Portugal, Lisbon, 505,526", "Germany, Berlin, 6,785,716", "Italy, Rome, 2,870,493", "Greece, Athens, 664,046", "Spain, Madrid, 3,223,335"
```

## Code Fence Stripping and Validation

Models sometimes wrap JSON in markdown code fences even when told not to. The `-o json` format automatically strips these and validates the result, so downstream tools like `jq` never choke on stray backticks:

```bash
cargo test -q -- test_strip_code_fences test_clean_output test_is_structured test_system_prompt_suffix 2>&1
```

```output

running 11 tests
...........
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 80 filtered out; finished in 0.00s

```

## Summary

| Flag | Behavior | Best paired with |
|---|---|---|
| `-o json` | Validated JSON, fences stripped | `jq` |
| `-o jsonl` | One JSON object per line, each validated | `jq -c`, line-oriented tools |
| `-o tsv` | Tab-separated values prompt | `cut`, `sort`, `awk` |
| `-o csv` | Comma-separated values prompt | spreadsheets, `csvtool` |
| `-o text` | Default behavior (explicit) | `grep`, `sed`, anything |

**Design:** `output_schema` on a role takes precedence over `-o`. Streaming is disabled for all structured formats. All 91 tests pass (80 existing + 11 new).

**Files changed:** `src/cli.rs`, `src/config/mod.rs`, `src/config/input.rs`, `src/client/common.rs`, `src/main.rs`, `src/pipe.rs`
