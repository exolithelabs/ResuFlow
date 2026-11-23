import { api, withSlug } from './api';
import type {
  McpInfo,
  Memory,
  Profile,
  ReleaseCheck,
  Resume,
  ResumeSummary,
  Skill,
  WorkspaceInfo,
} from './types';

if (new URLSearchParams(window.location.search).get('desktop') === '1') {
  document.documentElement.classList.add('desktop-app');
}

const app = document.querySelector<HTMLElement>('#app')!;
const saveStatus = document.querySelector<HTMLElement>('#save-status')!;
const releaseNotice = document.querySelector<HTMLElement>('#release-notice')!;
const releaseTitle = document.querySelector<HTMLElement>('#release-title')!;
const releaseDetail = document.querySelector<HTMLElement>('#release-detail')!;
const releaseLink = document.querySelector<HTMLAnchorElement>('#release-link')!;
const releaseDismiss = document.querySelector<HTMLButtonElement>('#release-dismiss')!;

const createDialog = document.querySelector<HTMLDialogElement>('#create-dialog')!;
const createForm = document.querySelector<HTMLFormElement>('#create-form')!;
const createTitle = document.querySelector<HTMLInputElement>('#create-title')!;
const createCancel = document.querySelector<HTMLButtonElement>('#create-cancel')!;
const skillDialog = document.querySelector<HTMLDialogElement>('#skill-dialog')!;
const skillForm = document.querySelector<HTMLFormElement>('#skill-form')!;
const skillName = document.querySelector<HTMLInputElement>('#skill-name')!;
const skillCancel = document.querySelector<HTMLButtonElement>('#skill-cancel')!;

type EditorState =
  | { kind: 'resume'; resume: Resume }
  | { kind: 'profile'; markdown: string }
  | { kind: 'skill'; slug: string; markdown: string; source: 'builtin' | 'user' }
  | { kind: 'memory'; markdown: string }
  | null;

let saveTimer = 0;
let updateStatusTimer = 0;
let editorState: EditorState = null;

window.addEventListener('hashchange', () => {
  closeSettingsMenu();
  void render();
});

const settingsBtn = document.querySelector<HTMLButtonElement>('#settings-btn')!;
const settingsDropdown = document.querySelector<HTMLElement>('#settings-dropdown')!;
const openBrowserBtn = document.querySelector<HTMLButtonElement>('#open-browser')!;
const checkUpdatesBtn = document.querySelector<HTMLButtonElement>('#check-updates')!;

settingsBtn.addEventListener('click', (event) => {
  event.stopPropagation();
  const open = settingsDropdown.hidden;
  settingsDropdown.hidden = !open;
  settingsBtn.setAttribute('aria-expanded', String(open));
});
settingsDropdown.addEventListener('click', () => closeSettingsMenu());
openBrowserBtn.addEventListener('click', async () => {
  await api('open-browser', {
    method: 'POST',
    body: JSON.stringify({ hash: location.hash }),
  });
});
checkUpdatesBtn.addEventListener('click', () => void checkForLatestRelease({ manual: true }));
document.addEventListener('click', (event) => {
  const target = event.target as Element | null;
  if (!target?.closest('.settings-menu')) closeSettingsMenu();
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') closeSettingsMenu();
});

function closeSettingsMenu(): void {
  settingsDropdown.hidden = true;
  settingsBtn.setAttribute('aria-expanded', 'false');
}

createCancel.addEventListener('click', () => createDialog.close());
skillCancel.addEventListener('click', () => skillDialog.close());

skillForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  const name = skillName.value.trim();
  if (!name) return;
  const skill = await api<Skill>('skills', {
    method: 'POST',
    body: JSON.stringify({ name }),
  });
  skillDialog.close();
  skillForm.reset();
  location.hash = `#/skills/${skill.slug}`;
});

createForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  const title = createTitle.value.trim();
  if (!title) return;
  const resume = await api<Resume>('resumes', {
    method: 'POST',
    body: JSON.stringify({ title }),
  });
  createDialog.close();
  createForm.reset();
  location.hash = `#/edit/${resume.slug}`;
});

void render();
void checkForLatestRelease();

async function checkForLatestRelease({ manual = false }: { manual?: boolean } = {}): Promise<void> {
  try {
    const release = await api<ReleaseCheck>(`releases/latest${manual ? '?refresh=1' : ''}`);
    if (!release.available || !release.tagName || !release.url) {
      if (manual) showUpdateCheckStatus(`ResuFlow ${release.currentVersion} is up to date.`);
      return;
    }
    const tagName = release.tagName;
    if (!manual && localStorage.getItem('resuflow-dismissed-release') === tagName) return;

    releaseTitle.textContent = `${release.name || tagName} is available.`;
    releaseDetail.textContent = `You are using ${release.currentVersion}. Review the release before updating.`;
    releaseLink.href = release.url;
    releaseNotice.hidden = false;

    releaseDismiss.onclick = () => {
      localStorage.setItem('resuflow-dismissed-release', tagName);
      releaseNotice.hidden = true;
    };
  } catch {
    if (manual) showUpdateCheckStatus('Could not check GitHub releases. Try again when online.');
  }
}

function showUpdateCheckStatus(message: string): void {
  window.clearTimeout(updateStatusTimer);
  const previousMessage = saveStatus.textContent;
  const wasHidden = saveStatus.hidden;
  saveStatus.hidden = false;
  saveStatus.textContent = message;
  updateStatusTimer = window.setTimeout(() => {
    if (saveStatus.textContent !== message) return;
    saveStatus.textContent = previousMessage;
    saveStatus.hidden = wasHidden;
  }, 4000);
}

type Route =
  | { name: 'dashboard' }
  | { name: 'profile' }
  | { name: 'skills' }
  | { name: 'skill'; slug: string }
  | { name: 'memory' }
  | { name: 'vault' }
  | { name: 'docs' }
  | { name: 'edit' | 'preview'; slug: string };

async function render(): Promise<void> {
  const route = parseRoute();
  const backHome = document.querySelector<HTMLElement>('#back-home');
  if (backHome) backHome.hidden = route.name === 'dashboard';

  if (route.name === 'profile') {
    await renderProfileEditor();
    return;
  }
  if (route.name === 'skills') {
    await renderSkills();
    return;
  }
  if (route.name === 'skill') {
    await renderSkillEditor(route.slug);
    return;
  }
  if (route.name === 'memory') {
    await renderMemory();
    return;
  }
  if (route.name === 'docs') {
    await renderDocs();
    return;
  }
  if (route.name === 'vault') {
    await renderVault();
    return;
  }
  if (route.name === 'edit' || route.name === 'preview') {
    await renderEditor(route.slug, route.name);
    return;
  }
  await renderDashboard();
}

function parseRoute(): Route {
  const hash = location.hash.replace(/^#/, '') || '/';
  if (hash === '/profile') return { name: 'profile' };
  if (hash === '/skills') return { name: 'skills' };
  if (hash === '/memory') return { name: 'memory' };
  if (hash === '/vault') return { name: 'vault' };
  if (hash === '/docs' || hash.startsWith('/docs/')) return { name: 'docs' };

  const skill = hash.match(/^\/skills\/([^/]+)\/?$/);
  if (skill) return { name: 'skill', slug: decodeURIComponent(skill[1]) };

  const edit = hash.match(/^\/(edit|preview)\/([^/]+)\/?$/);
  if (edit && (edit[1] === 'edit' || edit[1] === 'preview')) {
    return { name: edit[1], slug: decodeURIComponent(edit[2]) };
  }
  return { name: 'dashboard' };
}

async function renderDashboard(): Promise<void> {
  editorState = null;
  saveStatus.hidden = true;
  const { resumes } = await api<{ resumes: ResumeSummary[] }>('resumes');

  app.innerHTML = `
    <section class="hero">
      <div>
        <p class="eyebrow">Local workspace</p>
        <h1>Your resumes</h1>
        <p class="lede">Everything is Markdown. Use Edit to write, Preview to see and print the A4 page.</p>
      </div>
      <div class="actions">
        <button class="primary" id="new-resume">New resume</button>
      </div>
    </section>

    <section class="toolbar">
      <span class="muted">${resumes.length} ${resumes.length === 1 ? 'resume' : 'resumes'}</span>
      <label class="search">
        <span class="sr-only">Search</span>
        <input id="resume-search" type="search" placeholder="Search role or title…" />
      </label>
    </section>
    <section id="resume-grid" class="resume-list-wrap"></section>
  `;

  const grid = app.querySelector<HTMLElement>('#resume-grid')!;
  const search = app.querySelector<HTMLInputElement>('#resume-search')!;

  const draw = (items: ResumeSummary[]) => {
    if (!items.length) {
      grid.innerHTML = `<div class="empty-state"><h2>No resumes yet</h2><p>Create a resume, then write it in Markdown. Preview renders the designed A4 page.</p></div>`;
      return;
    }
    grid.innerHTML = `<ul class="resume-list">${items.map(listItemHtml).join('')}</ul>`;
  };
  draw(resumes);

  search.addEventListener('input', () => {
    const query = search.value.trim().toLowerCase();
    draw(
      resumes.filter((resume) =>
        [resume.title, resume.summary, ...(resume.tags || [])].join(' ').toLowerCase().includes(query),
      ),
    );
  });

  app.querySelector<HTMLButtonElement>('#new-resume')!.addEventListener('click', () => {
    createTitle.value = '';
    createDialog.showModal();
    createTitle.focus();
  });

  grid.addEventListener('click', async (event) => {
    const target = event.target as Element | null;
    const button = target?.closest<HTMLButtonElement>('[data-action]');
    if (!button) return;
    const slug = button.dataset.slug;
    if (!slug) return;

    if (button.dataset.action === 'delete') {
      if (!confirm(`Delete ${slug}? This cannot be undone.`)) return;
      await api('resume', withSlug(slug, { method: 'DELETE' }));
      await renderDashboard();
    }
    if (button.dataset.action === 'print') {
      printResume(slug);
    }
  });
}

function listItemHtml(resume: ResumeSummary): string {
  return `
    <li class="resume-row">
      <a class="resume-title" href="#/edit/${encodeURIComponent(resume.slug)}">${escapeHtml(resume.title)}</a>
      <span class="badge">${escapeHtml(resume.status || 'draft')}</span>
      <span class="muted">${resume.updated ? escapeHtml(resume.updated) : ''}</span>
      <div class="row-actions">
        <a class="secondary" href="#/edit/${encodeURIComponent(resume.slug)}">Edit</a>
        <a class="secondary" href="#/preview/${encodeURIComponent(resume.slug)}">Preview</a>
        <button class="secondary" data-action="print" data-slug="${escapeHtml(resume.slug)}">Print / PDF</button>
        <button class="danger" data-action="delete" data-slug="${escapeHtml(resume.slug)}">Delete</button>
      </div>
    </li>
  `;
}

async function renderSkills(): Promise<void> {
  editorState = null;
  saveStatus.hidden = true;
  const { skills } = await api<{ skills: Skill[] }>('skills');

  app.innerHTML = `
    <section class="hero">
      <div>
        <p class="eyebrow">Agent</p>
        <h1>Skills</h1>
        <p class="lede">Built-in skills ship with the app. Custom skills live in this workspace. Agents load them through the MCP server.</p>
      </div>
      <button class="primary" id="new-skill">New skill</button>
    </section>
    <ul class="resume-list">
      ${skills
        .map(
          (skill) => `
        <li class="resume-row">
          <a class="resume-title" href="#/skills/${encodeURIComponent(skill.slug)}">${escapeHtml(skill.name)}</a>
          <span class="badge">${escapeHtml(skill.source)}${skill.enabled === false ? ' · off' : ''}</span>
          <span class="muted">${escapeHtml(skill.description || '')}</span>
          <div class="row-actions">
            <button class="secondary" data-action="toggle-skill" data-slug="${escapeHtml(skill.slug)}" data-enabled="${skill.enabled !== false}">${skill.enabled === false ? 'Enable' : 'Disable'}</button>
            <a class="secondary" href="#/skills/${encodeURIComponent(skill.slug)}">Open</a>
            ${skill.source === 'user' ? `<button class="danger" data-action="delete-skill" data-slug="${escapeHtml(skill.slug)}">Delete</button>` : ''}
          </div>
        </li>
      `,
        )
        .join('')}
    </ul>
  `;

  app.querySelector<HTMLButtonElement>('#new-skill')!.addEventListener('click', () => {
    skillName.value = '';
    skillDialog.showModal();
    skillName.focus();
  });

  app.querySelector<HTMLElement>('.resume-list')!.addEventListener('click', async (event) => {
    const target = event.target as Element | null;
    const toggle = target?.closest<HTMLButtonElement>('[data-action=toggle-skill]');
    if (toggle && toggle.dataset.slug) {
      const enabled = toggle.dataset.enabled !== 'true';
      await api(
        'skill/enabled',
        withSlug(toggle.dataset.slug, {
          method: 'PUT',
          body: JSON.stringify({ enabled }),
        }),
      );
      await renderSkills();
      return;
    }
    const remove = target?.closest<HTMLButtonElement>('[data-action=delete-skill]');
    if (!remove || !remove.dataset.slug) return;
    if (!confirm(`Delete skill ${remove.dataset.slug}?`)) return;
    await api('skill', withSlug(remove.dataset.slug, { method: 'DELETE' }));
    await renderSkills();
  });
}

async function renderSkillEditor(slug: string): Promise<void> {
  const skill = await api<Skill>('skill', withSlug(slug));
  const skillEditorState = {
    kind: 'skill' as const,
    slug,
    markdown: skill.markdown,
    source: skill.source,
  };
  editorState = skillEditorState;
  saveStatus.hidden = skill.source === 'builtin';

  app.innerHTML = `
    <section class="doc">
      <div class="doc-toolbar">
        <a class="ghost" href="#/skills">← Skills</a>
        <span class="badge">${escapeHtml(skill.source)}${skill.enabled === false ? ' · off' : ''}</span>
        <button class="secondary" id="toggle-skill">${skill.enabled === false ? 'Enable' : 'Disable'}</button>
      </div>
      <textarea class="md-editor" id="markdown" ${skill.source === 'builtin' ? 'readonly' : ''} spellcheck="true">${escapeHtml(skill.markdown)}</textarea>
    </section>
  `;

  const editor = app.querySelector<HTMLTextAreaElement>('#markdown')!;
  if (skill.source !== 'builtin') {
    editor.addEventListener('input', () => {
      skillEditorState.markdown = editor.value;
      queueSave(async () => {
        await api(
          'skill',
          withSlug(slug, {
            method: 'PUT',
            body: JSON.stringify({ markdown: skillEditorState.markdown }),
          }),
        );
        saveStatus.textContent = 'Saved';
      });
    });
  }
  editor.focus();

  app.querySelector<HTMLButtonElement>('#toggle-skill')!.addEventListener('click', async () => {
    await api(
      'skill/enabled',
      withSlug(slug, {
        method: 'PUT',
        body: JSON.stringify({ enabled: skill.enabled === false }),
      }),
    );
    await renderSkillEditor(slug);
  });
}

async function renderMemory(): Promise<void> {
  const memory = await api<Memory>('memory');
  const memoryEditorState = { kind: 'memory' as const, markdown: memory.markdown };
  editorState = memoryEditorState;
  saveStatus.hidden = false;

  app.innerHTML = `
    <section class="doc">
      <textarea class="md-editor" id="markdown" spellcheck="true" aria-label="Memory Markdown">${escapeHtml(memory.markdown)}</textarea>
    </section>
  `;

  const editor = app.querySelector<HTMLTextAreaElement>('#markdown')!;
  editor.addEventListener('input', () => {
    memoryEditorState.markdown = editor.value;
    queueSave(async () => {
      await api('memory', {
        method: 'PUT',
        body: JSON.stringify({ markdown: memoryEditorState.markdown }),
      });
      saveStatus.textContent = 'Saved';
    });
  });
  editor.focus();
}

async function renderVault(): Promise<void> {
  editorState = null;
  saveStatus.hidden = true;
  const workspace = await api<WorkspaceInfo>('workspace');
  const recent = (workspace.recentVaults || []).filter((item) => item && item !== workspace.root);

  app.innerHTML = `
    <section class="doc">
      <form id="vault-form" class="empty-state">
        <h2>Vault</h2>
        <p class="muted">Your resumes live in a vault folder, not in the app install. The default vault is used when you run <code>resuflow</code> without <code>--dir</code>.</p>
        <p><strong>Current:</strong> <code>${escapeHtml(workspace.root)}</code></p>
        <p><strong>Default:</strong> <code>${escapeHtml(workspace.defaultVault || workspace.root)}</code></p>
        <label class="field"><span>Vault path</span><input name="root" value="${escapeHtml(workspace.root)}" required /></label>
        <div class="actions">
          <button class="primary" type="submit" data-mode="switch">Switch vault</button>
          <button class="secondary" type="submit" data-mode="init">Create vault here</button>
        </div>
        ${
          recent.length
            ? `<h3>Recent</h3><ul class="resume-list">${recent
                .map(
                  (item) => `
          <li class="resume-row">
            <span class="resume-title">${escapeHtml(item)}</span>
            <div class="row-actions">
              <button type="button" class="secondary" data-recent="${escapeHtml(item)}">Use</button>
            </div>
          </li>
        `,
                )
                .join('')}</ul>`
            : ''
        }
      </form>
    </section>
  `;

  const form = app.querySelector<HTMLFormElement>('#vault-form')!;
  const input = form.querySelector<HTMLInputElement>('input[name=root]')!;

  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const submitter = event.submitter as HTMLElement | null;
    const mode = submitter?.dataset.mode || 'switch';
    try {
      await api('workspace', {
        method: 'PUT',
        body: JSON.stringify({
          root: input.value.trim(),
          init: mode === 'init',
          setDefault: true,
        }),
      });
      location.hash = '#/';
      await render();
    } catch (error) {
      saveStatus.hidden = false;
      saveStatus.textContent = error instanceof Error ? error.message : String(error);
    }
  });

  form.querySelectorAll<HTMLButtonElement>('[data-recent]').forEach((button) => {
    button.addEventListener('click', () => {
      input.value = button.dataset.recent || '';
      form.requestSubmit(form.querySelector<HTMLButtonElement>('[data-mode=switch]'));
    });
  });
}

async function renderDocs(): Promise<void> {
  editorState = null;
  saveStatus.hidden = true;
  const info = await api<McpInfo>('mcp');
  const snippet = JSON.stringify(info.config, null, 2);

  app.innerHTML = `
    <article class="docs-page">
      <header class="docs-hero">
        <p class="eyebrow">Guide</p>
        <h1>Read docs</h1>
        <p class="lede">How this local resume app works: data, Markdown, skills, memory, and connecting your own agents over MCP.</p>
      </header>
      <nav class="docs-toc">
        <a href="#/docs/overview">Overview</a>
        <a href="#/docs/install">Install and run</a>
        <a href="#/docs/workspace">Workspace</a>
        <a href="#/docs/resumes">Resumes</a>
        <a href="#/docs/profile">Profile</a>
        <a href="#/docs/skills">Skills</a>
        <a href="#/docs/memory">Memory</a>
        <a href="#/docs/mcp">MCP</a>
      </nav>

      <section class="docs-section" id="docs-overview">
        <h2>Overview</h2>
        <p>ResuFlow is a native Rust + Tauri desktop application. It does not run an LLM. You write Markdown in the UI, or you point Grok, Cursor, Codex, Gemini, or another agent at this process over MCP.</p>
        <p>The product (this install) is separate from your data. Resumes live in a workspace folder under Documents; first launch creates a default vault automatically.</p>
      </section>

      <section class="docs-section" id="docs-install">
        <h2>Install and run</h2>
        <p>Windows users can download the installer from the product website. On Arch Linux, install the pacman package straight from its release URL, or download it first and install from the local file — both forms also upgrade an existing install.</p>
        <pre>Windows: download and run the .exe installer
Linux (Arch): sudo pacman -U https://github.com/exolithelabs/resuflow/releases/latest/download/resuflow-linux-x86_64.pkg.tar.zst
Linux (Arch, local file): cd ~/Downloads && sudo pacman -U ./resuflow-linux-x86_64.pkg.tar.zst</pre>
        <ul>
          <li><code>resuflow</code> opens the desktop window by default.</li>
          <li><code>resuflow --browser</code> opens the same UI in a web browser.</li>
          <li><code>resuflow serve</code> starts the server only.</li>
          <li>On Windows, download updates from the product website. On Arch Linux, re-run <code>sudo pacman -U</code> with the newer package URL or the locally downloaded package file.</li>
          <li>Removing the app does not remove your workspace.</li>
        </ul>
        <p>The UI is at <code>http://127.0.0.1:4173/</code> while the app is running.</p>
      </section>

      <section class="docs-section" id="docs-workspace">
        <h2>Workspace</h2>
        <p>Open <strong>Vault</strong> from the settings gear to see the path or change the default vault. Typical layout:</p>
        <pre>resuflow.json
profile.md
memory.md
skills/disabled.json
skills/&lt;slug&gt;/SKILL.md
resumes/&lt;slug&gt;/resume.md</pre>
        <p>Do not put personal resume data in the app install. Git in the vault is optional and not created automatically.</p>
      </section>

      <section class="docs-section" id="docs-resumes">
        <h2>Resumes</h2>
        <p>The home page is a list. Create a resume, then use <strong>Edit</strong> and <strong>Preview</strong>.</p>
        <ul>
          <li>Edit is the Markdown file, including YAML frontmatter (<code>title</code>, <code>status</code>, <code>tags</code>).</li>
          <li>Preview renders the designed A4 page.</li>
          <li>Print / PDF opens the native print dialog to save a clean A4 PDF.</li>
          <li>Changes autosave.</li>
        </ul>
        <p>The header <strong>← All resumes</strong> link appears on resume, profile, skills, memory, and docs pages — not on the home list.</p>
      </section>

      <section class="docs-section" id="docs-profile">
        <h2>Profile</h2>
        <p>Shared identity is <code>profile.md</code>. Open it from the settings gear. It is edit-only Markdown (name, email, and links in the frontmatter). Preview uses this header on every resume.</p>
      </section>

      <section class="docs-section" id="docs-skills">
        <h2>Skills</h2>
        <p>Skills are Markdown instructions agents can follow. Built-in skills ship with the app (research, impact writer, ATS optimizer, critic). You can create your own; those files live in the workspace.</p>
        <ul>
          <li>Enable or disable any skill. Disabled skills stay in the list but are hidden from MCP <code>list_skills</code>.</li>
          <li>Built-in skills cannot be edited or deleted. Duplicate the idea as a custom skill if you need to change the text.</li>
        </ul>
      </section>

      <section class="docs-section" id="docs-memory">
        <h2>Memory</h2>
        <p><code>memory.md</code> is durable notes for agents: facts, target roles, tone. Edit it from the settings gear. Agents can read and append through MCP (<code>get_memory</code>, <code>write_memory</code>, <code>append_memory</code>).</p>
      </section>

      <section class="docs-section" id="docs-mcp">
        <h2>MCP</h2>
        <p>While the app is running it hosts an MCP server in this process. Point your agent at:</p>
        <p><code>${escapeHtml(info.url)}</code></p>
        <p>Example config:</p>
        <pre>${escapeHtml(snippet)}</pre>
        <p>This configuration contains a private per-process bearer token. Do not publish or commit it; copy a fresh configuration after restarting ResuFlow.</p>
        <p>Tools include list/get/write for resumes, profile, skills, and memory, plus create resume and enable-aware skill listing.</p>
      </section>
    </article>
  `;

  const hash = location.hash.replace(/^#/, '');
  const key = hash.replace(/^\/docs\/?/, '');
  const target = document.getElementById(key ? `docs-${key}` : 'docs-overview');
  if (target && key) target.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

async function renderProfileEditor(): Promise<void> {
  if (!editorState || editorState.kind !== 'profile') {
    const profile = await api<Profile>('profile');
    editorState = { kind: 'profile', markdown: profile.markdown };
  }
  const profileState = editorState;
  saveStatus.hidden = false;

  app.innerHTML = `
    <section class="doc">
      <textarea class="md-editor" id="markdown" spellcheck="true" aria-label="Profile Markdown">${escapeHtml(profileState.markdown)}</textarea>
    </section>
  `;

  const editor = app.querySelector<HTMLTextAreaElement>('#markdown');
  if (editor) {
    editor.addEventListener('input', () => {
      profileState.markdown = editor.value;
      queueSave(saveProfileEditor);
    });
    editor.focus();
  }
}

async function saveProfileEditor(): Promise<void> {
  if (!editorState || editorState.kind !== 'profile') return;
  saveStatus.textContent = 'Saving…';
  const saved = await api<Profile>('profile', {
    method: 'PUT',
    body: JSON.stringify({ markdown: editorState.markdown }),
  });
  if (editorState && editorState.kind === 'profile') {
    editorState.markdown = saved.markdown;
  }
  saveStatus.textContent = 'Saved';
}

async function renderEditor(slug: string, mode: 'edit' | 'preview'): Promise<void> {
  if (editorState?.kind === 'resume' && editorState.resume.slug === slug) {
    if (mode === 'preview') await saveEditor();
  } else {
    const resume = await api<Resume>('resume', withSlug(slug));
    editorState = { kind: 'resume', resume };
  }
  paintEditor(mode);
}

function paintEditor(mode: 'edit' | 'preview'): void {
  if (!editorState || editorState.kind !== 'resume') return;
  const resumeState = editorState;
  const { resume } = resumeState;
  const slug = resume.slug;
  saveStatus.hidden = false;

  app.innerHTML = `
    <section class="doc">
      <div class="doc-toolbar">
        <div class="mode-switch" role="tablist">
          <a class="mode${mode === 'edit' ? ' active' : ''}" href="#/edit/${encodeURIComponent(slug)}">Edit</a>
          <a class="mode${mode === 'preview' ? ' active' : ''}" href="#/preview/${encodeURIComponent(slug)}">Preview</a>
        </div>
        <button type="button" class="secondary" id="print-resume">Print / PDF</button>
      </div>
      ${
        mode === 'edit'
          ? `
        <textarea class="md-editor" id="markdown" spellcheck="true" aria-label="Resume Markdown">${escapeHtml(resume.markdown)}</textarea>
      `
          : `
        <iframe class="preview-frame" id="preview" title="Resume preview" src="/preview/${encodeURIComponent(slug)}?t=${Date.now()}"></iframe>
      `
      }
    </section>
  `;

  const editor = app.querySelector<HTMLTextAreaElement>('#markdown');
  if (editor) {
    editor.addEventListener('input', () => {
      resumeState.resume.markdown = editor.value;
      queueSave(saveEditor);
    });
    editor.focus();
  }

  app.querySelector<HTMLButtonElement>('#print-resume')!.addEventListener('click', async () => {
    await saveEditor();
    printResume(slug);
  });
}

function printResume(slug: string): void {
  const existingFrame = app.querySelector<HTMLIFrameElement>('#preview');
  if (existingFrame?.contentWindow) {
    existingFrame.contentWindow.focus();
    existingFrame.contentWindow.print();
    return;
  }

  const frame = document.createElement('iframe');
  frame.style.position = 'fixed';
  frame.style.width = '0';
  frame.style.height = '0';
  frame.style.border = '0';
  frame.src = `/preview/${encodeURIComponent(slug)}?t=${Date.now()}`;
  frame.addEventListener('load', () => {
    frame.contentWindow?.focus();
    frame.contentWindow?.print();
    window.setTimeout(() => frame.remove(), 1000);
  });
  document.body.appendChild(frame);
}

async function saveEditor(): Promise<void> {
  if (!editorState || editorState.kind !== 'resume') return;
  saveStatus.textContent = 'Saving…';
  const saved = await api<Resume>(
    'resume',
    withSlug(editorState.resume.slug, {
      method: 'PUT',
      body: JSON.stringify({ markdown: editorState.resume.markdown }),
    }),
  );
  if (editorState && editorState.kind === 'resume') {
    editorState.resume = saved;
  }
  saveStatus.textContent = 'Saved';
}

function queueSave(fn: () => Promise<void>): void {
  saveStatus.hidden = false;
  saveStatus.textContent = 'Saving…';
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    fn().catch((error: unknown) => {
      saveStatus.textContent = error instanceof Error ? error.message : String(error);
    });
  }, 400);
}

function escapeHtml(value: unknown): string {
  return String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
