## Me
Using the project as context, describe the macro functionality in detail and how to leverage it.

## AI Assistant
Based on the provided code context, specifically `mod.rs` and `cli.rs`, the **Macro** functionality in this project is a feature designed to automate sequences of operations, effectively allowing users to script interactions with the LLM.

It is distinct from the Rust source code macros found in `macros.rs` (which handle internal code generation like `register_client!`).

Here is a detailed breakdown of the user-facing Macro functionality:

### 1. Concept and Purpose
Macros allow you to define a reusable workflow consisting of input variables and a list of sequential steps. Each step acts as a command executed within the application's REPL environment. This is useful for:
*   Standardizing complex prompts.
*   Chaining multiple logic steps (e.g., "summarize this," then "translate the summary").
*   Automating repetitive configuration changes before running a prompt.
*   Allowing dynamic loading of files or data between steps. 


### 2. Structure of a Macro
As defined in `mod.rs`, a macro is stored as a serialized object (likely YAML) containing two main fields:

*   **`variables`**: A list of `MacroVariable` objects. These represent the dynamic inputs the macro accepts when executed.
*   **`steps`**: A `Vec<String>` representing the commands to be executed.

**Logical Representation:**
```yaml
variables:
  - name: "input_text"
steps:
  - "run some command with $input_text"
  - "run another command"
```


### 3. How to Leverage Macros

#### Creation
The `mod.rs` file contains a `new_macro` function. This suggests the workflow for creating a macro involves:
1.  Triggering a creation command (likely a REPL command or CLI flag not fully detailed in the snippet, but the logic exists).
2.  The system determines the file path using `Self::macro_file(name)`.
3.  The system opens the default system editor (`edit_file`) for you to define the macro's content (YAML).

#### Execution
You can execute macros via the Command Line Interface (CLI) as seen in `cli.rs`.

**Command Line Argument:**
```shell script
aichat --macro <MACRO_NAME> "argument text"
```

*   The `cli.rs` logic handles input parsing specially when `--macro` is present. It combines trailing arguments (`self.text`) and standard input (`stdin`) to form the arguments passed to the macro.

**Internal Execution Flow (`macro_execute` in `mod.rs`):**
1.  **Load:** The macro file is read and deserialized.
2.  **Argument Resolution:** The input arguments are split and mapped to the defined `variables`. If resolution fails (e.g., missing arguments), it returns an error with usage instructions.
3.  **Context Isolation:**
    *   The macro runs in a **cloned configuration**.
    *   It inherits the current role, model, temperature, and `use_tools` settings.
    *   Crucially, it **clears** session-specific states (`session`, `rag`, `agent` are set to `None`). This ensures the macro runs in a clean state and doesn't accidentally pollute an active conversation history.
4.  **Step Iteration:**
    *   The system iterates through the `steps`.
    *   It performs interpolation (`Macro::interpolate_command`) to replace variables in the step string with the provided arguments.
    *   It executes the step using `run_repl_command`.

#### Listing Macros

According to `cli.rs`, you can view available macros using:
```shell script
aichat --list-macros
```


### 4. Technical Summary

*   **Storage:** File-based (YAML).
*   **Parsing:** Uses `serde_yaml` for loading.
*   **Interpolation:** variable substitution happens at runtime before the command is sent to the REPL processor.
*   **Concurrency:** The execution is asynchronous (`async fn macro_execute`) and supports abort signals.

### 5. Open Questions

- What visibility do I get between macro steps? 
- Macro steps are limited to repl commands - See [Macro Definition](https://github.com/sigoden/aichat/wiki/Macro-Guide#macro-definition). So I suppose
the question is would it be useful to start extending the language here in any way? Right now I think the answer is no as I rarely use the repl form of commands any way, 
and ultimately it seems like macros are not as good of a fit as shell scripts are for automating processes - especially if we are thinking about integrating 
with data orchestration tools like airflow or dagu. 
- Where I do see macro value in the future is in agent interaction 

### 6. Ideas