# ResuFlow

Local-first resumes for you and your AI agent.

ResuFlow is a native desktop app (Rust + Tauri) and a built-in [MCP](https://modelcontextprotocol.io) server. You
write in a single live-preview page (Markdown that renders as you type, like Obsidian) and print from it; your agent (Claude Code, Claude Desktop, Cursor, Codex, Gemini CLI…) reads your
profile and edits the same files over stdio. Both see each other's changes live. Everything is plain Markdown in
ResuFlow's app-data folder, created automatically.

There's no account, no network listener, no token and no bundled LLM.

## How it fits together

```text
 ResuFlow window ──┐                         ┌── AI agent (any MCP client)
  (Tauri commands) │                         │   spawns `resuflow mcp` (stdio)
                   ▼                         ▼
              ┌───────────── one Rust core ─────────────┐
              │ workspace · render (A4 HTML) · checks   │
              └────────────────────┬────────────────────┘
                                   ▼
                 app-data folder (plain Markdown files)
```

One binary does everything: `resuflow` opens the app, and `resuflow mcp` is the agent server. Writes are atomic
and hash-guarded, so the app and an agent can't silently overwrite each other. The app polls for changes and
reloads, or asks you if you were mid-edit.

## Where your data lives

ResuFlow picks the folder for you: `~/.local/share/resuflow` on Linux, `~/Library/Application Support/resuflow`
on macOS, `%APPDATA%\resuflow` on Windows. On first run it copies resumes, profile and memory from the old default,
`~/Documents/ResuFlow`, and leaves the originals in place.

```text
resuflow/
  resuflow.json             marker
  profile.md                front matter = printed header · body = private career notes
  memory.md                 notes agents keep between sessions
  resumes/<slug>/resume.md  one folder per variant
  .trash/                   deleted resumes land here; nothing is erased
```

In the app, a resume is one page: the header comes from your profile, and you edit the body in place. Title,
status and tags live in the toolbar. On disk it's Markdown with a little front matter (new resumes start with
just a title):

```markdown
---
title: Senior Backend Engineer, Acme
status: draft            # draft | ready | sent | archived
tags: [acme, rust]
headline: Payments platform engineer   # optional; overrides profile.md
---

## Experience

### Senior Backend Engineer — Fintra
*Jan 2022 – Present · Pune*

- Cut p99 checkout latency 41% by moving pricing to a read-through cache.
```

## Connect an agent

In the app, open **Agent → Connect an agent** (bottom of the sidebar) for a ready-to-copy config, or:

```bash
claude mcp add resuflow -- resuflow mcp        # Claude Code
```

```json
{ "mcpServers": { "resuflow": { "command": "resuflow", "args": ["mcp"] } } }
```

The agent works on the same data folder as the app. The **Agent** panel holds everything it uses: profile, memory,
the built-in skills and this connection.

**Tools:** `get_workspace`, `read_profile`, `update_profile`, `append_memory`, `update_memory`, `list_resumes`,
`read_resume`, `create_resume`, `write_resume`, `edit_resume`, `set_resume_meta`, `delete_resume`, `check_resume`,
`render_resume`, `read_skill`.

## Built-in skills

ResuFlow ships four read-only writing skills: `role-research`, `impact-bullets`, `ats-pass` and `recruiter-review`.
Agents load them over MCP (`read_skill`, or as MCP prompts). To give your agent its own copy, open
**Agent →** any skill **→ Install in Claude Code** (`~/.claude/skills`) or **Install in other
agents** (`~/.agents/skills`), or run `resuflow install-skills claude|agents`. They install as `resuflow-<name>`, so
they never overwrite your own skills.

## Command line

```text
resuflow                               open the app
resuflow mcp                           MCP server on stdio
resuflow mcp-config                    print client config for this install
resuflow list | check SLUG | render SLUG [-o FILE]
resuflow pdf SLUG -o FILE              save as PDF (Linux)
resuflow install-skills claude|agents  copy built-in skills into an agent
```

For development, `--dir DIR` or `$RESUFLOW_DIR` points any command at another folder.

## PDF

**Save PDF** (Ctrl+Shift+S) asks where to save and writes an A4 PDF with real, selectable text. It's the only
time ResuFlow asks you for a location. **Print** (Ctrl+P) opens the system print dialog. Direct PDF export uses
WebKitGTK and works on Linux; on macOS and Windows, use Print and choose "Save as PDF". Agents can get the exact
same page as HTML with `render_resume`.

## Develop

Needs Node 20+, Rust stable, and [Tauri's system dependencies](https://tauri.app/start/prerequisites/)
(on Arch: `webkit2gtk-4.1`).

```bash
npm install
npm run app            # desktop app with hot reload
npm run check          # typecheck + Rust tests
npm run app:build      # release binary → src-tauri/target/release/resuflow
```

To use the release binary everywhere, put it on your PATH, for example
`ln -s "$PWD/src-tauri/target/release/resuflow" ~/.local/bin/resuflow`.

Layout: `src-tauri/src/` holds the Rust core (`workspace`, `doc`, `render`, `lint`, `skills`), the MCP server (`mcp`), the
desktop shell (`app`) and the CLI (`main`). `web/` holds the UI: vanilla TypeScript plus CodeMirror 6 for the live-preview
editor (`livemd.ts`). `skills/` holds the built-in skills, which are compiled into the binary.

## License

Apache-2.0. See [LICENSE](LICENSE).
