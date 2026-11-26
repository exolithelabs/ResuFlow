import { api, STATUSES, type ResumeSummary, type Status } from "./api";
import { ResumeView } from "./resume";
import { app, firstResume, resumeRoute, store, type Route, type View } from "./store";
import { $, capitalize, h, hydrateIcons, icon, runDialog, showError, toast } from "./ui";
import { ConnectView, InstructionsView, SkillsView, TemplateView, TrashView, restore } from "./views";

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

type Filter = "active" | Status;
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
  return store.resumes.filter((r) => {
    const inFilter = filter === "active" ? r.status !== "archived" : r.status === filter;
    const matches = !q || [r.title, r.slug, ...r.tags].some((s) => s.toLowerCase().includes(q));
    return (q ? true : inFilter) && matches;
  });
}

function renderList() {
  const counts = new Map<Filter, number>([["active", 0]]);
  for (const r of store.resumes) {
    counts.set(r.status, (counts.get(r.status) ?? 0) + 1);
    if (r.status !== "archived") counts.set("active", counts.get("active")! + 1);
  }
  const filters: Filter[] = ["active", ...STATUSES];
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
      store.resumes.length
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
  menu.querySelector('[data-act="archive"] span')!.textContent = r.status === "archived" ? "Unarchive" : "Archive";
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
  else if (act === "delete") await app.trash(r.slug);
  else if (act === "archive") await setStatus(r, r.status === "archived" ? "draft" : "archived");
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

async function setStatus(r: ResumeSummary, status: Status) {
  if (store.route.kind === "resume" && store.route.slug === r.slug) return resumeView.setMeta({ status });
  try {
    await api.setMeta(r.slug, { status });
    await app.refreshList();
  } catch (e) {
    showError(e);
  }
}

// ================================================================ new, duplicate, delete

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

app.trash = async (slug: string) => {
  const open = store.route.kind === "resume" && store.route.slug === slug;
  try {
    if (open) {
      await resumeView.leave(); // unsaved edits go to the trash with it
      resumeView.close();
    }
    const entry = await api.deleteResume(slug);
    await app.refreshList();
    if (open) await app.navigate(resumeRoute(firstResume()?.slug));
    await views.trash.onDiskChange();
    toast(`Moved "${entry.title}" to the trash.`, { action: { label: "Undo", run: () => restore(entry) } });
  } catch (e) {
    showError(e);
  }
};

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
    } else if (route.kind === "welcome" && store.resumes.length) {
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

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !menu.hidden) return closeMenu();
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
  views = {
    resume: resumeView,
    template: new TemplateView(),
    instructions: new InstructionsView(),
    skills: new SkillsView(),
    connect: new ConnectView(),
    trash: new TrashView(),
    welcome: welcomeView,
  };
  const [instructions, trash] = await Promise.all([
    api.readInstructions(),
    api.listTrash(),
    app.refreshList(),
  ]);
  $("count-instructions").textContent = instructions.items.length ? String(instructions.items.length) : "";
  $("count-trash").textContent = trash.length ? String(trash.length) : "";
  revision = await api.revision();
  await app.navigate(resumeRoute(firstResume()?.slug));
  setInterval(poll, 1500);
}

boot().catch((e) => {
  document.body.replaceChildren(h("pre", { class: "boot-error" }, `ResuFlow could not start:\n${e}`));
});
