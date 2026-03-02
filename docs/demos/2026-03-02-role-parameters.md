# Role Parameters (-v) and Environment Variable Bridging

*2026-03-02T16:05:40Z by Showboat 0.6.1*
<!-- showboat-id: f624b392-1c78-4573-b8e9-0777bdef6b3e -->

Roles can now declare **variables** in their YAML frontmatter, supplied at invocation via `-v key=value`. Variables with defaults are optional; variables without defaults are required. A separate **environment variable bridging** syntax `{{$ENV_VAR}}` lets any prompt reference shell environment variables directly. Both features compose cleanly with existing system variables (`{{__os__}}`, `{{__model_name__}}`, etc.) and with the composable roles (`extends`, `include`) introduced earlier.

## Feature 1: Role Variables with `-v`

A role declares variables in its frontmatter. Each variable has a `name` and an optional `default`. Variables are referenced in the prompt with `{{name}}` syntax and resolved before any model interpolation.

Here is a role with two variables — `language` (required) and `tone` (defaults to "formal"):

```bash
cat <<'ROLE'
---
variables:
  - name: language
  - name: tone
    default: formal
---
You are a translator. Translate the user's input into {{language}}.
Use a {{tone}} tone. Preserve the original meaning faithfully.
ROLE
```

```output
---
variables:
  - name: language
  - name: tone
    default: formal
---
You are a translator. Translate the user's input into {{language}}.
Use a {{tone}} tone. Preserve the original meaning faithfully.
```

The `-v` flag supplies values at invocation. You can pass multiple variables:

```bash
cat <<'EXAMPLE'
# Supply both variables explicitly:
aichat -r translator -v language=French -v tone=casual "Hello, how are you?"

# Use the default tone (formal):
aichat -r translator -v language=Japanese "Hello, how are you?"
EXAMPLE
```

```output
# Supply both variables explicitly:
aichat -r translator -v language=French -v tone=casual "Hello, how are you?"

# Use the default tone (formal):
aichat -r translator -v language=Japanese "Hello, how are you?"
```

When a required variable is missing, the error message tells you exactly what to provide:

```bash
/Volumes/ExternalData/admin/Developer/Projects/aichat/target/release/aichat -r translator 'Hello' 2>&1 || true
```

```output
Error: Role variable 'language' is required but not provided (use -v language=VALUE)
```

With `--dry-run`, we can see the resolved prompt after variable substitution. Here we supply `language=French` and let `tone` use its default of "formal":

```bash
/Volumes/ExternalData/admin/Developer/Projects/aichat/target/release/aichat -r translator -v language=French --dry-run 'Hello, how are you?' 2>&1
```

```output
You are a translator. Translate the user's input into French.
Use a formal tone. Preserve the original meaning faithfully.

Hello, how are you?
```

Overriding the default — passing `-v tone=casual` replaces the "formal" default:

```bash
/Volumes/ExternalData/admin/Developer/Projects/aichat/target/release/aichat -r translator -v language=Japanese -v tone=casual --dry-run 'Good morning' 2>&1
```

```output
You are a translator. Translate the user's input into Japanese.
Use a casual tone. Preserve the original meaning faithfully.

Good morning
```

## Feature 2: Environment Variable Bridging (`{{$VAR}}`)

Any prompt (role or otherwise) can now reference shell environment variables with `{{$VAR}}` syntax. This is resolved as Phase 3 of interpolation — after conditionals (Phase 1) and model variables (Phase 2) — so it composes cleanly with all existing variable types.

This is useful for injecting runtime context like `$USER`, `$PWD`, project-specific env vars, or secrets that should not be hardcoded into role files.

Here is a role that mixes role variables with env variable references:

```bash
cat <<'ROLE'
---
variables:
  - name: environment
    default: staging
---
You are a deployment reviewer for the {{$PROJECT_NAME}} project.
The current user is {{$USER}}.
Review the deployment plan for the {{environment}} environment.
Flag any risks or missing steps.
ROLE
```

```output
---
variables:
  - name: environment
    default: staging
---
You are a deployment reviewer for the {{$PROJECT_NAME}} project.
The current user is {{$USER}}.
Review the deployment plan for the {{environment}} environment.
Flag any risks or missing steps.
```

`{{$PROJECT_NAME}}` and `{{$USER}}` are resolved from the shell environment at runtime. `{{environment}}` is a role variable resolved from `-v` or its default. With `--dry-run` we can see all three resolve together:

```bash
PROJECT_NAME=acme-api /Volumes/ExternalData/admin/Developer/Projects/aichat/target/release/aichat -r deploy-reviewer -v environment=production --dry-run 'Deploy v2.4.1 to prod' 2>&1
```

```output
You are a deployment reviewer for the acme-api project.
The current user is admin.
Review the deployment plan for the production environment.
Flag any risks or missing steps.

Deploy v2.4.1 to prod
```

When an env variable is not set, the `{{$VAR}}` reference is left intact (no error). This lets you define roles that gracefully degrade:

```bash
(unset PROJECT_NAME; /Volumes/ExternalData/admin/Developer/Projects/aichat/target/release/aichat -r deploy-reviewer --dry-run 'Deploy v2.4.1' 2>&1)
```

```output
You are a deployment reviewer for the {{$PROJECT_NAME}} project.
The current user is admin.
Review the deployment plan for the staging environment.
Flag any risks or missing steps.

Deploy v2.4.1
```

## Composition: Variables Inherited via `extends`

Role variables compose with `extends`. A child role inherits its parent's variables and can override their defaults. This means a base role can define the variable contract, and specialized children just fill in specific defaults.

```bash
cat <<'ROLE'
---
extends: translator
variables:
  - name: language
    default: French
---
When translating to French, prefer metropolitan French over Canadian French.
ROLE
```

```output
---
extends: translator
variables:
  - name: language
    default: French
---
When translating to French, prefer metropolitan French over Canadian French.
```

The child inherits `language` and `tone` from the parent `translator` role, but overrides `language` with a default of "French". Now you can use `french-translator` without passing `-v language=...`:

```bash
/Volumes/ExternalData/admin/Developer/Projects/aichat/target/release/aichat -r french-translator --dry-run 'Good evening, pleased to meet you.' 2>&1
```

```output
You are a translator. Translate the user's input into French.
Use a formal tone. Preserve the original meaning faithfully.

When translating to French, prefer metropolitan French over Canadian French.

Good evening, pleased to meet you.
```

## Unit Tests

All 11 tests for both features pass (5 for role variables, 6 for env bridging):

```bash
cargo test -- --test-threads=1 test_parse_role_variables test_role_variable_with_default test_role_variable_apply test_role_variables_empty test_role_variables_coexist test_env_variable_substitution test_env_variable_unset test_env_variable_mixed test_env_variable_with_model test_env_variable_does_not_match test_env_variable_ordering 2>&1 | grep -E '(test result|test .* ok|running|FAILED)' | sed 's/finished in [0-9.]*s$/finished in 0.00s/'
```

```output
running 11 tests
test config::role::tests::test_parse_role_variables_from_frontmatter ... ok
test config::role::tests::test_role_variable_apply ... ok
test config::role::tests::test_role_variable_with_default ... ok
test config::role::tests::test_role_variables_coexist_with_system_vars ... ok
test config::role::tests::test_role_variables_empty ... ok
test utils::variables::tests::test_env_variable_does_not_match_regular_vars ... ok
test utils::variables::tests::test_env_variable_mixed_with_system_vars ... ok
test utils::variables::tests::test_env_variable_ordering ... ok
test utils::variables::tests::test_env_variable_substitution ... ok
test utils::variables::tests::test_env_variable_unset ... ok
test utils::variables::tests::test_env_variable_with_model_vars ... ok
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 69 filtered out; finished in 0.00s
```

## Summary

| Syntax | Source | Required | Fallback |
|---|---|---|---|
| `{{name}}` | Role frontmatter `variables:` | If no `default:` | Defaults or `-v name=value` |
| `{{$ENV_VAR}}` | Shell environment | No | Left as `{{$VAR}}` if unset |
| `{{__os__}}` etc. | System (built-in) | No | Resolved at load time |

**Resolution order:** Role variables (Phase 0) → Conditionals (Phase 1) → Model variables (Phase 2) → Env variables (Phase 3)

**Files changed:** `src/cli.rs`, `src/config/mod.rs`, `src/config/role.rs`, `src/main.rs`, `src/utils/variables.rs` — 258 lines added, 1 removed.
