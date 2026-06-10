/**
 * aichat ↔ pi slash-command bridge.
 *
 * When the user runs `aichat --pi-repl` (or `AICHAT_REPL=pi aichat`), aichat
 * stages this extension into `<cwd>/.pi/extensions/aichat-bridge.js` and execs
 * `pi`. Pi auto-discovers the file, calls the default export with its
 * `ExtensionAPI`, and registers the slash commands defined below. Each
 * command translates to an HTTP call against the aichat server running
 * in-process on `AICHAT_BRIDGE_URL`, authenticated with `AICHAT_BRIDGE_TOKEN`.
 *
 * The same artifact is safe to drop into `~/.pi/agent/extensions/` outside
 * of an aichat-managed launch — it self-detects the absence of the bridge
 * env vars and becomes a no-op rather than throwing.
 */

import type {
  ExtensionAPI,
  ExtensionCommandContext,
} from "@earendil-works/pi-coding-agent";

import path from "node:path";
import fs from "node:fs/promises";

const BRIDGE_URL = process.env.AICHAT_BRIDGE_URL;
const BRIDGE_TOKEN = process.env.AICHAT_BRIDGE_TOKEN;

interface BridgeResult {
  ok?: boolean;
  info?: string;
  output?: string;
  [key: string]: unknown;
}

/** One CLI flag, as returned by `GET /v1/discovery/flags`. */
interface FlagInfo {
  long: string | null;
  short: string | null;
  help: string;
  takes_value: boolean;
}

/** One embedded feature doc, as returned by `GET /v1/discovery/docs`. */
interface DocInfo {
  name: string;
  file: string;
  title: string;
}

/** Render a flag the way `aichat --help` would: `-s, --long <VALUE>  help`. */
function formatFlag(f: FlagInfo): string {
  const forms: string[] = [];
  if (f.short) forms.push(`-${f.short}`);
  if (f.long) forms.push(`--${f.long}${f.takes_value ? " <VALUE>" : ""}`);
  const lhs = forms.join(", ");
  return f.help ? `${lhs}\n    ${f.help}` : lhs;
}

async function bridgeFetch(
  path: string,
  init: { method: "GET" | "POST"; body?: unknown } = { method: "GET" },
): Promise<BridgeResult> {
  if (!BRIDGE_URL || !BRIDGE_TOKEN) {
    throw new Error(
      "aichat bridge env not set (AICHAT_BRIDGE_URL/AICHAT_BRIDGE_TOKEN). " +
        "This extension is intended to be staged by `aichat --pi-repl`.",
    );
  }
  const url = `${BRIDGE_URL}${path}`;
  const headers: Record<string, string> = {
    Authorization: `Bearer ${BRIDGE_TOKEN}`,
  };
  let body: string | undefined;
  if (init.body !== undefined) {
    body = JSON.stringify(init.body);
    headers["Content-Type"] = "application/json";
  }
  const res = await fetch(url, { method: init.method, headers, body });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`aichat bridge ${path} → ${res.status}: ${text}`);
  }

  if (!text) return {};
  try {
    return JSON.parse(text) as BridgeResult;
  } catch {
    // Server returned non-JSON; surface the raw body.
    return { info: text };
  }
}

/**
 * Run a bridge call wrapped with consistent user-facing feedback. On
 * success, surface either the `info` text (read endpoints) or a short
 * confirmation (write endpoints). On failure, render the error inline.
 */
async function runWithFeedback(
  ctx: ExtensionCommandContext,
  op: () => Promise<BridgeResult>,
  onOk: (result: BridgeResult) => string | undefined,
): Promise<void> {
  try {
    const result = await op();
    const msg = onOk(result);
    if (msg) ctx.ui.notify(msg, "info");
  } catch (err) {
    ctx.ui.notify(err instanceof Error ? err.message : String(err), "error");
  }
}

/**
 * Phase 34A: auto-memory read surface for pi's *native* agent turns.
 *
 * aichat's Rust side injects `memory/MEMORY.md` into the system prompt for
 * role/agent/prompt turns (see `src/memory/mod.rs`), but pi's own agent loop
 * builds its system prompt independently of any aichat role. This hook fills
 * that gap: it reads the project-local `memory/MEMORY.md`, caps it to the same
 * 200-line / 8-KiB budget as the Rust loader, and prepends the capped block to
 * pi's system prompt. Read-only — no write loop here (34C/34D deferred).
 */
const MEMORY_SUBDIR = "memory";
const MEMORY_INDEX = "MEMORY.md";
const MAX_PREAMBLE_LINES = 200;
const MAX_PREAMBLE_BYTES = 8 * 1024;
const PREAMBLE_HEADER = "# Project memory";

/** Cap raw MEMORY.md content to the line/byte budget, mirroring `cap_preamble`. */
function capPreamble(raw: string): string {
  let lines = raw.split("\n");
  if (lines.length > MAX_PREAMBLE_LINES) {
    lines = lines.slice(0, MAX_PREAMBLE_LINES);
  }
  let joined = lines.join("\n");
  while (
    Buffer.byteLength(joined, "utf8") > MAX_PREAMBLE_BYTES &&
    lines.length > 1
  ) {
    lines.pop();
    joined = lines.join("\n");
  }
  // A lone over-budget line: hard-truncate on a byte budget (Buffer slicing
  // respects nothing, so re-decode and drop the trailing partial char).
  if (Buffer.byteLength(joined, "utf8") > MAX_PREAMBLE_BYTES) {
    joined = Buffer.from(joined, "utf8")
      .subarray(0, MAX_PREAMBLE_BYTES)
      .toString("utf8")
      .replace(/�+$/, "");
  }
  return joined;
}

/**
 * Register a one-time `before_agent_start` hook that loads, caps, and prepends
 * the project-local memory block. The block is read once and cached so
 * multi-turn sessions don't re-read the file.
 */
function registerMemoryMdInjection(pi: ExtensionAPI, workingDir: string): void {
  let cached: string | null | undefined; // undefined = not yet read
  pi.on("before_agent_start", async (evt: { systemPrompt: string }) => {
    if (cached === undefined) {
      try {
        const memoryPath = path.join(workingDir, MEMORY_SUBDIR, MEMORY_INDEX);
        const raw = await fs.readFile(memoryPath, "utf8");
        const capped = capPreamble(raw).trim();
        cached = capped ? `${PREAMBLE_HEADER}\n${capped}\n\n` : null;
      } catch {
        cached = null; // no memory file — become a no-op for the session.
      }
    }
    if (!cached) return;
    return { systemPrompt: cached + evt.systemPrompt };
  });
}

/**
 * Default export: pi calls this once at startup, passing its ExtensionAPI.
 * If the bridge env is absent (e.g. extension dropped in manually without
 * aichat managing the launch), register nothing — surfacing dead commands
 * would be worse than being invisible.
 */
export default function aichatBridge(pi: ExtensionAPI): void {
  if (!BRIDGE_URL || !BRIDGE_TOKEN) {
    // Silent no-op. The aichat launcher always sets these.
    return;
  }
  // Phase 34A: prepend project-local `memory/MEMORY.md` to pi's native system
  // prompt. The hook reads lazily on the first agent turn, so we register it
  // up front with the launch working directory.
  registerMemoryMdInjection(pi, process.cwd());

  pi.registerCommand("role", {
    description: "Switch the active aichat role (e.g. /role coder)",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const name = args.trim();
      if (!name) {
        ctx.ui.notify("Usage: /role <name>", "warning");
        return;
      }
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/role", { method: "POST", body: { name } }),
        () => `Switched to role: ${name}`,
      );
    },
  });

  pi.registerCommand("aichat-session", {
    description:
      "Start/switch an aichat session (use without args for a temp session). Renamed from /session to avoid colliding with pi's built-in /session command.",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const name = args.trim() || undefined;
      await runWithFeedback(
        ctx,
        () =>
          bridgeFetch("/v1/state/session", { method: "POST", body: { name } }),
        () => (name ? `Switched to session: ${name}` : "Started temp session"),
      );
    },
  });

  pi.registerCommand("rag", {
    description: "Start/switch an aichat RAG (use without args for a temp RAG)",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const name = args.trim() || undefined;
      await runWithFeedback(
        ctx,
        () => bridgeFetch("/v1/state/rag", { method: "POST", body: { name } }),
        () => (name ? `Switched to RAG: ${name}` : "Started temp RAG"),
      );
    },
  });

  pi.registerCommand("agent", {
    description:
      "Invoke an aichat agent (e.g. /agent todo, /agent coder my-session)",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const parts = args.trim().split(/\s+/).filter(Boolean);
      if (parts.length === 0) {
        ctx.ui.notify("Usage: /agent <name> [session]", "warning");
        return;
      }
      const [name, session] = parts;
      await runWithFeedback(
        ctx,
        () =>
          bridgeFetch("/v1/state/agent", {
            method: "POST",
            body: { name, session },
          }),
        () => `Bound agent: ${name}${session ? ` (session: ${session})` : ""}`,
      );
    },
  });

  pi.registerCommand("macro", {
    description: "Run an aichat macro by name (e.g. /macro plan)",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const trimmed = args.trim();
      if (!trimmed) {
        ctx.ui.notify("Usage: /macro <name> [text]", "warning");
        return;
      }
      const [name, ...rest] = trimmed.split(/\s+/);
      const text = rest.length ? rest.join(" ") : undefined;
      await runWithFeedback(
        ctx,
        () =>
          bridgeFetch("/v1/state/macro", {
            method: "POST",
            body: { name, text },
          }),
        (result) => {
          if (typeof result.output === "string" && result.output.length > 0) {
            return result.output;
          }
          return `Macro ${name} ran (no text output)`;
        },
      );
    },
  });

  pi.registerCommand("info", {
    description: "Show current aichat context (role/agent/session/rag)",
    getArgumentCompletions: (prefix: string) => {
      const choices = ["role", "agent", "session", "rag"];
      const filtered = choices.filter((c) => c.startsWith(prefix));
      return filtered.length > 0
        ? filtered.map((value) => ({ value, label: value }))
        : null;
    },
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const of = args.trim();
      const qs = of ? `?of=${encodeURIComponent(of)}` : "";
      await runWithFeedback(
        ctx,
        () => bridgeFetch(`/v1/state/info${qs}`, { method: "GET" }),
        (result) =>
          typeof result.info === "string" ? result.info : "(no info)",
      );
    },
  });

  pi.registerCommand("exit-context", {
    description: "Leave an aichat context (e.g. /exit-context role)",
    getArgumentCompletions: (prefix: string) => {
      const choices = ["role", "session", "rag", "agent"];
      const filtered = choices.filter((c) => c.startsWith(prefix));
      return filtered.length > 0
        ? filtered.map((value) => ({ value, label: value }))
        : null;
    },
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const kind = args.trim();
      if (!kind) {
        ctx.ui.notify("Usage: /exit-context role|session|rag|agent", "warning");
        return;
      }
      await runWithFeedback(
        ctx,
        () =>
          bridgeFetch("/v1/state/exit-context", {
            method: "POST",
            body: { kind },
          }),
        () => `Exited ${kind}`,
      );
    },
  });

  // Phase 53: discovery surface — find aichat's flags and feature docs from
  // inside the REPL without dropping to a shell. Both are read-only GETs
  // against the same bridge server; nothing about the live context changes.
  pi.registerCommand("aichat-flags", {
    description:
      "Discover aichat CLI flags, optionally filtered (e.g. /aichat-flags role)",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const q = args.trim();
      const qs = q ? `?q=${encodeURIComponent(q)}` : "";
      await runWithFeedback(
        ctx,
        () => bridgeFetch(`/v1/discovery/flags${qs}`, { method: "GET" }),
        (result) => {
          const flags = (result.flags as FlagInfo[] | undefined) ?? [];
          if (flags.length === 0) {
            return q ? `No flags match "${q}"` : "No flags found";
          }
          const header = q
            ? `aichat flags matching "${q}" (${flags.length}):`
            : `aichat flags (${flags.length}):`;
          return [header, ...flags.map(formatFlag)].join("\n");
        },
      );
    },
  });

  pi.registerCommand("aichat-docs", {
    description:
      "List aichat feature docs, or show one (e.g. /aichat-docs server)",
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const name = args.trim();
      const qs = name ? `?name=${encodeURIComponent(name)}` : "";
      await runWithFeedback(
        ctx,
        () => bridgeFetch(`/v1/discovery/docs${qs}`, { method: "GET" }),
        (result) => {
          if (name) {
            return typeof result.content === "string"
              ? result.content
              : `(no content for ${name})`;
          }
          const docs = (result.docs as DocInfo[] | undefined) ?? [];
          if (docs.length === 0) return "No feature docs found";
          const rows = docs.map((d) => `  ${d.name} — ${d.title}`);
          return [
            `aichat feature docs (${docs.length}):`,
            ...rows,
            "",
            "Show one with: /aichat-docs <name>",
          ].join("\n");
        },
      );
    },
  });

  // `.edit <target>` bridge. The legacy REPL spawned `$EDITOR` on a YAML file;
  // here pi owns the TTY, so we round-trip through pi's *native* in-TUI editor
  // instead: GET the current text, hand it to `ctx.ui.editor`, then POST the
  // result back for aichat to persist and reload. `session` is intentionally
  // absent — pi owns that format, so sessions are edited via pi's own surface.
  pi.registerCommand("aichat-edit", {
    description:
      "Edit an aichat file in pi's editor (config|role|rag-docs|agent-config)",
    getArgumentCompletions: (prefix: string) => {
      const choices = ["config", "role", "rag-docs", "agent-config"];
      const filtered = choices.filter((c) => c.startsWith(prefix));
      return filtered.length > 0
        ? filtered.map((value) => ({ value, label: value }))
        : null;
    },
    handler: async (args: string, ctx: ExtensionCommandContext) => {
      const target = args.trim();
      if (!target) {
        ctx.ui.notify(
          "Usage: /aichat-edit config|role|rag-docs|agent-config",
          "warning",
        );
        return;
      }

      // 1. Read the current on-disk text from the bridge. A 4xx (e.g. no
      //    active role, or `session`) surfaces here as a thrown error.
      let current: BridgeResult;
      try {
        current = await bridgeFetch(
          `/v1/state/edit?target=${encodeURIComponent(target)}`,
          { method: "GET" },
        );
      } catch (err) {
        ctx.ui.notify(err instanceof Error ? err.message : String(err), "error");
        return;
      }
      const content = typeof current.content === "string" ? current.content : "";
      const label = typeof current.label === "string" ? current.label : target;

      // 2. Edit in pi's native multi-line editor, seeded with the content.
      const edited = await ctx.ui.editor(`Edit ${label}`, content);
      if (edited === undefined) {
        ctx.ui.notify("Edit cancelled", "info");
        return;
      }
      if (edited === content) {
        ctx.ui.notify("No changes", "info");
        return;
      }

      // 3. Persist + reload through the bridge; surface its `info` summary.
      await runWithFeedback(
        ctx,
        () =>
          bridgeFetch("/v1/state/edit", {
            method: "POST",
            body: { target, content: edited },
          }),
        (result) =>
          typeof result.info === "string" ? result.info : `Saved ${target}`,
      );
    },
  });

  // register the subprocess with acp if invoking under zed
  // generate a subprocess by invoking the bridge. `aichatBridge` is sync (pi
  // calls it without awaiting), so fire-and-forget inside an async IIFE.
  if (process.env.ZED_BRIDGE_URL) {
    void (async () => {
      await bridgeFetch("/v1/state/subprocess", {
        method: "POST",
        body: {},
      });
    })();
  }
}
