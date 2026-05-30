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
 *
 * @param pi
 * @returns
 */
async function registerMemoryMdInjection(pi, mem) {
  pi.on("before_agent_start", async (evt, ctx) => {
    return {
      systemPrompt: `${mem}` + evt.systemPrompt,
    };
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
  pi.on("session_start", async (evt: SessionEntryBase, ctx) => {
    const workingDir = ctx.getSessionManager().getCurrentWorkingDirectory();
    if (!workingDir) return;
    let memoryPath = path.join(workingDir, "MEMORY.md");

    if (fs.path.existsSync(memoryPath)) {
      const memoryPath = path.join(workingDir, "MEMORY.md");
      const memoryContent = await fs.readFile(memoryPath, "utf8");
      registerMemoryMdInjection(pi, memoryContent);
    }
  });

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
}
