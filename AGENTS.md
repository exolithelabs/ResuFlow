# Working on ResuFlow

- One Rust core serves three front ends: the Tauri app (`app.rs`), the stdio MCP server (`mcp.rs`) and the CLI
  (`main.rs`). Put behaviour in `workspace.rs`, `render.rs` or `lint.rs`, never in a front end, so all three
  stay identical.
- New user-facing actions need a `Workspace` method, a Tauri command, an MCP tool and, if it makes sense, a CLI
  command.
- Every write goes through `atomic_write` and accepts an `expected_hash`. Don't add write paths that skip the guard.
- Names that become paths go through `check_slug`.
- `stdout` in `mcp` mode carries JSON-RPC only. Log to stderr.
- Built-in skills live in `skills/` and are compiled in via `skills.rs`. They are read-only at runtime; never
  write them into the workspace.
- Keep it small: no UI framework, no async runtime, no HTTP server. CodeMirror is the one big frontend
  dependency (the live-preview editor). Justify any new dependency.
- Never commit real resume data. Test against a temp folder: `--dir /tmp/rf-test` or `RESUFLOW_DIR=/tmp/rf-test`.
- Before finishing: `npm run check`.
