# Outline of Code Changes in Last 3 Commits

*2026-02-27T19:44:26Z by Showboat 0.6.1*
<!-- showboat-id: 122d9c5e-456e-4ec5-b294-5cd30e6b3fce -->

This document outlines the code changes for the three most recent commits, showing the diff and file statistics.

```bash
git show --stat 4f19c043f1201204fc1f6a91523b1b7a4627aca4 && git show 4f19c043f1201204fc1f6a91523b1b7a4627aca4
```

```output
commit 4f19c043f1201204fc1f6a91523b1b7a4627aca4
Author: Jordan Kanter <jikanter@gmail.com>
Date:   Sat Jan 31 12:30:50 2026 -0600

    added a react-framework checking ability

 .gitignore                                     | 90 +++++++++++++++++++++++++-
 .idea/.gitignore                               |  8 +++
 assets/generated-documentation/instructions.md | 15 +++++
 assets/generated-documentation/macros.md       | 74 +++++++++++++++++++++
 scripts/dev-install.sh                         | 17 +++++
 scripts/dev-run.sh                             | 13 ++++
 src/client/common.rs                           | 30 +++++++++
 src/main.rs                                    | 32 ++-------
 src/repl/mod.rs                                | 25 ++-----
 src/utils/command.rs                           |  2 +-
 10 files changed, 259 insertions(+), 47 deletions(-)
commit 4f19c043f1201204fc1f6a91523b1b7a4627aca4
Author: Jordan Kanter <jikanter@gmail.com>
Date:   Sat Jan 31 12:30:50 2026 -0600

    added a react-framework checking ability

diff --git a/.gitignore b/.gitignore
index af2d186..99ae334 100644
--- a/.gitignore
+++ b/.gitignore
@@ -1,4 +1,92 @@
 /target
 /tmp
 /.env
-*.log
\ No newline at end of file
+*.log
+
+# Jetbrains .gitignore
+# Covers JetBrains IDEs: IntelliJ, GoLand, RubyMine, PhpStorm, AppCode, PyCharm, CLion, Android Studio, WebStorm and Rider
+# Reference: https://intellij-support.jetbrains.com/hc/en-us/articles/206544839
+
+# User-specific stuff
+.idea/**/workspace.xml
+.idea/**/tasks.xml
+.idea/**/usage.statistics.xml
+.idea/**/dictionaries
+.idea/**/shelf
+
+# AWS User-specific
+.idea/**/aws.xml
+
+# Generated files
+.idea/**/contentModel.xml
+
+# Sensitive or high-churn files
+.idea/**/dataSources/
+.idea/**/dataSources.ids
+.idea/**/dataSources.local.xml
+.idea/**/sqlDataSources.xml
+.idea/**/dynamic.xml
+.idea/**/uiDesigner.xml
+.idea/**/dbnavigator.xml
+
+# Gradle
+.idea/**/gradle.xml
+.idea/**/libraries
+
+# Gradle and Maven with auto-import
+# When using Gradle or Maven with auto-import, you should exclude module files,
+# since they will be recreated, and may cause churn.  Uncomment if using
+# auto-import.
+# .idea/artifacts
+# .idea/compiler.xml
+# .idea/jarRepositories.xml
+# .idea/modules.xml
+# .idea/*.iml
+# .idea/modules
+# *.iml
+# *.ipr
+
+# CMake
+cmake-build-*/
+
+# Mongo Explorer plugin
+.idea/**/mongoSettings.xml
+
+# File-based project format
+*.iws
+
+# IntelliJ
+out/
+
+# mpeltonen/sbt-idea plugin
+.idea_modules/
+
+# JIRA plugin
+atlassian-ide-plugin.xml
+
+# Cursive Clojure plugin
+.idea/replstate.xml
+
+# SonarLint plugin
+.idea/sonarlint/
+.idea/sonarlint.xml # see https://community.sonarsource.com/t/is-the-file-idea-idea-idea-sonarlint-xml-intended-to-be-under-source-control/121119
+
+# Crashlytics plugin (for Android Studio and IntelliJ)
+com_crashlytics_export_strings.xml
+crashlytics.properties
+crashlytics-build.properties
+fabric.properties
+
+# Editor-based HTTP Client
+.idea/httpRequests
+http-client.private.env.json
+
+# Android studio 3.1+ serialized cache file
+.idea/caches/build_file_checksums.ser
+
+# Apifox Helper cache
+.idea/.cache/.Apifox_Helper
+.idea/ApifoxUploaderProjectSetting.xml
+
+# Github Copilot persisted session migrations, see: https://github.com/microsoft/copilot-intellij-feedback/issues/712#issuecomment-3322062215
+.idea/**/copilot.data.migration.*.xml
\ No newline at end of file
diff --git a/.idea/.gitignore b/.idea/.gitignore
new file mode 100644
index 0000000..13566b8
--- /dev/null
+++ b/.idea/.gitignore
@@ -0,0 +1,8 @@
+# Default ignored files
+/shelf/
+/workspace.xml
+# Editor-based HTTP Client requests
+/httpRequests/
+# Datasource local storage ignored files
+/dataSources/
+/dataSources.local.xml
diff --git a/assets/generated-documentation/instructions.md b/assets/generated-documentation/instructions.md
new file mode 100644
index 0000000..a3f14e8
--- /dev/null
+++ b/assets/generated-documentation/instructions.md
@@ -0,0 +1,15 @@
+# Important Instructions
+
+- `%%` 
+
+
+
+### `%%`
+
+#### Functionality
+Refers to Previous Output: When you use %% as an argument, the application substitutes it with the content of the most recent message received from the AI.
+Chaining Context: This allows you to easily use the output of the previous turn as an input context for the next prompt without manually copying and pasting or saving it to a temporary file.
+
+See also: The top level [Readme](/README.md)
+
+Crucially, these can also be used in macros. I am using this for an expert prompt generator
diff --git a/assets/generated-documentation/macros.md b/assets/generated-documentation/macros.md
new file mode 100644
index 0000000..ce73906
--- /dev/null
+++ b/assets/generated-documentation/macros.md
@@ -0,0 +1,74 @@
+## Me
+Using the project as context, describe the macro functionality in detail and how to leverage it.
+
+## AI Assistant
+Based on the provided code context, specifically `mod.rs` and `cli.rs`, the **Macro** functionality in this project is a feature designed to automate sequences of operations, effectively allowing users to script interactions with the LLM.
+
+It is distinct from the Rust source code macros found in `macros.rs` (which handle internal code generation like `register_client!`).
+
+Here is a detailed breakdown of the user-facing Macro functionality:
+
+### 1. Concept and Purpose
+Macros allow you to define a reusable workflow consisting of input variables and a list of sequential steps. Each step acts as a command executed within the application's REPL environment. This is useful for:
+*   Standardizing complex prompts.
+*   Chaining multiple logic steps (e.g., "summarize this," then "translate the summary").
+*   Automating repetitive configuration changes before running a prompt.
+
+### 2. Structure of a Macro
+As defined in `mod.rs`, a macro is stored as a serialized object (likely YAML) containing two main fields:
+
+*   **`variables`**: A list of `MacroVariable` objects. These represent the dynamic inputs the macro accepts when executed.
+*   **`steps`**: A `Vec<String>` representing the commands to be executed.
+
+**Logical Representation:**
+```yaml
+variables:
+  - name: "input_text"
+steps:
+  - "run some command with $input_text"
+  - "run another command"
+```
+
+
+### 3. How to Leverage Macros
+
+#### Creation
+The `mod.rs` file contains a `new_macro` function. This suggests the workflow for creating a macro involves:
+1.  Triggering a creation command (likely a REPL command or CLI flag not fully detailed in the snippet, but the logic exists).
+2.  The system determines the file path using `Self::macro_file(name)`.
+3.  The system opens the default system editor (`edit_file`) for you to define the macro's content (YAML).
+
+#### Execution
+You can execute macros via the Command Line Interface (CLI) as seen in `cli.rs`.
+
+**Command Line Argument:**
+```shell script
+aichat --macro <MACRO_NAME> "argument text"
+```
+
+*   The `cli.rs` logic handles input parsing specially when `--macro` is present. It combines trailing arguments (`self.text`) and standard input (`stdin`) to form the arguments passed to the macro.
+
+**Internal Execution Flow (`macro_execute` in `mod.rs`):**
+1.  **Load:** The macro file is read and deserialized.
+2.  **Argument Resolution:** The input arguments are split and mapped to the defined `variables`. If resolution fails (e.g., missing arguments), it returns an error with usage instructions.
+3.  **Context Isolation:**
+    *   The macro runs in a **cloned configuration**.
+    *   It inherits the current role, model, temperature, and `use_tools` settings.
+    *   Crucially, it **clears** session-specific states (`session`, `rag`, `agent` are set to `None`). This ensures the macro runs in a clean state and doesn't accidentally pollute an active conversation history.
+4.  **Step Iteration:**
+    *   The system iterates through the `steps`.
+    *   It performs interpolation (`Macro::interpolate_command`) to replace variables in the step string with the provided arguments.
+    *   It executes the step using `run_repl_command`.
+
+#### Listing Macros
+According to `cli.rs`, you can view available macros using:
+```shell script
+aichat --list-macros
+```
+
+
+### 4. Technical Summary
+*   **Storage:** File-based (YAML).
+*   **Parsing:** Uses `serde_yaml` for loading.
+*   **Interpolation:** variable substitution happens at runtime before the command is sent to the REPL processor.
+*   **Concurrency:** The execution is asynchronous (`async fn macro_execute`) and supports abort signals.
diff --git a/scripts/dev-install.sh b/scripts/dev-install.sh
new file mode 100755
index 0000000..f687d93
--- /dev/null
+++ b/scripts/dev-install.sh
@@ -0,0 +1,17 @@
+#!/usr/bin/env bash
+if [[ -x "${HOME}/.cargo/bin/aichat" ]]; then
+  printf "[DEV INSTALLER]: %s\n" "found existing aichat install. moving it aside before compiling"
+  mv "${HOME}/.cargo/bin/aichat" "${HOME}/.cargo/bin/aichat_tmp"
+  cargo install --path .
+  if [[ ! -x "${HOME}/.cargo/bin/aichat_tmp" ]] || [[ ! -x "${HOME}/.cargo/bin/aichat" ]]; then
+    printf "[DEV INSTALLER]: %s\n" "could not find the new or temporary production aichat app....exiting"
+    exit 3
+  else
+    # move the dev install to aichat-dev
+    mv "${HOME}/.cargo/bin/aichat" "${HOME}/.cargo/bin/aichat-dev"
+    # move the tmp production build back
+    mv "${HOME}/.cargo/bin/aichat_tmp" "${HOME}/.cargo/bin/aichat"
+    printf "[DEV INSTALLER]: %s\n" "aichat-dev installed"
+  fi
+fi
+
diff --git a/scripts/dev-run.sh b/scripts/dev-run.sh
new file mode 100644
index 0000000..e4d24a9
--- /dev/null
+++ b/scripts/dev-run.sh
@@ -0,0 +1,13 @@
+#!/usr/bin/env bash
+#AICHAT_CONFIG_DIR: Overrides the entire configuration directory.
+#AICHAT_CONFIG_FILE: Overrides only the config.yaml path.
+#AICHAT_ROLES_DIR: Overrides where roles are stored.
+#AICHAT_SESSIONS_DIR: Overrides where session history is stored
+if [[ ! -x "${HOME}/.cargo/bin/aichat-dev" ]]; then
+    printf "[DEV RUN]: %s\n" "aichat-dev binary not found, run dev-install.sh first"
+    exit 3
+fi
+
+export AICHAT_CONFIG_DIR="/Users/admin/Library/Application Support/aichat-dev"
+printf "[DEV RUN]: %s\n" "running aichat-dev with configuration directory ${AICHAT_CONFIG_DIR} "
+aichat-dev
diff --git a/src/client/common.rs b/src/client/common.rs
index 3b89d11..8892b72 100644
--- a/src/client/common.rs
+++ b/src/client/common.rs
@@ -403,6 +403,36 @@ pub async fn create_openai_compatible_client_config(
     Ok(Some((model, clients)))
 }
 
+const MAX_REACT_STEPS: usize = 10;
+
+pub async fn call_react(
+    input: &mut Input,
+    client: &dyn Client,
+    abort_signal: AbortSignal,
+) -> Result<(String, Vec<ToolResult>)> {
+    let mut total_text = String::new();
+    let mut step = 0;
+    loop {
+        let (text, tool_results) = if input.stream() {
+            call_chat_completions_streaming(input, client, abort_signal.clone()).await?
+        } else {
+            call_chat_completions(input, true, false, client, abort_signal.clone()).await?
+        };
+        if !total_text.is_empty() {
+            total_text.push('\n');
+        }
+        total_text.push_str(&text);
+        if tool_results.is_empty() {
+            return Ok((total_text, vec![]));
+        }
+        step += 1;
+        if step >= MAX_REACT_STEPS {
+            bail!("ReAct loop exceeded maximum steps ({MAX_REACT_STEPS})");
+        }
+        *input = input.clone().merge_tool_results(text, tool_results);
+    }
+}
+
 pub async fn call_chat_completions(
     input: &Input,
     print: bool,
diff --git a/src/main.rs b/src/main.rs
index 369a282..a57131f 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -14,7 +14,7 @@ extern crate log;
 
 use crate::cli::Cli;
 use crate::client::{
-    call_chat_completions, call_chat_completions_streaming, list_models, ModelType,
+    call_chat_completions, call_chat_completions_streaming, call_react, list_models, ModelType,
 };
 use crate::config::{
     ensure_parent_exists, list_agents, load_env_file, macro_execute, Config, GlobalConfig, Input,
@@ -195,39 +195,19 @@ async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()>
 #[async_recursion::async_recursion]
 async fn start_directive(
     config: &GlobalConfig,
-    input: Input,
-    code_mode: bool,
+    mut input: Input,
+    _code_mode: bool,
     abort_signal: AbortSignal,
 ) -> Result<()> {
     let client = input.create_client()?;
-    let extract_code = !*IS_STDOUT_TERMINAL && code_mode;
     config.write().before_chat_completion(&input)?;
-    let (output, tool_results) = if !input.stream() || extract_code {
-        call_chat_completions(
-            &input,
-            true,
-            extract_code,
-            client.as_ref(),
-            abort_signal.clone(),
-        )
-        .await?
-    } else {
-        call_chat_completions_streaming(&input, client.as_ref(), abort_signal.clone()).await?
-    };
+    let (output, tool_results) =
+        call_react(&mut input, client.as_ref(), abort_signal.clone()).await?;
+
     config
         .write()
         .after_chat_completion(&input, &output, &tool_results)?;
 
-    if !tool_results.is_empty() {
-        start_directive(
-            config,
-            input.merge_tool_results(output, tool_results),
-            code_mode,
-            abort_signal,
-        )
-        .await?;
-    }
-
     config.write().exit_session()?;
     Ok(())
 }
diff --git a/src/repl/mod.rs b/src/repl/mod.rs
index f8a51d1..c65b6b0 100644
--- a/src/repl/mod.rs
+++ b/src/repl/mod.rs
@@ -6,7 +6,7 @@ use self::completer::ReplCompleter;
 use self::highlighter::ReplHighlighter;
 use self::prompt::ReplPrompt;
 
-use crate::client::{call_chat_completions, call_chat_completions_streaming};
+use crate::client::call_react;
 use crate::config::{
     macro_execute, AgentVariables, AssertState, Config, GlobalConfig, Input, LastMessage,
     StateFlags,
@@ -735,27 +735,14 @@ async fn ask(
 
     let client = input.create_client()?;
     config.write().before_chat_completion(&input)?;
-    let (output, tool_results) = if input.stream() {
-        call_chat_completions_streaming(&input, client.as_ref(), abort_signal.clone()).await?
-    } else {
-        call_chat_completions(&input, true, false, client.as_ref(), abort_signal.clone()).await?
-    };
+    let (output, tool_results) =
+        call_react(&mut input, client.as_ref(), abort_signal.clone()).await?;
     config
         .write()
         .after_chat_completion(&input, &output, &tool_results)?;
-    if !tool_results.is_empty() {
-        ask(
-            config,
-            abort_signal,
-            input.merge_tool_results(output, tool_results),
-            false,
-        )
-        .await
-    } else {
-        Config::maybe_autoname_session(config.clone());
-        Config::maybe_compress_session(config.clone());
-        Ok(())
-    }
+    Config::maybe_autoname_session(config.clone());
+    Config::maybe_compress_session(config.clone());
+    Ok(())
 }
 
 fn unknown_command() -> Result<()> {
diff --git a/src/utils/command.rs b/src/utils/command.rs
index 7d98504..1829f0b 100644
--- a/src/utils/command.rs
+++ b/src/utils/command.rs
@@ -71,7 +71,7 @@ pub fn detect_shell() -> Shell {
         }
     };
     let shell_arg = match name {
-        "powershel" => "-Command",
+        "powershell" => "-Command",
         "cmd" => "/C",
         _ => "-c",
     };
```

```bash
git show --stat 82976d349ad97ac9aae0655ad631dace5e2a6385 && git show 82976d349ad97ac9aae0655ad631dace5e2a6385
```

```output
commit 82976d349ad97ac9aae0655ad631dace5e2a6385
Author: sigoden <sigoden@gmail.com>
Date:   Mon Feb 23 19:16:27 2026 +0800

    chore: update models.yaml

 models.yaml | 415 +++++++++++++++++++++++++++++++++++++++++++++++-------------
 1 file changed, 326 insertions(+), 89 deletions(-)
commit 82976d349ad97ac9aae0655ad631dace5e2a6385
Author: sigoden <sigoden@gmail.com>
Date:   Mon Feb 23 19:16:27 2026 +0800

    chore: update models.yaml

diff --git a/models.yaml b/models.yaml
index e779ff8..bf9236f 100644
--- a/models.yaml
+++ b/models.yaml
@@ -137,21 +137,21 @@
 #  - https://docs.anthropic.com/en/api/messages
 - provider: claude
   models:
-    - name: claude-sonnet-4-5-20250929
+    - name: claude-opus-4-6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: claude-sonnet-4-5-20250929:thinking
-      real_name: claude-sonnet-4-5-20250929
+    - name: claude-opus-4-6:thinking
+      real_name: claude-opus-4-6
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
       patch:
@@ -161,21 +161,21 @@
           thinking:
             type: enabled
             budget_tokens: 16000
-    - name: claude-haiku-4-5-20251001
+    - name: claude-sonnet-4-6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       supports_function_calling: true
-    - name: claude-haiku-4-5-20251001:thinking
-      real_name: claude-haiku-4-5-20251001
+    - name: claude-sonnet-4-6:thinking
+      real_name: claude-sonnet-4-6
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       supports_function_calling: true
       patch:
@@ -209,6 +209,54 @@
           thinking:
             type: enabled
             budget_tokens: 16000
+    - name: claude-sonnet-4-5-20250929
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      supports_function_calling: true
+    - name: claude-sonnet-4-5-20250929:thinking
+      real_name: claude-sonnet-4-5-20250929
+      max_input_tokens: 200000
+      max_output_tokens: 24000
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      supports_function_calling: true
+      patch:
+        body:
+          temperature: null
+          top_p: null
+          thinking:
+            type: enabled
+            budget_tokens: 16000
+    - name: claude-haiku-4-5-20251001
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      supports_function_calling: true
+    - name: claude-haiku-4-5-20251001:thinking
+      real_name: claude-haiku-4-5-20251001
+      max_input_tokens: 200000
+      max_output_tokens: 24000
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      supports_function_calling: true
+      patch:
+        body:
+          temperature: null
+          top_p: null
+          thinking:
+            type: enabled
+            budget_tokens: 16000
 
 # Links:
 #  - https://docs.mistral.ai/getting-started/models/models_overview/
@@ -484,21 +532,21 @@
       output_price: 0.3
       supports_vision: true
       supports_function_calling: true
-    - name: claude-sonnet-4-5@20250929
+    - name: claude-opus-4-6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: claude-sonnet-4-5@20250929:thinking
-      real_name: claude-sonnet-4-5@20250929
+    - name: claude-opus-4-6:thinking
+      real_name: claude-opus-4-6
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       patch:
         body:
@@ -507,21 +555,21 @@
           thinking:
             type: enabled
             budget_tokens: 16000
-    - name: claude-haiku-4-5@20251001
+    - name: claude-sonnet-4-6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       supports_function_calling: true
-    - name: claude-haiku-4-5@20251001:thinking
-      real_name: claude-haiku-4-5@20251001
+    - name: claude-sonnet-4-6:thinking
+      real_name: claude-sonnet-4-6
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       patch:
         body:
@@ -553,6 +601,52 @@
           thinking:
             type: enabled
             budget_tokens: 16000
+    - name: claude-sonnet-4-5@20250929
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      supports_function_calling: true
+    - name: claude-sonnet-4-5@20250929:thinking
+      real_name: claude-sonnet-4-5@20250929
+      max_input_tokens: 200000
+      max_output_tokens: 24000
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      patch:
+        body:
+          temperature: null
+          top_p: null
+          thinking:
+            type: enabled
+            budget_tokens: 16000
+    - name: claude-haiku-4-5@20251001
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      supports_function_calling: true
+    - name: claude-haiku-4-5@20251001:thinking
+      real_name: claude-haiku-4-5@20251001
+      max_input_tokens: 200000
+      max_output_tokens: 24000
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      patch:
+        body:
+          temperature: null
+          top_p: null
+          thinking:
+            type: enabled
+            budget_tokens: 16000
     - name: text-embedding-005
       type: embedding
       max_input_tokens: 20000
@@ -575,21 +669,21 @@
 #  - https://docs.aws.amazon.com/bedrock/latest/userguide/conversation-inference-call.html
 - provider: bedrock
   models:
-    - name: us.anthropic.claude-sonnet-4-5-20250929-v1:0
+    - name: us.anthropic.claude-opus-4-6-v1
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: us.anthropic.claude-sonnet-4-5-20250929-v1:0:thinking
-      real_name: us.anthropic.claude-sonnet-4-5-20250929-v1:0
+    - name: us.anthropic.claude-opus-4-6-v1:thinking
+      real_name: us.anthropic.claude-opus-4-6-v1
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       patch:
         body:
@@ -600,21 +694,21 @@
             thinking:
               type: enabled
               budget_tokens: 16000
-    - name: us.anthropic.claude-haiku-4-5-20251001-v1:0
+    - name: us.anthropic.claude-sonnet-4-6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       supports_function_calling: true
-    - name: us.anthropic.claude-haiku-4-5-20251001-v1:0:thinking
-      real_name: us.anthropic.claude-haiku-4-5-20251001-v1:0
+    - name: us.anthropic.claude-sonnet-4-6:thinking
+      real_name: us.anthropic.claude-sonnet-4-6
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       patch:
         body:
@@ -650,6 +744,56 @@
             thinking:
               type: enabled
               budget_tokens: 16000
+    - name: us.anthropic.claude-sonnet-4-5-20250929-v1:0
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      supports_function_calling: true
+    - name: us.anthropic.claude-sonnet-4-5-20250929-v1:0:thinking
+      real_name: us.anthropic.claude-sonnet-4-5-20250929-v1:0
+      max_input_tokens: 200000
+      max_output_tokens: 24000
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      patch:
+        body:
+          inferenceConfig:
+            temperature: null
+            topP: null
+          additionalModelRequestFields:
+            thinking:
+              type: enabled
+              budget_tokens: 16000
+    - name: us.anthropic.claude-haiku-4-5-20251001-v1:0
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      supports_function_calling: true
+    - name: us.anthropic.claude-haiku-4-5-20251001-v1:0:thinking
+      real_name: us.anthropic.claude-haiku-4-5-20251001-v1:0
+      max_input_tokens: 200000
+      max_output_tokens: 24000
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      patch:
+        body:
+          inferenceConfig:
+            temperature: null
+            topP: null
+          additionalModelRequestFields:
+            thinking:
+              type: enabled
+              budget_tokens: 16000
     - name: us.meta.llama4-maverick-17b-instruct-v1:0
       max_input_tokens: 131072
       max_output_tokens: 8192
@@ -741,6 +885,12 @@
       require_max_tokens: true
       input_price: 0
       output_price: 0
+    - name: '@cf/zai-org/glm-4.7-flash'
+      max_input_tokens: 131072
+      max_output_tokens: 2048
+      require_max_tokens: true
+      input_price: 0
+      output_price: 0
     - name: '@cf/google/gemma-3-12b-it'
       max_input_tokens: 131072
       max_output_tokens: 2048
@@ -805,25 +955,46 @@
 #  - https://help.aliyun.com/zh/model-studio/developer-reference/use-qwen-by-calling-api
 - provider: qianwen
   models:
-    - name: qwen3-max
+    - name: qwen3.5-plus
       max_input_tokens: 262144
       supports_function_calling: true
-    - name: qwen-plus
-      max_input_tokens: 131072
+      patch:
+        body:
+          enable_thinking: false
+    - name: qwen3.5-plus:thinking
+      real_name: qwen3.5-plus
+      max_input_tokens: 262144
       supports_function_calling: true
-    - name: qwen-flash
-      max_input_tokens: 1000000
+    - name: qwen3-max
+      max_input_tokens: 262144
+      supports_function_calling: true
+    - name: qwen3-max:thinking
+      real_name: qwen3-max
+      max_input_tokens: 262144
       supports_function_calling: true
+      patch:
+        body:
+          enable_thinking: true
     - name: qwen3-vl-plus
       max_input_tokens: 262144
       supports_vision: true
     - name: qwen3-vl-flash
       max_input_tokens: 262144
       supports_vision: true
-    - name: qwen-coder-plus
+    - name: qwen3-coder-plus
       max_input_tokens: 1000000
-    - name: qwen-coder-flash
+    - name: qwen3-coder-flash
       max_input_tokens: 1000000
+    - name: qwen3.5-397b-a17b
+      max_input_tokens: 262144
+      supports_function_calling: true
+      patch:
+        body:
+          enable_thinking: false
+    - name: qwen3.5-397b-a17b:thinking
+      real_name: qwen3.5-397b-a17b
+      max_input_tokens: 262144
+      supports_function_calling: true
     - name: qwen3-next-80b-a3b-instruct
       max_input_tokens: 131072
       input_price: 0.14
@@ -861,18 +1032,12 @@
       input_price: 0.07
       output_price: 0.28
       supports_vision: true
+    - name: qwen3-coder-next
+      max_input_tokens: 262144
     - name: qwen3-coder-480b-a35b-instruct
       max_input_tokens: 262144
-      input_price: 1.26
-      output_price: 5.04
     - name: qwen3-coder-30b-a3b-instruct
       max_input_tokens: 262144
-      input_price: 0.315
-      output_price: 1.26
-    - name: deepseek-v3.2-exp
-      max_input_tokens: 131072
-      input_price: 0.28
-      output_price: 0.42
     - name: text-embedding-v4
       type: embedding
       input_price: 0.1
@@ -970,16 +1135,23 @@
 #  - https://open.bigmodel.cn/dev/api#glm-4
 - provider: zhipuai
   models:
+    - name: glm-5
+      max_input_tokens: 202752
+      supports_function_calling: true
+    - name: glm-5:instruct
+      real_name: glm-5
+      max_input_tokens: 202752
+      supports_function_calling: true
+      patch:
+        body:
+          thinking:
+            type: disabled
     - name: glm-4.7
       max_input_tokens: 202752
-      input_price: 0.56
-      output_price: 2.24
       supports_function_calling: true
     - name: glm-4.7:instruct
       real_name: glm-4.7
       max_input_tokens: 202752
-      input_price: 0.56
-      output_price: 2.24
       supports_function_calling: true
       patch:
         body:
@@ -992,8 +1164,6 @@
       supports_function_calling: true
     - name: glm-4.6v
       max_input_tokens: 65536
-      input_price: 0.28
-      output_price: 0.84
       supports_vision: true
     - name: glm-4.6v-flash
       max_input_tokens: 65536
@@ -1012,18 +1182,28 @@
       input_price: 0.112
 
 # Links:
-# - https://platform.minimaxi.com/docs/guides/pricing-payg
+# - https://platform.minimaxi.com/docs/guides/pricing-paygo
 # - https://platform.minimaxi.com/document/ChatCompletion%20v2
 - provider: minimax
   models:
-    - name: minimax-m2.1
+    - name: minimax-m2.5
       max_input_tokens: 204800
       input_price: 0.294
       output_price: 1.176
       supports_function_calling: true
-    - name: minimax-m2.1-lightning
+    - name: minimax-m2.5-highspeed
+      max_input_tokens: 204800
+      input_price: 0.588
+      output_price: 2.352
+      supports_function_calling: true
+    - name: minimax-m2.1
       max_input_tokens: 204800
       input_price: 0.294
+      output_price: 1.176
+      supports_function_calling: true
+    - name: minimax-m2.1-highspeed
+      max_input_tokens: 204800
+      input_price: 0.588
       output_price: 2.352
       supports_function_calling: true
 
@@ -1116,20 +1296,20 @@
       max_input_tokens: 131072
       input_price: 0.1
       output_price: 0.2
-    - name: anthropic/claude-sonnet-4.5
+    - name: anthropic/claude-opus-4.6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 3
-      output_price: 15
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: anthropic/claude-haiku-4.5
+    - name: anthropic/claude-sonnet-4.6
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 1
-      output_price: 5
+      input_price: 3
+      output_price: 15
       supports_vision: true
       supports_function_calling: true
     - name: anthropic/claude-opus-4.5
@@ -1140,6 +1320,22 @@
       output_price: 25
       supports_vision: true
       supports_function_calling: true
+    - name: anthropic/claude-sonnet-4.5
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 3
+      output_price: 15
+      supports_vision: true
+      supports_function_calling: true
+    - name: anthropic/claude-haiku-4.5
+      max_input_tokens: 200000
+      max_output_tokens: 8192
+      require_max_tokens: true
+      input_price: 1
+      output_price: 5
+      supports_vision: true
+      supports_function_calling: true
     - name: meta-llama/llama-4-maverick
       max_input_tokens: 1048576
       input_price: 0.18
@@ -1197,11 +1393,6 @@
       input_price: 2
       output_price: 8
       supports_function_calling: true
-    - name: ai21/jamba-mini-1.7
-      max_input_tokens: 256000
-      input_price: 0.2
-      output_price: 0.4
-      supports_function_calling: true
     - name: cohere/command-a
       max_input_tokens: 256000
       input_price: 2.5
@@ -1221,11 +1412,22 @@
       input_price: 1.2
       output_price: 6
       supports_function_calling: true
-    - name: qwen/qwen-plus
-      max_input_tokens: 131072
+    - name: qwen/qwen3-max-thinking
+      max_input_tokens: 262144
+      input_price: 1.2
+      output_price: 6
+      supports_function_calling: true
+    - name: qwen/qwen3.5-plus-02-15
+      max_input_tokens: 1000000
       max_output_tokens: 8192
       input_price: 0.4
-      output_price: 1.2
+      output_price: 2.4
+      supports_function_calling: true
+    - name: qwen/qwen3.5-397b-a17b
+      max_input_tokens: 262144
+      max_output_tokens: 8192
+      input_price: 0.15
+      output_price: 1
       supports_function_calling: true
     - name: qwen/qwen3-next-80b-a3b-instruct
       max_input_tokens: 262144
@@ -1236,7 +1438,7 @@
       max_input_tokens: 262144
       input_price: 0.1
       output_price: 0.8
-    - name: qwen/qwen5-235b-a22b-2507 # Qwen3 235B A22B Instruct 2507
+    - name: qwen/qwen3-235b-a22b-2507 # Qwen3 235B A22B Instruct 2507
       max_input_tokens: 262144
       input_price: 0.12
       output_price: 0.59
@@ -1263,6 +1465,11 @@
       input_price: 0.08
       output_price: 0.50
       supports_vision: true
+    - name: qwen/qwen3-coder-next
+      max_input_tokens: 262144
+      input_price: 0.12
+      output_price: 0.75
+      supports_function_calling: true
     - name: qwen/qwen3-coder-plus
       max_input_tokens: 128000
       input_price: 1
@@ -1300,11 +1507,6 @@
       input_price: 0.45
       output_price: 2.35
       supports_function_calling: true
-    - name: moonshotai/kimi-dev-72b
-      max_input_tokens: 131072
-      input_price: 0.29
-      output_price: 1.15
-      supports_function_calling: true
     - name: x-ai/grok-4.1-fast
       max_input_tokens: 2000000
       input_price: 0.2
@@ -1359,11 +1561,21 @@
       patch:
         body:
           include_reasoning: true
+    - name: minimax/minimax-m2.5
+      max_input_tokens: 196608
+      input_price: 0.3
+      output_price: 1.1
+      supports_function_calling: true
     - name: minimax/minimax-m2.1
       max_input_tokens: 196608
       input_price: 0.12
       output_price: 0.48
       supports_function_calling: true
+    - name: z-ai/glm-5
+      max_input_tokens: 204800
+      input_price: 0.95
+      output_price: 2.55
+      supports_function_calling: true
     - name: z-ai/glm-4.7
       max_input_tokens: 202752
       input_price: 0.16
@@ -1489,6 +1701,16 @@
       input_price: 0.08
       output_price: 0.3
       supports_vision: true
+    - name: Qwen/Qwen3-Max
+      max_input_tokens: 262144
+      input_price: 1.2
+      output_price: 6
+      supports_function_calling: true
+    - name: Qwen/Qwen3-Max-Thinking
+      max_input_tokens: 262144
+      input_price: 1.2
+      output_price: 6
+      supports_function_calling: true
     - name: Qwen/Qwen3-Next-80B-A3B-Instruct
       max_input_tokens: 262144
       input_price: 0.14
@@ -1554,16 +1776,31 @@
       input_price: 0.55
       output_price: 2.5
       supports_function_calling: true
+    - name: MiniMaxAI/MiniMax-M2.5
+      max_input_tokens: 196608
+      input_price: 0.27
+      output_price: 0.95
+      supports_function_calling: true
     - name: MiniMaxAI/MiniMax-M2.1
-      max_input_tokens: 262144
-      input_price: 0.28
-      output_price: 1.2
+      max_input_tokens: 196608
+      input_price: 0.27
+      output_price: 0.95
+      supports_function_calling: true
+    - name: zai-org/GLM-5
+      max_input_tokens: 202752
+      input_price: 0.8
+      output_price: 2.56
       supports_function_calling: true
     - name: zai-org/GLM-4.7
       max_input_tokens: 202752
       input_price: 0.43
       output_price: 1.75
       supports_function_calling: true
+    - name: zai-org/GLM-4.7-Flash
+      max_input_tokens: 202752
+      input_price: 0.06
+      output_price: 0.4
+      supports_function_calling: true
     - name: zai-org/GLM-4.6V
       max_input_tokens: 131072
       input_price: 0.3
```

```bash
git show --stat 0493134bd3e13c6e8507da5409d4b0a82152d6dc && git show 0493134bd3e13c6e8507da5409d4b0a82152d6dc
```

```output
commit 0493134bd3e13c6e8507da5409d4b0a82152d6dc
Author: sigoden <sigoden@gmail.com>
Date:   Thu Jan 29 11:14:04 2026 +0800

    chore: update models.yaml

 models.yaml | 355 ++++++++++--------------------------------------------------
 1 file changed, 58 insertions(+), 297 deletions(-)
commit 0493134bd3e13c6e8507da5409d4b0a82152d6dc
Author: sigoden <sigoden@gmail.com>
Date:   Thu Jan 29 11:14:04 2026 +0800

    chore: update models.yaml

diff --git a/models.yaml b/models.yaml
index 2c70877..e779ff8 100644
--- a/models.yaml
+++ b/models.yaml
@@ -185,21 +185,21 @@
           thinking:
             type: enabled
             budget_tokens: 16000
-    - name: claude-opus-4-1-20250805
+    - name: claude-opus-4-5-20251101
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 15
-      output_price: 75
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: claude-opus-4-1-20250805:thinking
-      real_name: claude-opus-4-1-20250805
+    - name: claude-opus-4-5-20251101:thinking
+      real_name: claude-opus-4-5-20251101
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 15
-      output_price: 75
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
       patch:
@@ -209,85 +209,6 @@
           thinking:
             type: enabled
             budget_tokens: 16000
-    - name: claude-opus-4-20250514
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      supports_function_calling: true
-    - name: claude-opus-4-20250514:thinking
-      real_name: claude-opus-4-20250514
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      supports_function_calling: true
-      patch:
-        body:
-          temperature: null
-          top_p: null
-          thinking:
-            type: enabled
-            budget_tokens: 16000
-    - name: claude-sonnet-4-20250514
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: claude-sonnet-4-20250514:thinking
-      real_name: claude-sonnet-4-20250514
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-      patch:
-        body:
-          temperature: null
-          top_p: null
-          thinking:
-            type: enabled
-            budget_tokens: 16000
-    - name: claude-3-7-sonnet-20250219
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: claude-3-7-sonnet-20250219:thinking
-      real_name: claude-3-7-sonnet-20250219
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      patch:
-        body:
-          temperature: null
-          top_p: null
-          thinking:
-            type: enabled
-            budget_tokens: 16000
-    - name: claude-3-5-haiku-20241022
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 0.8
-      output_price: 4
-      supports_vision: true
-      supports_function_calling: true
 
 # Links:
 #  - https://docs.mistral.ai/getting-started/models/models_overview/
@@ -609,21 +530,21 @@
           thinking:
             type: enabled
             budget_tokens: 16000
-    - name: claude-opus-4-1@20250805
+    - name: claude-opus-4-5@20251101
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 15
-      output_price: 75
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: claude-opus-4-1@20250805:thinking
-      real_name: claude-opus-4-1@20250805
+    - name: claude-opus-4-5@20251101:thinking
+      real_name: claude-opus-4-5@20251101
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 15
-      output_price: 75
+      input_price: 5
+      output_price: 25
       supports_vision: true
       patch:
         body:
@@ -632,83 +553,6 @@
           thinking:
             type: enabled
             budget_tokens: 16000
-    - name: claude-opus-4@20250514
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      supports_function_calling: true
-    - name: claude-opus-4@20250514:thinking
-      real_name: claude-opus-4@20250514
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      patch:
-        body:
-          temperature: null
-          top_p: null
-          thinking:
-            type: enabled
-            budget_tokens: 16000
-    - name: claude-sonnet-4@20250514
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: claude-sonnet-4@20250514:thinking
-      real_name: claude-sonnet-4@20250514
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      patch:
-        body:
-          temperature: null
-          top_p: null
-          thinking:
-            type: enabled
-            budget_tokens: 16000
-    - name: claude-3-7-sonnet@20250219
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: claude-3-7-sonnet@20250219:thinking
-      real_name: claude-3-7-sonnet@20250219
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      patch:
-        body:
-          temperature: null
-          top_p: null
-          thinking:
-            type: enabled
-            budget_tokens: 16000
-    - name: claude-3-5-haiku@20241022
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 0.8
-      output_price: 4
-      supports_vision: true
-      supports_function_calling: true
     - name: text-embedding-005
       type: embedding
       max_input_tokens: 20000
@@ -781,21 +625,21 @@
             thinking:
               type: enabled
               budget_tokens: 16000
-    - name: us.anthropic.claude-opus-4-1-20250805-v1:0
+    - name: us.anthropic.claude-opus-4-5-20251101-v1:0
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 15
-      output_price: 75
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
-    - name: us.anthropic.claude-opus-4-1-20250805-v1:0:thinking
-      real_name: us.anthropic.claude-opus-4-1-20250805-v1:0
+    - name: us.anthropic.claude-opus-4-5-20251101-v1:0:thinking
+      real_name: us.anthropic.claude-opus-4-5-20251101-v1:0
       max_input_tokens: 200000
       max_output_tokens: 24000
       require_max_tokens: true
-      input_price: 15
-      output_price: 75
+      input_price: 5
+      output_price: 25
       supports_vision: true
       patch:
         body:
@@ -806,89 +650,6 @@
             thinking:
               type: enabled
               budget_tokens: 16000
-    - name: us.anthropic.claude-opus-4-20250514-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      supports_function_calling: true
-    - name: us.anthropic.claude-opus-4-20250514-v1:0:thinking
-      real_name: us.anthropic.claude-opus-4-20250514-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      patch:
-        body:
-          inferenceConfig:
-            temperature: null
-            topP: null
-          additionalModelRequestFields:
-            thinking:
-              type: enabled
-              budget_tokens: 16000
-    - name: us.anthropic.claude-sonnet-4-20250514-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: us.anthropic.claude-sonnet-4-20250514-v1:0:thinking
-      real_name: us.anthropic.claude-sonnet-4-20250514-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      patch:
-        body:
-          inferenceConfig:
-            temperature: null
-            topP: null
-          additionalModelRequestFields:
-            thinking:
-              type: enabled
-              budget_tokens: 16000
-    - name: us.anthropic.claude-3-7-sonnet-20250219-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: us.anthropic.claude-3-7-sonnet-20250219-v1:0:thinking
-      real_name: us.anthropic.claude-3-7-sonnet-20250219-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 24000
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      patch:
-        body:
-          inferenceConfig:
-            temperature: null
-            topP: null
-          additionalModelRequestFields:
-            thinking:
-              type: enabled
-              budget_tokens: 16000
-    - name: anthropic.claude-3-5-haiku-20241022-v1:0
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 0.8
-      output_price: 4
-      supports_vision: true
-      supports_function_calling: true
     - name: us.meta.llama4-maverick-17b-instruct-v1:0
       max_input_tokens: 131072
       max_output_tokens: 8192
@@ -1158,6 +919,12 @@
 #  - https://platform.moonshot.cn/docs/api/chat#%E5%85%AC%E5%BC%80%E7%9A%84%E6%9C%8D%E5%8A%A1%E5%9C%B0%E5%9D%80
 - provider: moonshot
   models:
+    - name: kimi-k2.5
+      max_input_tokens: 262144
+      input_price: 0.56
+      output_price: 2.94
+      supports_vision: true
+      supports_function_calling: true
     - name: kimi-k2-turbo-preview
       max_input_tokens: 262144
       input_price: 1.12
@@ -1208,7 +975,6 @@
       input_price: 0.56
       output_price: 2.24
       supports_function_calling: true
-
     - name: glm-4.7:instruct
       real_name: glm-4.7
       max_input_tokens: 202752
@@ -1219,11 +985,21 @@
         body:
           thinking:
             type: disabled
+    - name: glm-4.7-flash
+      max_input_tokens: 202752
+      input_price: 0
+      output_price: 0
+      supports_function_calling: true
     - name: glm-4.6v
       max_input_tokens: 65536
       input_price: 0.28
       output_price: 0.84
       supports_vision: true
+    - name: glm-4.6v-flash
+      max_input_tokens: 65536
+      input_price: 0
+      output_price: 0
+      supports_vision: true
     - name: embedding-3
       type: embedding
       max_input_tokens: 8192
@@ -1236,7 +1012,7 @@
       input_price: 0.112
 
 # Links:
-# - https://platform.minimaxi.com/docs/guides/pricing
+# - https://platform.minimaxi.com/docs/guides/pricing-payg
 # - https://platform.minimaxi.com/document/ChatCompletion%20v2
 - provider: minimax
   models:
@@ -1356,44 +1132,12 @@
       output_price: 5
       supports_vision: true
       supports_function_calling: true
-    - name: anthropic/claude-opus-4.1
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      supports_function_calling: true
-    - name: anthropic/claude-opus-4
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 15
-      output_price: 75
-      supports_vision: true
-      supports_function_calling: true
-    - name: anthropic/claude-sonnet-4
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: anthropic/claude-3.7-sonnet
-      max_input_tokens: 200000
-      max_output_tokens: 8192
-      require_max_tokens: true
-      input_price: 3
-      output_price: 15
-      supports_vision: true
-      supports_function_calling: true
-    - name: anthropic/claude-3.5-haiku
+    - name: anthropic/claude-opus-4.5
       max_input_tokens: 200000
       max_output_tokens: 8192
       require_max_tokens: true
-      input_price: 0.8
-      output_price: 4
+      input_price: 5
+      output_price: 25
       supports_vision: true
       supports_function_calling: true
     - name: meta-llama/llama-4-maverick
@@ -1539,10 +1283,17 @@
       input_price: 0.052
       output_price: 0.207
       supports_function_calling: true
+    - name: moonshotai/kimi-k2.5
+      max_input_tokens: 262144
+      input_price: 0.57
+      output_price: 2.85
+      supports_vision: true
+      supports_function_calling: true
     - name: moonshotai/kimi-k2-0905
       max_input_tokens: 262144
       input_price: 0.296
       output_price: 1.185
+      supports_vision: true
       supports_function_calling: true
     - name: moonshotai/kimi-k2-thinking
       max_input_tokens: 262144
@@ -1618,6 +1369,11 @@
       input_price: 0.16
       output_price: 0.80
       supports_function_calling: true
+    - name: z-ai/glm-4.7-flash
+      max_input_tokens: 202752
+      input_price: 0.07
+      output_price: 0.40
+      supports_function_calling: true
     - name: z-ai/glm-4.6v
       max_input_tokens: 131072
       input_price: 0.3
@@ -1783,6 +1539,11 @@
       max_input_tokens: 32768
       input_price: 0.06
       output_price: 0.12
+    - name: moonshotai/Kimi-K2.5
+      max_input_tokens: 262144
+      input_price: 0.5
+      output_price: 2.8
+      supports_function_calling: true
     - name: moonshotai/Kimi-K2-Instruct-0905
       max_input_tokens: 262144
       input_price: 0.5
```

### Summary of Code Changes

1. **Commit 4f19c043**: Introduced the ability to detect React-framework projects. This involved updating .gitignore, IDE configurations, and core logic in src/client/common.rs and src/main.rs to support project-specific instructions and macros.

2. **Commit 82976d34**: Updated models.yaml with latest model configurations, including updates to pricing and metadata for various models like kimi-k2.5.

3. **Commit 0493134b**: Another update to models.yaml, continuing the maintenance of model configurations, including vision and function calling support updates.
