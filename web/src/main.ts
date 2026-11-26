import { api, STATUSES, type ResumeSummary, type Status } from "./api";
import { ResumeView } from "./resume";
import { activeResumes, app, firstResume, resumeRoute, store, type Route, type View } from "./store";
import { $, capitalize, h, hydrateIcons, icon, runDialog, showError, toast } from "./ui";
import { AboutView, ArchiveView, ConnectView, InstructionsView, SkillsView, TemplateView } from "./views";

hydrateIcons();

let views: Record<Exclude<Route["kind"], "welcome">, View> & { welcome: View };
let resumeView: ResumeView;

// ================================================================ routing

let navigating: Promise<void> = Promise.resolve();

app.navigate = (route: Route) => {
  navigating = navigating.then(() => go(route)).catch(showError);
  return navigating;
};

async function go(route: Route) {
  const from = views[store.route.kind];
  await from.leave();
  store.route = route;
  for (const v of Object.values(views)) v.section.hidden = v !== views[route.kind];
  for (const b of document.querySelectorAll<HTMLElement>(".nav-item")) b.classList.toggle("active", b.dataset.route === route.kind);
  renderList();
  await views[route.kind].show(route);
}

const welcomeView: View = { section: $("view-welcome"), show: async () => {}, leave: async () => {}, onDiskChange: async () => {} };

document.addEventListener("click", (e) => {
  const target = (e.target as HTMLElement).closest<HTMLElement>("[data-route]");
  if (target) void app.navigate({ kind: target.dataset.route } as Route);
});

// ================================================================ sidebar

type Filter = "active" | Exclude<Status, "archived">;
let filter: Filter = "active";
let focused = -1; // keyboard selection while searching
const search = $<HTMLInputElement>("search");
const listEl = $("resume-list");

app.refreshList = async () => {
  store.resumes = await api.listResumes();
  renderList();
};

function visible(): ResumeSummary[] {
  const q = search.value.trim().toLowerCase();
  return activeResumes().filter((r) => {
    const inFilter = filter === "active" || r.status === filter;
    const matches = !q || [r.title, r.slug, ...r.tags].some((s) => s.toLowerCase().includes(q));
    return (q ? true : inFilter) && matches;
  });
}

function renderList() {
  const active = activeResumes();
  const counts = new Map<Filter, number>([["active", active.length]]);
  for (const r of active) counts.set(r.status as Filter, (counts.get(r.status as Filter) ?? 0) + 1);
  const archived = store.resumes.length - active.length;
  $("count-archive").textContent = archived ? String(archived) : "";
  const filters: Filter[] = ["active", ...STATUSES.filter((s): s is Exclude<Status, "archived"> => s !== "archived")];
  $("status-filter").replaceChildren(
    ...filters
      .filter((f) => f === "active" || f === filter || counts.get(f))
      .map((f) =>
        h(
          "button",
          {
            type: "button",
            class: "chip" + (f === filter ? " on" : ""),
            "aria-pressed": String(f === filter),
            title: f === "active" ? `${counts.get(f)} not archived` : `${counts.get(f) ?? 0} ${f}`,
            onclick: () => setFilter(f),
          },
          f === "active" ? "All" : capitalize(f),
        ),
      ),
  );

  const shown = visible();
  focused = Math.min(focused, shown.length - 1);
  const current = store.route.kind === "resume" ? store.route.slug : null;
  listEl.replaceChildren(
    ...shown.map((r, i) => {
      const row = h(
        "div",
        { class: "row" + (r.slug === current ? " active" : "") + (i === focused ? " focused" : ""), dataset: { slug: r.slug } },
        h(
          "button",
          { type: "button", class: "row-main", title: r.title, onclick: () => void app.navigate({ kind: "resume", slug: r.slug }) },
          h("span", { class: `dot ${r.status}`, title: capitalize(r.status) }),
          h(
            "span",
            { class: "row-title" },
            h("span", { class: "row-name" }, r.title),
            r.has_posting && h("span", { title: "Has a target job posting", "aria-label": "has a target job posting", role: "img" }, icon("target")),
          ),
          h("span", { class: "row-sub" }, ResumeView.describe(r.words, r.tags, r.updated_ms)),
        ),
        h(
          "button",
          {
            type: "button",
            class: "icon-btn row-more",
            title: "More",
            "aria-label": `More actions for ${r.title}`,
            "aria-haspopup": "menu",
            onclick: (e: Event) => {
              e.stopPropagation();
              openMenu(r, e.currentTarget as HTMLElement);
            },
          },
          icon("more"),
        ),
      );
      row.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        openMenu(r, row, e.clientX, e.clientY);
      });
      return row;
    }),
  );
  if (!shown.length) {
    listEl.append(
      active.length
        ? h("p", { class: "list-empty" }, search.value ? "No matches." : `No ${filter === "active" ? "" : filter + " "}resumes.`)
        : h("p", { class: "list-empty" }, "No resumes yet. ", h("button", { type: "button", onclick: () => app.newResume() }, "Create one"), "."),
    );
  }
  listEl.querySelector(".row.active, .row.focused")?.scrollIntoView({ block: "nearest" });
}

function setFilter(f: Filter) {
  filter = f;
  search.value = "";
  renderList();
}

search.addEventListener("input", () => {
  focused = search.value ? 0 : -1;
  renderList();
});
search.addEventListener("keydown", (e) => {
  const shown = visible();
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    focused = Math.max(0, Math.min(shown.length - 1, focused + (e.key === "ArrowDown" ? 1 : -1)));
    renderList();
  } else if (e.key === "Enter" && shown[Math.max(0, focused)]) {
    void app.navigate({ kind: "resume", slug: shown[Math.max(0, focused)].slug });
    search.value = "";
    focused = -1;
    search.blur();
  } else if (e.key === "Escape") {
    search.value = "";
    focused = -1;
    renderList();
    search.blur();
  }
});
search.addEventListener("blur", () => {
  focused = -1;
  setTimeout(renderList, 150); // let a click on a row land first
});

// ---------------------------------------------------------------- row menu

const menu = $("row-menu");
let menuFor: ResumeSummary | null = null;

function openMenu(r: ResumeSummary, anchor: HTMLElement, x?: number, y?: number) {
  closeMenu();
  menuFor = r;
  const box = anchor.getBoundingClientRect();
  menu.hidden = false;
  const left = x ?? box.right - menu.offsetWidth;
  const top = y ?? box.bottom + 4;
  menu.style.left = `${Math.max(8, Math.min(left, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.min(top, innerHeight - menu.offsetHeight - 8)}px`;
  anchor.closest(".row")?.classList.add("menu-open");
  menu.querySelector<HTMLElement>(".menu-item")?.focus();
}

function closeMenu() {
  menu.hidden = true;
  menuFor = null;
  document.querySelectorAll(".row.menu-open").forEach((n) => n.classList.remove("menu-open"));
}

menu.addEventListener("click", async (e) => {
  const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]")?.dataset.act;
  const r = menuFor;
  closeMenu();
  if (!r || !act) return;
  if (act === "duplicate") app.newResume(r.slug);
  else if (act === "delete") await app.deleteResume(r.slug);
  else if (act === "archive") await app.archive(r.slug);
});
menu.addEventListener("keydown", (e) => {
  const items = [...menu.querySelectorAll<HTMLElement>(".menu-item")];
  const i = items.indexOf(document.activeElement as HTMLElement);
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length].focus();
  }
});
document.addEventListener("click", (e) => {
  if (!menu.hidden && !menu.contains(e.target as Node)) closeMenu();
});
listEl.addEventListener("scroll", closeMenu);

// ================================================================ new, duplicate, archive, delete

const dlgNew = $<HTMLDialogElement>("dlg-new");
const newTitle = dlgNew.querySelector<HTMLInputElement>("input[name=title]")!;
const newFrom = dlgNew.querySelector<HTMLSelectElement>("select[name=from]")!;

app.newResume = (from?: string) => {
  const source = store.resumes.find((r) => r.slug === from);
  newFrom.replaceChildren(
    new Option("Default template", ""),
    ...store.resumes.map((r) => new Option(`Copy of ${r.title}`, r.slug)),
  );
  newFrom.value = source?.slug ?? "";
  newTitle.value = source ? `${source.title} (copy)` : "";
  void runDialog(dlgNew).then(async (result) => {
    const title = newTitle.value.trim();
    if (result !== "ok" || !title) return;
    try {
      const slug = await api.createResume(title, newFrom.value || undefined);
      await app.refreshList();
      if (filter !== "active" && filter !== "draft") filter = "active";
      await app.navigate({ kind: "resume", slug });
    } catch (e) {
      showError(e);
    }
  });
  newTitle.focus();
  newTitle.select();
};

/** If `slug` is open, save it and close it; returns whether it was. */
async function closeIfOpen(slug: string, save: boolean) {
  const open = store.route.kind === "resume" && store.route.slug === slug;
  if (open) {
    if (save) await resumeView.leave();
    resumeView.close();
  }
  return open;
}

async function afterChange(wasOpen: boolean) {
  await app.refreshList();
  if (wasOpen) await app.navigate(resumeRoute(firstResume()?.slug));
  await views.archive.onDiskChange();
}

app.archive = async (slug: string) => {
  try {
    const open = await closeIfOpen(slug, true);
    const r = await api.archiveResume(slug);
    await afterChange(open);
    toast(`Archived "${r.title}".`, { action: { label: "Undo", run: () => app.restore(slug) } });
  } catch (e) {
    showError(e);
  }
};

app.restore = async (slug: string) => {
  try {
    const r = await api.restoreResume(slug);
    await afterChange(false);
    toast(`Restored "${r.title}".`, { action: { label: "Open", run: () => app.navigate({ kind: "resume", slug }) } });
  } catch (e) {
    showError(e);
  }
};

const dlgDelete = $<HTMLDialogElement>("dlg-delete");

app.deleteResume = async (slug: string) => {
  const title = store.resumes.find((r) => r.slug === slug)?.title ?? slug;
  $("dlg-delete-msg").textContent = `"${title}" and its target job posting will be erased from your data folder. This can't be undone.`;
  if ((await runDialog(dlgDelete)) !== "ok") return;
  try {
    const open = await closeIfOpen(slug, false); // its edits go with it
    await api.deleteResume(slug);
    await afterChange(open);
    toast(`Deleted "${title}".`);
  } catch (e) {
    showError(e);
  }
};

app.saveAll = () => views[store.route.kind].leave();

$("new-resume").onclick = () => app.newResume();
$("welcome-new").onclick = () => app.newResume();

// ================================================================ live updates from disk

let revision = "";
let polling = false;

async function poll() {
  if (document.hidden || polling) return;
  polling = true;
  try {
    const rev = await api.revision();
    if (rev === revision) return;
    revision = rev;
    await app.refreshList();
    const route = store.route;
    if (route.kind === "resume" && !store.resumes.some((r) => r.slug === route.slug)) {
      resumeView.close();
      toast("The open resume was deleted outside the app.");
      await app.navigate(resumeRoute(firstResume()?.slug));
    } else if (route.kind === "welcome" && firstResume()) {
      await app.navigate(resumeRoute(firstResume()?.slug));
    }
    await Promise.all(Object.values(views).map((v) => v.onDiskChange()));
  } catch {
    /* transient; the next tick retries */
  } finally {
    polling = false;
  }
}

// ================================================================ keyboard

/** Keys the webview would treat as a browser's: reload, find, caret browsing, view source, history,
 *  back/forward, and (outside `npm run app`) the developer tools. Editing keys are never listed. */
function browserKey(e: KeyboardEvent): boolean {
  const k = e.key.toLowerCase();
  const mod = e.ctrlKey || e.metaKey;
  const devtools = !import.meta.env.DEV;
  if (["f3", "f5", "f7", "browserback", "browserforward", "browserrefresh", "browsersearch", "browserhome"].includes(k)) return true;
  if (k === "f12") return devtools;
  if (e.altKey && !mod && (k === "arrowleft" || k === "arrowright" || k === "home")) return true;
  if (!mod || e.altKey) return false;
  if (e.shiftKey && ["i", "j", "c"].includes(k)) return devtools;
  return ["r", "f", "g", "u", "o", "j", "h", "p", "l", "d", "+", "=", "-", "0"].includes(k);
}

// Mouse back/forward buttons would navigate the webview away from the app.
addEventListener("mouseup", (e) => (e.button === 3 || e.button === 4) && e.preventDefault());

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !menu.hidden) return closeMenu();
  // Runs after the editor's keymap, so its own Ctrl+U (underline) and friends have already happened.
  if (browserKey(e)) e.preventDefault();
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
  const k = e.key.toLowerCase();
  const onResume = store.route.kind === "resume" && !dlgNew.open;
  const run = (f: () => unknown) => {
    e.preventDefault();
    void f();
  };
  if (k === "k") run(() => (search.focus(), search.select()));
  else if (k === "n" && !dlgNew.open) run(() => app.newResume());
  else if (k === "s" && e.shiftKey && onResume) run(() => resumeView.exportPdf());
  else if (k === "s") run(() => views[store.route.kind].leave());
  else if (k === "p" && onResume) run(() => resumeView.print());
  else if (k === "\\" && onResume) run(() => resumeView.toggleInspector());
});

window.addEventListener("beforeunload", () => void views[store.route.kind].leave());
document.addEventListener("visibilitychange", () => !document.hidden && void poll());

// ================================================================ boot

async function boot() {
  store.info = await api.info();
  resumeView = new ResumeView();
  const about = new AboutView();
  views = {
    resume: resumeView,
    template: new TemplateView(),
    instructions: new InstructionsView(),
    skills: new SkillsView(),
    connect: new ConnectView(),
    archive: new ArchiveView(),
    about,
    welcome: welcomeView,
  };
  const [instructions] = await Promise.all([api.readInstructions(), app.refreshList()]);
  $("count-instructions").textContent = instructions.items.length ? String(instructions.items.length) : "";
  revision = await api.revision();
  await app.navigate(resumeRoute(firstResume()?.slug));
  setInterval(poll, 1500);
  const up = store.info.upgraded;
  if (up) {
    toast(`Your data was upgraded to this version's format (${up.from} → ${up.to}). A copy of the old files is in ${up.backup}.`, {
      ms: 15000,
      action: { label: "Copy path", run: () => navigator.clipboard.writeText(up.backup) },
    });
  }
  setTimeout(() => void about.startupCheck(), 4000);
}

boot().catch((e) => {
  document.body.replaceChildren(h("pre", { class: "boot-error" }, `ResuFlow could not start:\n${e}`));
});
