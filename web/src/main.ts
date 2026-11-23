import { api, list, str, STATUSES, type FieldValue, type Parts, type Report, type ResumeSummary, type Skill, type Status } from "./api";
import { createLiveEditor } from "./livemd";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const el = {
  list: $("resume-list"),
  filter: $<HTMLInputElement>("filter"),
  title: $<HTMLInputElement>("title"),
  status: $<HTMLSelectElement>("status"),
  tags: $<HTMLInputElement>("tags"),
  header: $("resume-header"),
  pageScroll: $("page-scroll"),
  checks: $<HTMLDetailsElement>("checks"),
  checksSummary: $("checks-summary"),
  findings: $("findings"),
  banner: $("banner"),
  saveState: $("save-state"),
  stats: $("stats"),
  empty: $("empty"),
  rowMenu: $("row-menu"),
};

const state = {
  slug: null as string | null,
  hash: "",
  bodyOffset: 0,
  fields: {} as Parts["fields"],
  profile: {} as Parts["fields"],
  dirty: false,
  saving: null as Promise<void> | null,
  saveTimer: 0,
  revision: "",
  resumes: [] as ResumeSummary[],
  conflict: false,
};

const fileId = () => `resume:${state.slug}`;

// ================================================================ sidebar

async function refreshList() {
  state.resumes = await api.listResumes();
  renderList();
}

function renderList() {
  const q = el.filter.value.trim().toLowerCase();
  const shown = state.resumes.filter(
    (r) => !q || r.title.toLowerCase().includes(q) || r.tags.some((t) => t.toLowerCase().includes(q)),
  );
  el.list.replaceChildren(...shown.map(row));
  if (!shown.length) {
    const p = document.createElement("p");
    p.className = "muted small pad";
    p.textContent = state.resumes.length ? "No matches." : "No resumes yet.";
    el.list.append(p);
  }
}

function row(r: ResumeSummary) {
  const wrap = document.createElement("div");
  wrap.className = "row" + (r.slug === state.slug ? " active" : "");

  const b = document.createElement("button");
  b.className = "item";
  b.title = r.slug;
  b.onclick = () => openResume(r.slug);
  const dot = document.createElement("span");
  dot.className = `dot ${r.status}`;
  dot.title = r.status;
  const label = document.createElement("span");
  label.className = "item-label";
  label.textContent = r.title;
  b.append(dot, label);
  if (r.tags.length) {
    const t = document.createElement("span");
    t.className = "item-tags";
    t.textContent = r.tags.join(" · ");
    b.append(t);
  }

  const more = document.createElement("button");
  more.className = "more";
  more.title = "More";
  more.setAttribute("aria-label", `More actions for ${r.title}`);
  more.textContent = "⋯";
  more.onclick = (e) => {
    e.stopPropagation();
    openRowMenu(r, more);
  };

  wrap.append(b, more);
  return wrap;
}

el.filter.addEventListener("input", renderList);

// ---------------------------------------------------------------- row menu

let menuFor: ResumeSummary | null = null;

function openRowMenu(r: ResumeSummary, anchor: HTMLElement) {
  if (menuFor?.slug === r.slug && !el.rowMenu.hidden) return closeRowMenu();
  closeRowMenu();
  menuFor = r;
  const box = anchor.getBoundingClientRect();
  el.rowMenu.style.top = `${box.bottom + 4}px`;
  el.rowMenu.style.left = `${box.right - 150}px`;
  el.rowMenu.hidden = false;
  anchor.closest(".row")?.classList.add("menu-open");
}

function closeRowMenu() {
  el.rowMenu.hidden = true;
  menuFor = null;
  document.querySelectorAll(".row.menu-open").forEach((n) => n.classList.remove("menu-open"));
}

document.addEventListener("click", (e) => {
  if (!el.rowMenu.hidden && !el.rowMenu.contains(e.target as Node)) closeRowMenu();
});
document.addEventListener("keydown", (e) => e.key === "Escape" && closeRowMenu());
$("sidebar").addEventListener("scroll", closeRowMenu);

$("row-delete").onclick = async () => {
  const r = menuFor;
  closeRowMenu();
  if (!r || !(await confirmTrash(r.title))) return;
  if (r.slug === state.slug) {
    clearTimeout(state.saveTimer);
    state.dirty = false;
  }
  await api.deleteResume(r.slug);
  await refreshList();
  if (r.slug === state.slug) await openResume(state.resumes[0]?.slug ?? null);
};

const dlgConfirm = $<HTMLDialogElement>("dlg-confirm");
function confirmTrash(title: string): Promise<boolean> {
  $("confirm-title").textContent = `Delete "${title}"?`;
  $("confirm-text").textContent =
    "It moves to the workspace's .trash folder. Nothing is erased, so you can restore it from there.";
  dlgConfirm.showModal();
  return new Promise((res) =>
    dlgConfirm.addEventListener("close", () => res(dlgConfirm.returnValue === "ok"), { once: true }),
  );
}

// ================================================================ resume page (single live-preview pane)

const editor = createLiveEditor({
  parent: $("editor-host"),
  placeholder: "Start writing. Use ## for a section, ### for a role, - for a bullet.",
  onChange: () => {
    state.dirty = true;
    setSaveState("Editing…");
    updateStats();
    clearTimeout(state.saveTimer);
    state.saveTimer = window.setTimeout(() => void save(), 450);
  },
});

async function openResume(slug: string | null) {
  await flush();
  state.slug = slug;
  hideBanner();
  el.empty.hidden = !!slug;
  renderList();
  if (!slug) return;
  await load();
  el.pageScroll.scrollTop = 0;
  editor.focus();
}

function applyParts(p: Parts) {
  state.hash = p.hash;
  state.bodyOffset = p.body_offset;
  state.fields = p.fields;
  el.title.value = str(p.fields.title) || state.slug || "";
  el.status.value = str(p.fields.status) || "draft";
  el.tags.value = list(p.fields.tags).join(", ");
  renderHeader();
}

async function load(keepPlace = false) {
  if (!state.slug) return;
  const p = await api.readParts(fileId());
  applyParts(p);
  const scroll = el.pageScroll.scrollTop;
  editor.set(p.body, keepPlace);
  if (keepPlace) el.pageScroll.scrollTop = scroll;
  state.dirty = false;
  setSaveState("Saved");
  updateStats();
  await refreshChecks();
}

async function save(force = false) {
  const slug = state.slug;
  if (!slug || (!state.dirty && !force) || state.conflict) return;
  clearTimeout(state.saveTimer);
  if (state.saving) await state.saving;
  const body = editor.get();
  state.saving = (async () => {
    try {
      setSaveState("Saving…");
      const p = await api.writeBody(`resume:${slug}`, body, force ? undefined : state.hash);
      if (slug !== state.slug) return;
      applyParts(p);
      if (editor.get() === body) state.dirty = false;
      setSaveState(state.dirty ? "Editing…" : "Saved");
      state.revision = await api.revision();
      await refreshChecks();
      void refreshList();
    } catch (e) {
      const msg = String(e);
      if (msg.startsWith("conflict")) showConflict();
      else setSaveState(`Not saved: ${msg}`, true);
    } finally {
      state.saving = null;
    }
  })();
  await state.saving;
}

async function flush() {
  if (state.dirty) await save();
  else if (state.saving) await state.saving;
}

function setSaveState(text: string, error = false) {
  el.saveState.textContent = text;
  el.saveState.classList.toggle("error", error);
}

function updateStats() {
  const words = editor.get().split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w)).length;
  el.stats.textContent = `${words} words`;
}

// ---------------------------------------------------------------- header from profile

function renderHeader() {
  const pick = (k: string) => str(state.fields[k]) || str(state.profile[k]);
  const name = pick("name");
  const headline = pick("headline");
  const links = state.fields.links !== undefined ? list(state.fields.links) : list(state.profile.links);
  const contacts = [pick("location"), pick("email"), pick("phone"), ...links].filter(Boolean);

  const parts: HTMLElement[] = [];
  const h1 = document.createElement("h1");
  h1.textContent = name || "Your name";
  h1.classList.toggle("placeholder", !name);
  parts.push(h1);
  if (headline) {
    const p = document.createElement("p");
    p.className = "headline";
    p.textContent = headline;
    parts.push(p);
  }
  const c = document.createElement("p");
  c.className = "contact";
  c.textContent = contacts.length ? contacts.join("  ·  ") : "Add your contact details in Profile";
  c.classList.toggle("placeholder", !contacts.length);
  parts.push(c);
  el.header.replaceChildren(...parts);
}

async function refreshProfile() {
  state.profile = (await api.readParts("profile")).fields;
  renderHeader();
}

el.header.onclick = () => void hub.open({ kind: "profile" });

// ---------------------------------------------------------------- conflicts & live reload

function showConflict() {
  state.conflict = true;
  setSaveState("Changed on disk", true);
  el.banner.hidden = false;
  const msg = document.createElement("span");
  msg.textContent = "This resume was changed outside the editor (an agent or another app) while you were typing.";
  el.banner.replaceChildren(
    msg,
    button("Load their version", async () => {
      hideBanner();
      await load(true);
    }),
    button("Keep mine", async () => {
      hideBanner();
      await save(true);
    }),
  );
}

function hideBanner() {
  state.conflict = false;
  el.banner.hidden = true;
}

function button(label: string, onClick: () => void) {
  const b = document.createElement("button");
  b.className = "btn ghost";
  b.type = "button";
  b.textContent = label;
  b.onclick = onClick;
  return b;
}

async function poll() {
  if (document.hidden) return;
  try {
    const rev = await api.revision();
    if (rev === state.revision) return;
    state.revision = rev;
    await Promise.all([refreshList(), refreshProfile()]);
    await hub.onDiskChange();
    if (!state.slug || state.saving) return;
    const disk = await api.readParts(fileId()).catch(() => null);
    if (!disk) return void openResume(state.resumes[0]?.slug ?? null); // deleted elsewhere
    if (disk.hash === state.hash) return void refreshChecks();
    if (state.dirty) showConflict();
    else {
      await load(true);
      setSaveState("Updated from disk");
      setTimeout(() => !state.dirty && setSaveState("Saved"), 2500);
    }
  } catch {
    /* transient; next tick retries */
  }
}

// ---------------------------------------------------------------- checks

async function refreshChecks() {
  if (!state.slug) return;
  renderChecks(await api.checkResume(state.slug));
}

function renderChecks(r: Report) {
  const errors = r.findings.filter((f) => f.level === "error").length;
  const warns = r.findings.filter((f) => f.level === "warn").length;
  const issues = [errors && `${errors} error${errors > 1 ? "s" : ""}`, warns && `${warns} warning${warns > 1 ? "s" : ""}`]
    .filter(Boolean)
    .join(" ");
  el.checksSummary.textContent =
    `Checks · ${r.bullets_with_numbers}/${r.bullets} bullets quantified · ~${r.est_pages.toFixed(1)} pages · ` +
    (issues || "all clear");
  el.checks.classList.toggle("has-errors", errors > 0);
  el.checks.classList.toggle("has-warns", !errors && warns > 0);
  el.findings.replaceChildren(
    ...r.findings.map((f) => {
      const li = document.createElement("li");
      li.className = f.level;
      li.textContent = f.message;
      const bodyLine = f.line ? f.line - state.bodyOffset : 0;
      if (bodyLine > 0) {
        const a = document.createElement("button");
        a.className = "line-link";
        a.textContent = `line ${bodyLine}`;
        a.onclick = () => editor.gotoLine(bodyLine);
        li.prepend(a);
      }
      return li;
    }),
  );
}

// ---------------------------------------------------------------- metadata

async function updateMeta(meta: { title?: string; status?: Status; tags?: string[] }) {
  if (!state.slug) return;
  await flush();
  try {
    applyParts(await api.setMeta(state.slug, meta));
    state.revision = await api.revision();
    await refreshList();
  } catch (e) {
    setSaveState(String(e), true);
  }
}

el.status.replaceChildren(...STATUSES.map((s) => new Option(s[0].toUpperCase() + s.slice(1), s)));
el.status.addEventListener("change", () => updateMeta({ status: el.status.value as Status }));
el.title.addEventListener("change", () => el.title.value.trim() && updateMeta({ title: el.title.value.trim() }));
el.tags.addEventListener("change", () => updateMeta({ tags: list(el.tags.value) }));
for (const input of [el.title, el.tags]) {
  input.addEventListener("keydown", (e) => e.key === "Enter" && input.blur());
}

// ---------------------------------------------------------------- new & print

const dlgNew = $<HTMLDialogElement>("dlg-new");
const newTitle = dlgNew.querySelector<HTMLInputElement>("input[name=title]")!;
function newResume() {
  newTitle.value = "";
  dlgNew.showModal();
  newTitle.focus();
}
dlgNew.addEventListener("close", async () => {
  const title = newTitle.value.trim();
  if (dlgNew.returnValue !== "ok" || !title) return;
  const slug = await api.createResume(title);
  await refreshList();
  await openResume(slug);
});
$("new-resume").onclick = newResume;
$("empty-new").onclick = newResume;
$("print").onclick = printCurrent;
$("save-pdf").onclick = savePdf;

async function printCurrent() {
  if (!state.slug) return;
  await flush();
  await api.printResume(state.slug).catch((e) => setSaveState(String(e), true));
}

async function savePdf() {
  if (!state.slug) return;
  await flush();
  try {
    setSaveState("Saving PDF…");
    const path = await api.savePdf(state.slug);
    setSaveState(path ? `PDF saved to ${path}` : "Saved");
  } catch (e) {
    setSaveState(`PDF not saved: ${e}`, true);
  }
}

// ================================================================ Agent panel

type Tab = { kind: "profile" | "memory" | "connect" } | { kind: "skill"; name: string };

const hub = (() => {
  const dlg = $<HTMLDialogElement>("dlg-hub");
  const props = $<HTMLFormElement>("hub-props");
  const saveEl = $("hub-save");
  const host = $("hub-editor-host");
  const s = { tab: { kind: "profile" } as Tab, hash: "", dirty: false, timer: 0, conflict: false, skills: [] as Skill[] };

  const ed = createLiveEditor({
    parent: host,
    onChange: () => {
      s.dirty = true;
      status("Editing…");
      clearTimeout(s.timer);
      s.timer = window.setTimeout(() => void save(), 450);
    },
  });

  const TEXT = {
    profile: ["Profile", "Who you are on every resume, plus the private notes your agent writes from."],
    memory: ["Memory", "Durable notes your agents keep between sessions: target roles, preferences, decisions."],
    connect: ["Connect an agent", "Let your AI agent read and edit your resumes."],
  } as const;

  const key = (t: Tab) => (t.kind === "skill" ? `skill:${t.name}` : t.kind);
  const editable = (t: Tab): t is { kind: "profile" | "memory" } => t.kind === "profile" || t.kind === "memory";

  function status(text: string, error = false) {
    saveEl.textContent = text;
    saveEl.classList.toggle("error", error);
  }

  async function open(tab: Tab = s.tab) {
    await flush();
    if (!s.skills.length) s.skills = await api.listSkills();
    $("hub-skills").replaceChildren(
      ...s.skills.map((sk) => {
        const b = document.createElement("button");
        b.className = "hub-tab";
        b.dataset.tab = `skill:${sk.name}`;
        b.title = sk.description;
        b.textContent = sk.name;
        return b;
      }),
    );
    if (!dlg.open) dlg.showModal();
    await select(tab);
  }

  async function select(tab: Tab) {
    await save();
    s.tab = tab;
    s.conflict = false;
    s.dirty = false;
    dlg.querySelectorAll<HTMLElement>(".hub-tab").forEach((b) => b.classList.toggle("active", b.dataset.tab === key(tab)));

    const skill = tab.kind === "skill" ? s.skills.find((x) => x.name === tab.name) : undefined;
    const [title, desc] = tab.kind === "skill" ? [tab.name, skill?.description ?? ""] : TEXT[tab.kind];
    $("hub-title").textContent = title;
    $("hub-desc").textContent = desc;
    status("");

    props.hidden = tab.kind !== "profile";
    $("hub-notes-label").hidden = tab.kind !== "profile";
    $("hub-skill-bar").hidden = tab.kind !== "skill";
    host.hidden = tab.kind === "connect";
    $("hub-connect").hidden = tab.kind !== "connect";

    if (tab.kind === "connect") {
      const info = await api.info();
      $("cc-cmd").textContent = `claude mcp add resuflow -- ${shellQuote(info.mcp_command.replace(/ mcp$/, ""))} mcp`;
      $("mcp-json").textContent = JSON.stringify(info.mcp_config, null, 2);
      $("hub-root").textContent = info.root;
      return;
    }
    if (tab.kind === "skill") {
      ed.setReadOnly(true);
      ed.set((skill?.content ?? "").replace(/^---\n[\s\S]*?\n---\n+/, ""));
      return;
    }
    ed.setReadOnly(false);
    await reload(false);
    if (tab.kind === "memory") ed.focus();
  }

  function fillProps(fields: Parts["fields"]) {
    for (const input of props.querySelectorAll("input")) {
      if (document.activeElement !== input) input.value = str(fields[input.name]);
    }
  }

  async function reload(keepPlace = true) {
    const t = s.tab;
    if (!editable(t)) return;
    const p = await api.readParts(t.kind);
    s.hash = p.hash;
    s.dirty = false;
    s.conflict = false;
    if (t.kind === "profile") fillProps(p.fields);
    ed.set(p.body, keepPlace);
    status("Saved");
  }

  function conflict() {
    s.conflict = true;
    saveEl.classList.add("error");
    saveEl.replaceChildren("Changed on disk · ", button("Load their version", () => void reload()));
  }

  async function save() {
    clearTimeout(s.timer);
    const t = s.tab;
    if (!s.dirty || s.conflict || !editable(t)) return;
    const body = ed.get();
    try {
      status("Saving…");
      const p = await api.writeBody(t.kind, body, s.hash);
      s.hash = p.hash;
      if (ed.get() === body) s.dirty = false;
      status(s.dirty ? "Editing…" : "Saved");
      state.revision = await api.revision();
    } catch (e) {
      if (String(e).startsWith("conflict")) conflict();
      else status(`Not saved: ${e}`, true);
    }
  }

  // Profile properties save on change; the header updates right away.
  props.addEventListener("change", async (e) => {
    const input = e.target as HTMLInputElement;
    await save();
    if (s.conflict) return;
    const value: FieldValue = input.name === "links" ? list(input.value) : input.value;
    try {
      status("Saving…");
      const p = await api.setFields("profile", { [input.name]: value }, s.hash);
      s.hash = p.hash;
      state.profile = p.fields;
      renderHeader();
      status("Saved");
      state.revision = await api.revision();
    } catch (err) {
      if (String(err).startsWith("conflict")) conflict();
      else status(`Not saved: ${err}`, true);
    }
  });
  props.addEventListener("submit", (e) => e.preventDefault());

  async function onDiskChange() {
    const t = s.tab;
    if (!dlg.open || !editable(t)) return;
    const p = await api.readParts(t.kind).catch(() => null);
    if (!p || p.hash === s.hash) return;
    if (s.dirty) conflict();
    else await reload();
  }

  dlg.addEventListener("click", async (e) => {
    const target = e.target as HTMLElement;
    if (target === dlg) return void close(); // backdrop
    const tab = target.closest<HTMLElement>(".hub-tab")?.dataset.tab;
    if (tab) {
      const [kind, name] = tab.split(":");
      return void select(kind === "skill" ? { kind, name } : ({ kind } as Tab));
    }
    const copy = target.closest<HTMLButtonElement>("[data-copy]");
    if (copy) return void copyText(copy, $(copy.dataset.copy!).textContent ?? "");
    const install = target.closest<HTMLButtonElement>("[data-install]");
    if (install) {
      try {
        const dir = await api.installSkills(install.dataset.install as "claude" | "agents");
        status(`Installed ${s.skills.length} skills in ${dir}`);
      } catch (err) {
        status(String(err), true);
      }
    }
  });

  $("skill-copy").onclick = (e) => {
    const t = s.tab;
    const skill = t.kind === "skill" ? s.skills.find((x) => x.name === t.name) : undefined;
    if (skill) void copyText(e.currentTarget as HTMLButtonElement, skill.content);
  };

  async function close() {
    await save();
    dlg.close();
  }
  dlg.addEventListener("cancel", (e) => {
    e.preventDefault();
    void close();
  });
  $("hub-close").onclick = () => void close();

  return { open, onDiskChange, save, isOpen: () => dlg.open };
})();

$("open-hub").onclick = () => void hub.open();

async function copyText(btn: HTMLButtonElement, text: string) {
  await navigator.clipboard.writeText(text);
  const label = btn.textContent;
  btn.textContent = "Copied";
  setTimeout(() => (btn.textContent = label), 1500);
}

function shellQuote(s: string) {
  return /^[\w@%+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`;
}

// ================================================================ keys & boot

document.addEventListener("keydown", (e) => {
  if (!(e.ctrlKey || e.metaKey)) return;
  const k = e.key.toLowerCase();
  if (k === "s" && e.shiftKey && !hub.isOpen()) {
    e.preventDefault();
    void savePdf();
  } else if (k === "s") {
    e.preventDefault();
    void (hub.isOpen() ? hub.save() : save());
  } else if (k === "p" && !hub.isOpen()) {
    e.preventDefault();
    void printCurrent();
  } else if (k === "n" && !hub.isOpen()) {
    e.preventDefault();
    newResume();
  }
});

window.addEventListener("beforeunload", () => void flush());

async function boot() {
  const info = await api.info();
  // Without direct export, Print (and its "Save as PDF" option) is the way to a PDF.
  if (!info.pdf_export) {
    $("save-pdf").hidden = true;
    $("print").classList.replace("ghost", "primary");
    $("print").textContent = "Print / PDF";
  }
  state.slug = null;
  await Promise.all([refreshList(), refreshProfile()]);
  state.revision = await api.revision();
  await openResume(state.resumes[0]?.slug ?? null);
}

void boot();
setInterval(poll, 1500);
document.addEventListener("visibilitychange", () => !document.hidden && void poll());
