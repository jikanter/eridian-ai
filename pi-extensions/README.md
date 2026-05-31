# pi-extensions/aichat-bridge

Source for the pi-coding-agent TypeScript extension that aichat stages into
`<cwd>/.pi/extensions/` when launched with `--pi-repl`. Pi auto-discovers the
extension on startup and exposes aichat's roles, agents, sessions, RAG, and
macros as `/role`, `/agent`, `/aichat-session`, `/rag`, `/macro`, `/info`,
and `/exit-context` slash commands. (`/aichat-session` is namespaced to
avoid colliding with pi's built-in `/session` command.) Each command makes an authenticated HTTP
call back into the aichat server running in-process on
`$AICHAT_BRIDGE_URL`.

The compiled artifact lives at `../assets/pi-extensions/aichat-bridge.js` and
is checked in so contributors without Node tooling can still build and run
the Rust side. Regenerate it after edits with:

```bash
cd pi-extensions
npm install   # first time only
npm run build
```

The build is a single esbuild invocation; the produced file is ESM,
node20-targeted, and ~5 KB.
