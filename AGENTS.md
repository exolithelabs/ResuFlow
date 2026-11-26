# Working on ResuFlow

- One Rust core serves two front ends: the Tauri app (`app.rs`) and the stdio MCP server (`mcp.rs`). Put
  behaviour in `workspace.rs`, `doc.rs`, `render.rs` or `lint.rs`, never in a front end, so both stay identical.
- `main.rs` only dispatches: `resuflow` opens the app, `resuflow mcp` runs the server, plus `--dir`, `--help`
  and `--version`. Don't add CLI subcommands; new features belong in the app and, where an agent should have
  them, the MCP server.
- New user-facing actions need a `Workspace` method, a Tauri command and, unless agents mustn't have it, an MCP
  tool. Deleting a resume is app-only: agents can archive and restore, never delete.
- Every write goes through `atomic_write` and accepts an `expected_hash`. Don't add write paths that skip the guard.
- Names that become paths go through `check_slug`.
- `stdout` in `mcp` mode carries JSON-RPC only. Log to stderr.
- Built-in skills live in `skills/` and are compiled in via `skills.rs`. They are read-only at runtime; never
  write them into the workspace.
- Keep it small: no UI framework, no async runtime, no HTTP server. CodeMirror is the one big frontend
  dependency (the live-preview editor). Justify any new dependency.
- Never commit real resume data. Test against a temp folder: `--dir /tmp/rf-test` or `RESUFLOW_DIR=/tmp/rf-test`.
- After changing dependencies, run `npm run licenses` to regenerate `THIRD_PARTY_LICENSES.md`; only the licenses
  allowed in `scripts/licenses.mjs` may be added.
- Before finishing: `npm run check` (typecheck, license audit, Rust tests).
