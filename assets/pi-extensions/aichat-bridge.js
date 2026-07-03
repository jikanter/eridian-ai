// src/index.ts
import path from "node:path";
import fs from "node:fs/promises";
var BRIDGE_URL = process.env.AICHAT_BRIDGE_URL;
var BRIDGE_TOKEN = process.env.AICHAT_BRIDGE_TOKEN;
function formatFlag(f) {
  const forms = [];
  if (f.short) forms.push(`-${f.short}`);
  if (f.long) forms.push(`--${f.long}${f.takes_value ? " <VALUE>" : ""}`);
  const lhs = forms.join(", ");
  return f.help ? `${lhs}
    ${f.help}` : lhs;
}
async function bridgeFetch(path2, init = { method: "GET" }) {
  if (!BRIDGE_URL || !BRIDGE_TOKEN) {
    throw new Error(
      "aichat bridge env not set (AICHAT_BRIDGE_URL/AICHAT_BRIDGE_TOKEN). This extension is intended to be staged by `aichat --pi-repl`."
    );
  }
  const url = `${BRIDGE_URL}${path2}`;
  const headers = {
    Authorization: `Bearer ${BRIDGE_TOKEN}`
  };
  let body;
  if (init.body !== void 0) {
    body = JSON.stringify(init.body);
    headers["Content-Type"] = "application/json";
  }
  const res = await fetch(url, { method: init.method, headers, body });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`aichat bridge ${path2} \u2192 ${res.status}: ${text}`);
  }
  if (!text) return {};
  try {
    return JSON.parse(text);
  } catch {
    return { info: text };
  }
}
async function runWithFeedback(ctx, op, onOk) {
  try {
    const result = await op();
    const msg = onOk(result);
    if (msg) ctx.ui.notify(msg, "info");
  } catch (err) {
    ctx.ui.notify(err instanceof Error ? err.message : String(err), "error");
  }
}
var MEMORY_SUBDIR = "memory";
var MEMORY_INDEX = "MEMORY.md";
var MAX_PREAMBLE_LINES = 200;
var MAX_PREAMBLE_BYTES = 8 * 1024;
var PREAMBLE_HEADER = "# Project memory";
function capPreamble(raw) {
  let lines = raw.split("\n");
  if (lines.length > MAX_PREAMBLE_LINES) {
    lines = lines.slice(0, MAX_PREAMBLE_LINES);
  }
  let joined = lines.join("\n");
  while (Buffer.byteLength(joined, "utf8") > MAX_PREAMBLE_BYTES && lines.length > 1) {
    lines.pop();
    joined = lines.join("\n");
  }
  if (Buffer.byteLength(joined, "utf8") > MAX_PREAMBLE_BYTES) {
    joined = Buffer.from(joined, "utf8").subarray(0, MAX_PREAMBLE_BYTES).toString("utf8").replace(/�+$/, "");
  }
  return joined;
}
function registerMemoryMdInjection(pi, workingDir) {
  let cached;
  pi.on("before_agent_start", async (evt) => {
    if (cached === void 0) {
      try {
        const memoryPath = path.join(workingDir, MEMORY_SUBDIR, MEMORY_INDEX);
        const raw = await fs.readFile(memoryPath, "utf8");
        const capped = capPreamble(raw).trim();
        cached = capped ? `${PREAMBLE_HEADER}
${capped}

` : null;
      } catch {
        cached = null;
      }
    }
    if (!cached) return;
    return { systemPrompt: cached + evt.systemPrompt };
  });
}
function aichatBridge(pi) {
  if (!BRIDGE_URL || !BRIDGE_TOKEN) {
    return;
  }
  registerMemoryMdInjection(pi, process.cwd());
  pi.registerCommand("role", {
    description: "Switch the active aichat role (e.g. /role coder)",
    handler: async (args, ctx) => {
      const name = args.trim();
      if (!name) {
        ctx.ui.notify("Usage: /role <name>", "warning");
        return;
      }
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/role", { method: "POST", body: { name } }),
        () => `Switched to role: ${name}`
      );
    }
  });
  pi.registerCommand("aichat-session", {
    description: "Start/switch an aichat session (use without args for a temp session). Renamed from /session to avoid colliding with pi's built-in /session command.",
    handler: async (args, ctx) => {
      const name = args.trim() || void 0;
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/session", { method: "POST", body: { name } }),
        () => name ? `Switched to session: ${name}` : "Started temp session"
      );
    }
  });
  pi.registerCommand("rag", {
    description: "Start/switch an aichat RAG (use without args for a temp RAG)",
    handler: async (args, ctx) => {
      const name = args.trim() || void 0;
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/rag", { method: "POST", body: { name } }),
        () => name ? `Switched to RAG: ${name}` : "Started temp RAG"
      );
    }
  });
  pi.registerCommand("agent", {
    description: "Invoke an aichat agent (e.g. /agent todo, /agent coder my-session)",
    handler: async (args, ctx) => {
      const parts = args.trim().split(/\s+/).filter(Boolean);
      if (parts.length === 0) {
        ctx.ui.notify("Usage: /agent <name> [session]", "warning");
        return;
      }
      const [name, session] = parts;
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/agent", {
          method: "POST",
          body: { name, session }
        }),
        () => `Bound agent: ${name}${session ? ` (session: ${session})` : ""}`
      );
    }
  });
  pi.registerCommand("macro", {
    description: "Run an aichat macro by name (e.g. /macro plan)",
    handler: async (args, ctx) => {
      const trimmed = args.trim();
      if (!trimmed) {
        ctx.ui.notify("Usage: /macro <name> [text]", "warning");
        return;
      }
      const [name, ...rest] = trimmed.split(/\s+/);
      const text = rest.length ? rest.join(" ") : void 0;
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/macro", {
          method: "POST",
          body: { name, text }
        }),
        (result) => {
          if (typeof result.output === "string" && result.output.length > 0) {
            return result.output;
          }
          return `Macro ${name} ran (no text output)`;
        }
      );
    }
  });
  pi.registerCommand("info", {
    description: "Show current aichat context (role/agent/session/rag)",
    getArgumentCompletions: (prefix) => {
      const choices = ["role", "agent", "session", "rag"];
      const filtered = choices.filter((c) => c.startsWith(prefix));
      return filtered.length > 0 ? filtered.map((value) => ({ value, label: value })) : null;
    },
    handler: async (args, ctx) => {
      const of = args.trim();
      const qs = of ? `?of=${encodeURIComponent(of)}` : "";
      await runWithFeedback(
        ctx,
        () => bridgeFetch(`/v1/state/info${qs}`, { method: "GET" }),
        (result) => typeof result.info === "string" ? result.info : "(no info)"
      );
    }
  });
  pi.registerCommand("exit-context", {
    description: "Leave an aichat context (e.g. /exit-context role)",
    getArgumentCompletions: (prefix) => {
      const choices = ["role", "session", "rag", "agent"];
      const filtered = choices.filter((c) => c.startsWith(prefix));
      return filtered.length > 0 ? filtered.map((value) => ({ value, label: value })) : null;
    },
    handler: async (args, ctx) => {
      const kind = args.trim();
      if (!kind) {
        ctx.ui.notify("Usage: /exit-context role|session|rag|agent", "warning");
        return;
      }
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/exit-context", {
          method: "POST",
          body: { kind }
        }),
        () => `Exited ${kind}`
      );
    }
  });
  pi.registerCommand("aichat-flags", {
    description: "Discover aichat CLI flags, optionally filtered (e.g. /aichat-flags role)",
    handler: async (args, ctx) => {
      const q = args.trim();
      const qs = q ? `?q=${encodeURIComponent(q)}` : "";
      await runWithFeedback(
        ctx,
        () => bridgeFetch(`/v1/discovery/flags${qs}`, { method: "GET" }),
        (result) => {
          const flags = result.flags ?? [];
          if (flags.length === 0) {
            return q ? `No flags match "${q}"` : "No flags found";
          }
          const header = q ? `aichat flags matching "${q}" (${flags.length}):` : `aichat flags (${flags.length}):`;
          return [header, ...flags.map(formatFlag)].join("\n");
        }
      );
    }
  });
  pi.registerCommand("aichat-docs", {
    description: "List aichat feature docs, or show one (e.g. /aichat-docs server)",
    handler: async (args, ctx) => {
      const name = args.trim();
      const qs = name ? `?name=${encodeURIComponent(name)}` : "";
      await runWithFeedback(
        ctx,
        () => bridgeFetch(`/v1/discovery/docs${qs}`, { method: "GET" }),
        (result) => {
          if (name) {
            return typeof result.content === "string" ? result.content : `(no content for ${name})`;
          }
          const docs = result.docs ?? [];
          if (docs.length === 0) return "No feature docs found";
          const rows = docs.map((d) => `  ${d.name} \u2014 ${d.title}`);
          return [
            `aichat feature docs (${docs.length}):`,
            ...rows,
            "",
            "Show one with: /aichat-docs <name>"
          ].join("\n");
        }
      );
    }
  });
  pi.registerCommand("aichat-edit", {
    description: "Edit an aichat file in pi's editor (config|role|rag-docs|agent-config)",
    getArgumentCompletions: (prefix) => {
      const choices = ["config", "role", "rag-docs", "agent-config"];
      const filtered = choices.filter((c) => c.startsWith(prefix));
      return filtered.length > 0 ? filtered.map((value) => ({ value, label: value })) : null;
    },
    handler: async (args, ctx) => {
      const target = args.trim();
      if (!target) {
        ctx.ui.notify(
          "Usage: /aichat-edit config|role|rag-docs|agent-config",
          "warning"
        );
        return;
      }
      let current;
      try {
        current = await bridgeFetch(
          `/v1/state/edit?target=${encodeURIComponent(target)}`,
          { method: "GET" }
        );
      } catch (err) {
        ctx.ui.notify(err instanceof Error ? err.message : String(err), "error");
        return;
      }
      const content = typeof current.content === "string" ? current.content : "";
      const label = typeof current.label === "string" ? current.label : target;
      const edited = await ctx.ui.editor(`Edit ${label}`, content);
      if (edited === void 0) {
        ctx.ui.notify("Edit cancelled", "info");
        return;
      }
      if (edited === content) {
        ctx.ui.notify("No changes", "info");
        return;
      }
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/edit", {
          method: "POST",
          body: { target, content: edited }
        }),
        (result) => typeof result.info === "string" ? result.info : `Saved ${target}`
      );
    }
  });
  if (process.env.AICHAT_BRIDGE_SURFACE === "acp") {
    void (async () => {
      try {
        await bridgeFetch("/v1/state/subprocess", {
          method: "POST",
          body: { surface: "acp" }
        });
      } catch (err) {
        const detail = err instanceof Error ? err.message : String(err);
        console.error(`aichat-bridge: subprocess registration failed: ${detail}`);
      }
    })();
  }
}
export {
  aichatBridge as default
};
