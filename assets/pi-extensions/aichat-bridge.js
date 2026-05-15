// src/index.ts
var BRIDGE_URL = process.env.AICHAT_BRIDGE_URL;
var BRIDGE_TOKEN = process.env.AICHAT_BRIDGE_TOKEN;

let cache = {
  roles: null,
  rags: null,
  lastFetch: 0,
};
const CACHE_TTL = 60 * 1000; // 1 minute

async function fetchCache() {
  const now = Date.now();
  if (cache.roles && cache.rags && (now - cache.lastFetch < CACHE_TTL)) {
    return;
  }

  try {
    const [rolesRes, ragsRes] = await Promise.allSettled([
      bridgeFetch("/v1/state/roles", { method: "GET" }),
      bridgeFetch("/v1/state/rags", { method: "GET" }),
    ]);

    if (rolesRes.status === "fulfilled") {
      cache.roles = rolesRes.value;
    }
    if (ragsRes.status === "fulfilled") {
      cache.rags = ragsRes.value;
    }
    cache.lastFetch = now;
  } catch (err) {
    // If fetching fails, we keep the old cache if it exists
    console.error("Failed to refresh aichat bridge cache:", err);
  }
}

async function bridgeFetch(path, init = { method: "GET" }) {
  if (!BRIDGE_URL || !BRIDGE_TOKEN) {
    throw new Error(
      "aichat bridge env not set (AICHAT_BRIDGE_URL/AICHAT_BRIDGE_TOKEN). This extension is intended to be staged by `aichat --pi-repl`."
    );
  }
  const url = `${BRIDGE_URL}${path}`;
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
    throw new Error(`aichat bridge ${path} \u2192 ${res.status}: ${text}`);
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
function aichatBridge(pi) {
  if (!BRIDGE_URL || !BRIDGE_TOKEN) {
    return;
  }
  fetchCache();
  pi.registerCommand("role", {
    description: "Switch the active aichat role (e.g. /role coder)",
    getArgumentCompletions: (prefix) => {
      if (cache.roles) {
        const filtered = cache.roles.filter((r) => r.startsWith(prefix));
        return filtered.length > 0 ? filtered.map((value) => ({ value, label: value })) : null;
      }
      return null;
    },
    handler: async (args, ctx) => {
      await fetchCache();
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
    getArgumentCompletions: (prefix) => {
      if (cache.rags) {
        const filtered = cache.rags.filter((r) => r.startsWith(prefix));
        return filtered.length > 0 ? filtered.map((value) => ({ value, label: value })) : null;
      }
      return null;
    },
    handler: async (args, ctx) => {
      await fetchCache();
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
        () => bridgeFetch("/v1/state/agent", { method: "POST", body: { name, session } }),
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
        () => bridgeFetch("/v1/state/macro", { method: "POST", body: { name, text } }),
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
        () => bridgeFetch("/v1/state/exit-context", { method: "POST", body: { kind } }),
        () => `Exited ${kind}`
      );
    }
  });
}
export {
  aichatBridge as default
};
