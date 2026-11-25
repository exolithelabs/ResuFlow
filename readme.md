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
on macOS, `%APPDATA%\resuflow` on Windows. On first run it copies resumes, the default template and instructions from the old default,
`~/Documents/ResuFlow`, and leaves the originals in place.

```text
resuflow/
  resuflow.json             marker
  profile.md                default template: front matter = printed header · body = copied into each new resume
  instructions.md           rules you write; agents read them and do not change them
  resumes/<slug>/resume.md  one folder per variant
  resumes/<slug>/posting.md optional: the job posting that resume targets
  .trash/                   deleted resumes land here and can be restored; nothing is erased
```

In the app, a resume is one page that looks like the printout: the header (name, headline, email, phone,
location, links) and the body edit in place. A new resume starts as a copy of the default template, or of another
resume. Next to the page, a side panel shows:

- **Review**: word count, quantified bullets, estimated pages and deterministic checks (weak openers, long
  bullets, tables, leftovers from the template). Click a finding to jump to its line.
- **Target job**: paste the posting a resume targets. ResuFlow picks out its key terms and shows which ones the
  resume covers and which are missing. Your agent sees the same posting and coverage.
- **Details**: tags, the file on disk, duplicate and move to trash (with Undo).

**Preview** (Ctrl+E) shows the exact page that prints. On disk a resume is Markdown with a little front matter (new
resumes start with just a title and the header fields):

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

In the app, open **Connect an agent** (bottom of the sidebar) for a ready-to-copy config, or:

```bash
claude mcp add resuflow -- resuflow mcp        # Claude Code
```

```json
{ "mcpServers": { "resuflow": { "command": "resuflow", "args": ["mcp"] } } }
```

The agent works on the same data folder as the app. The sidebar's workspace section holds the default template, your
instructions, the built-in skills, this connection and the trash. You edit instructions; the agent only reads them.

**Tools:** `get_workspace`, `read_profile`, `update_profile`, `read_instructions`, `list_resumes`,
`read_resume`, `create_resume`, `write_resume`, `edit_resume`, `set_resume_meta`, `delete_resume`, `list_trash`,
`restore_resume`, `read_posting`, `set_posting`, `check_resume`, `render_resume`, `read_skill`.

## Built-in skills

ResuFlow ships four read-only writing skills: `role-research`, `impact-bullets`, `ats-pass` and `recruiter-review`.
Agents load them over MCP (`read_skill`, or as MCP prompts). To give your agent its own copy, open
**Skills** in the sidebar and choose **Install in Claude Code** (`~/.claude/skills`) or **Install for other agents**
(`~/.agents/skills`), or run `resuflow install-skills claude|agents`. They install as `resuflow-<name>`, so
they never overwrite your own skills.

## Command line

```text
resuflow                               open the app
resuflow mcp                           MCP server on stdio
resuflow mcp-config                    print client config for this install
resuflow list | check SLUG | render SLUG [-o FILE]
resuflow pdf SLUG -o FILE              save as PDF (Linux)
resuflow trash | restore ID            list deleted resumes, bring one back
resuflow install-skills claude|agents  copy built-in skills into an agent
```

For development, `--dir DIR` or `$RESUFLOW_DIR` points any command at another folder.

## PDF

**Export PDF** (Ctrl+Shift+S) asks where to save and writes an A4 PDF with real, selectable text. It's the only
time ResuFlow asks you for a location. Direct PDF export uses WebKitGTK and works on Linux; elsewhere, **Print**
(Ctrl+P) opens the system print dialog, where you can save as PDF. Agents can get the exact same page as HTML with
`render_resume`.

## Keyboard

| Keys | Action |
| --- | --- |
| Ctrl+N | New resume |
| Ctrl+K | Search resumes (↑ ↓ Enter to open) |
| Ctrl+E | Switch between editing and the print preview |
| Ctrl+\\ | Show or hide the side panel |
| Ctrl+S | Save now (everything also saves as you type) |
| Ctrl+P · Ctrl+Shift+S | Print · Export PDF |
| Ctrl+B · Ctrl+I | Bold · italic in the editor |

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
desktop shell (`app`) and the CLI (`main`). `web/src/` holds the UI in vanilla TypeScript: `main.ts` (sidebar, routing,
live reload), `resume.ts` (the resume page and side panel), `views.ts` (template, instructions, skills, connect, trash),
`livemd.ts` (the CodeMirror 6 live-preview editor) and `ui.ts` (helpers, toasts, hash-guarded autosave). `skills/` holds
the built-in skills, which are compiled into the binary.

## License

Apache-2.0. See [LICENSE](LICENSE).
