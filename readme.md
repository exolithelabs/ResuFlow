# ResuFlow

Local-first resumes for you and your AI agent.

ResuFlow is a native desktop app (Rust + Tauri) and a built-in [MCP](https://modelcontextprotocol.io) server. You
write in a single live-preview page (Markdown that renders as you type, like Obsidian) and print from it; your AI agent reads your
profile and edits the same files over stdio. Both see each other's changes live. Everything is plain Markdown in
ResuFlow's app-data folder, created automatically.

There's no account, no network listener, no token and no bundled LLM. The only request ResuFlow makes is the update
check against this project's GitHub releases, which you can turn off (About → Check when ResuFlow starts).

## Install

Download the latest release from [GitHub Releases](https://github.com/exolithelabs/ResuFlow/releases/latest):

| System | File | Notes |
| --- | --- | --- |
| Linux (x86_64) | `ResuFlow_<version>_amd64.AppImage` | `chmod +x` it, then run it. |
| Windows (x86_64) | `ResuFlow_<version>_x64-setup.exe` | Installs for your user only; no admin rights needed. |
| macOS (Apple silicon and Intel) | `ResuFlow_<version>_universal.dmg` | Drag ResuFlow to Applications. |

### Updates

ResuFlow checks for a new release when it starts and tells you when one is ready; **About → Check for updates** does
it on demand. Installing saves your open edits, downloads the update, verifies its signature against the key built
into the app (an unsigned or tampered download is refused) and restarts into the new version. Self-update works for
the AppImage, the Windows install and the macOS app; a copy built from source or installed by a package manager is
updated the way you installed it.

An update never changes your data by itself. When a new version stores data differently, it upgrades your data
folder the first time it opens it, safely:

- it first copies the whole folder to `.backups/format-<old>-<time>/` inside it, and keeps the last five copies;
- if the upgrade fails, or the app is closed or crashes partway through, the folder is put back exactly as it was
  from that copy (next start, if need be);
- the app and an agent's MCP server never upgrade at the same time;
- an older version refuses to open, or write to, a folder a newer one has upgraded, rather than damage it. A
  ResuFlow still running from before the update (an agent's MCP server, say) stops writing until it is restarted.

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
  resuflow.json              marker
  profile.mdx                default template, header included, copied into each new resume
  instructions.json          rules you write; agents read them and do not change them
  resumes/<slug>/resume.mdx  one folder per variant
  resumes/<slug>/posting.txt optional: the job posting that resume targets
```

Opening a folder from an older version converts it in place, after taking a backup (see [Updates](#updates)):
`instructions.md` becomes `instructions.json` and each `posting.md` becomes `posting.txt`. `resuflow.json` records
the folder's data format.

In the app, a resume is one page of Markdown that looks like the printout. All of it edits in place, Obsidian-style,
header and section rules included: formatting renders as you type, and the raw Markdown (`#`, `**`, `---`) shows
on the line the cursor is on. Right-click for a formatting menu. A new resume starts as a copy of the default
template, or of another resume. Next to the page, a side panel shows:

- **Review**: word count, quantified bullets, estimated pages and deterministic checks (weak openers, long
  bullets, tables, leftovers from the template). Click a finding to jump to its line.
- **Target job**: paste the posting a resume targets. ResuFlow picks out its key terms and shows which ones the
  resume covers and which are missing. Your agent sees the same posting and coverage.
- **Details**: tags, the file on disk, duplicate and delete. Deleting asks first and erases the resume
  for good; to put one away without losing it, archive it from the list's menu (with Undo). Archived resumes live in
  the sidebar's Archive, where each can be restored or deleted.

On disk a resume is an `.mdx` file: Markdown with a little front matter, plus two optional styling components. The body opens with the header: a `# Name` line, a
headline line, then a contact line whose items are separated by ` · ` (emails, phone numbers and web addresses print
as links). A `---` under a section heading prints as its rule.

```markdown
---
title: Senior Backend Engineer, Acme
status: draft            # draft | ready | sent | archived
tags: [acme, rust]
---

# Ada Lovelace
Payments platform engineer
Pune · ada@example.com · +91 98765 43210 · github.com/ada

## Experience
---

### Senior Backend Engineer — Fintra
*Jan 2022 – Present · Pune*

- Cut p99 checkout latency 41% by moving pricing to a read-through cache.
```

To style words or a rule, use the two components below. They render live in the editor and print the same way:

```mdx
## <Text color="#1f4e8c" size="10pt" caps spacing="0.08em">Experience</Text>
<Rule color="#d6d9de" />

Cut latency <Text color="#b42318">41%</Text> by <Text underline underlineColor="#1f4e8c">moving pricing to a cache</Text>.
```

`<Text>` takes any of `color`, `underline`, `underlineColor`, `size`, `caps`, `spacing` and `font` (`serif`, `sans` or
`mono`); `<Rule />`, on its own line, takes `color`. Attribute values are quoted strings (or a bare `underline` or
`caps`); colours are hex, CSS names, `rgb()` or `hsl()`, and sizes are lengths such as `9.5pt` or `0.08em`.

The default template uses this styling for its look: a muted headline and contact line, small blue capitals for
section headings and muted dates. Right-click in the editor for colour and underline-colour swatches, font chips (small,
large, capitals, spacing, serif, mono), rule colour, and **Apply resume style**, which gives a plain resume the
template's look in one step. With nothing selected, styling applies to the whole heading or header line, or to the
word at the cursor. That is the whole
of the MDX ResuFlow reads: nothing in a resume runs as code, and other tags, imports or `{expressions}` print as plain
text.

Workspaces from earlier versions, which kept the header in front matter, are converted the first time they open: `.md` files are renamed
to `.mdx` (their text is unchanged), and if the header was still in front matter, each resume gets the header it used to
print, filled in from the default template where it relied on it.

## Connect an agent

In the app, open **Connect an agent** (bottom of the sidebar) for a ready-to-copy config that uses the full path of
this install. If `resuflow` is on your PATH, this works too:

```json
{ "mcpServers": { "resuflow": { "command": "resuflow", "args": ["mcp"] } } }
```

The agent works on the same data folder as the app. The sidebar's workspace section holds the default template, your
instructions, the built-in skills, this connection and the archive. You edit instructions; the agent only reads them.

Your instructions reach the agent when it connects: they are appended to the server's `initialize` instructions,
scoped to resume work, and `get_workspace` returns the current ones.

**Tools:** `get_workspace`, `read_profile`, `update_profile`, `list_resumes`, `read_resume`, `create_resume`,
`write_resume`, `edit_resume`, `set_resume_meta`, `archive_resume`, `restore_resume`, `read_posting`,
`set_posting`, `check_resume`, `render_resume`, `read_skill`. Agents can archive and restore resumes but never delete
them; only you can, in the app.

**Resources** (read-only Markdown): `resuflow://profile`, `resuflow://instructions`, `resuflow://resumes/{slug}` and
`skill://{name}/SKILL.md`.

**Prompts:** one per built-in skill, with an optional `slug` argument. Clients that support completion get resume
slugs suggested for it, and for the resource templates' `{slug}` and `{name}`.

## Built-in skills

ResuFlow ships four read-only writing skills: `role-research`, `impact-bullets`, `ats-pass` and `recruiter-review`.
Agents load them over MCP: `read_skill`, the MCP prompt of the same name, or the `skill://{name}/SKILL.md` resource.

## Command line

```text
resuflow                   open the app
resuflow mcp               MCP server on stdio (what your agent runs)
resuflow --help | --version
```

Everything else happens in the app or through your agent. For development, `--dir DIR` or `$RESUFLOW_DIR` points
either one at another folder.

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
| Ctrl+\\ | Show or hide the side panel |
| Ctrl+S | Save now (everything also saves as you type) |
| Ctrl+P · Ctrl+Shift+S | Print · Export PDF |
| Ctrl+B · Ctrl+I · Ctrl+U | Bold · italic · underline in the editor |
| Right-click | Formatting and styling menu in the resume and template editors (Shift+right-click for spelling) |

## Develop

Needs Node 20+, Rust stable, and [Tauri's system dependencies](https://tauri.app/start/prerequisites/)
(on Arch: `webkit2gtk-4.1`).

```bash
npm install
npm run app            # desktop app with hot reload
npm run check          # typecheck + license audit + Rust tests
npm run app:build      # release binary → src-tauri/target/release/resuflow
npm run app:bundle     # installer for this OS (needs the updater signing key, see Release)
npm run licenses       # regenerate THIRD_PARTY_LICENSES.md after changing dependencies
```

To use the release binary everywhere, put it on your PATH, for example
`ln -s "$PWD/src-tauri/target/release/resuflow" ~/.local/bin/resuflow`.

Layout: `src-tauri/src/` holds the Rust core (`workspace`, `config`, `doc`, `render`, `lint`, `skills`, `about`), the MCP server (`mcp`), the
desktop shell (`app`, with `pdf` for direct export) and `main`, which only picks between the app and the MCP
server. `web/src/` holds the UI in vanilla TypeScript: `main.ts` (sidebar, routing,
live reload), `resume.ts` (the resume page and side panel), `views.ts` (template, instructions, skills, connect, archive, about and updates),
`livemd.ts` (the CodeMirror 6 live-preview editor), `editmenu.ts` (its right-click formatting menu) and `ui.ts` (helpers, toasts, hash-guarded autosave). `skills/` holds
the built-in skills, which are compiled into the binary.

### Release

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and run
   `npm run check`.
2. Tag and push: `git tag v0.3.0 && git push origin v0.3.0`.

[`.github/workflows/release.yml`](.github/workflows/release.yml) checks the versions match, runs the checks, builds
the AppImage, the Windows installer and a universal macOS app and DMG into a draft release, and publishes it only
when every platform built and the updater manifest (`latest.json`) lists a signed update for each. Installed copies
look for updates in the latest published release.

Updates are signed with a minisign key. Its public half is `plugins.updater.pubkey` in `tauri.conf.json`; the
private half goes in the repository secret `TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
if it has one). Keep a safe copy of it: if it is lost, installed copies can no longer be updated and users have to
reinstall once. To bundle locally, export the same variables. macOS builds are ad-hoc signed unless the optional
`APPLE_*` secrets listed in the workflow are set, in which case they are signed and notarized.

### Dependencies and licenses

Only dependencies under the permissive licenses (and MPL-2.0) allowed in [`scripts/licenses.mjs`](scripts/licenses.mjs)
may be added. `npm run check`, and so CI, fails when a dependency's license isn't allowed or when
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) is out of date; `npm run licenses` regenerates it. The file is
compiled into the app (About → Licenses) and installed next to it.

## License

ResuFlow is licensed under the Apache License, Version 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE). It is built
on open-source packages under their own licenses, listed with their full texts in
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
