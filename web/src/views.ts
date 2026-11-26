// Workspace views: the default template, instructions, built-in skills, connecting an agent, the archive, and about.

import { api, type Licenses, type Skill, type ThirdParty, type ThirdPartyPackage, type UpdateInfo } from "./api";
import { attachEditMenu } from "./editmenu";
import { createLiveEditor } from "./livemd";
import { app, store, type Route, type View } from "./store";
import { $, Autosave, copyText, h, icon, paintSaveState, plural, relTime, toast } from "./ui";

/** A conflict banner with "load theirs" and "keep mine". */
function conflictBanner(banner: HTMLElement, message: string, theirs: () => Promise<void>, mine: () => Promise<void>) {
  const done = (f: () => Promise<void>) => () => void f().then(() => (banner.hidden = true));
  banner.hidden = false;
  banner.replaceChildren(
    h("span", { class: "msg" }, message),
    h("button", { class: "btn small", type: "button", textContent: "Load their version", onclick: done(theirs) }),
    h("button", { class: "btn small", type: "button", textContent: "Keep mine", onclick: done(mine) }),
  );
}

// ================================================================ default template

export class TemplateView implements View {
  section = $("view-template");
  private state = $("template-state");
  private banner = $("template-banner");
  private loaded = false;

  private editor = createLiveEditor({
    parent: $("template-editor"),
    placeholder: "What every new resume starts with: # your name, a headline and contact line, then your usual sections.",
    header: true,
    onChange: () => this.save.touch(),
  });

  private save = new Autosave({
    write: async (hash) => {
      const p = await api.writeBody("profile", this.editor.get(), hash);
      return p.hash;
    },
    state: (s, d) => paintSaveState(this.state, s, d),
    onConflict: () =>
      conflictBanner(this.banner, "The default template changed outside the editor while you had unsaved edits.", () => this.load(true), () => this.save.flush(true)),
  });

  constructor() {
    attachEditMenu(this.editor); // the same formatting and styling menu as a resume
  }

  async show(_route: Route) {
    if (!this.loaded) await this.load(false);
    this.loaded = true;
  }

  async leave() {
    await this.save.flush();
  }

  private async load(keepPlace: boolean) {
    const p = await api.readParts("profile");
    this.banner.hidden = true;
    this.save.reset(p.hash);
    this.editor.set(p.body, keepPlace);
    paintSaveState(this.state, "idle");
  }

  async onDiskChange() {
    if (!this.loaded || this.save.busy) return;
    const p = await api.readParts("profile");
    if (p.hash === this.save.hash) return;
    if (this.save.dirty) this.save.markConflict();
    else await this.load(true);
  }
}

// ================================================================ instructions

export class InstructionsView implements View {
  section = $("view-instructions");
  private listEl = $("instruction-list");
  private state = $("instructions-state");
  private banner = $("instructions-banner");
  private items: string[] = [];
  private loaded = false;

  private save = new Autosave({
    write: async (hash) => (await api.writeInstructions(this.items, hash)).hash,
    state: (s, d) => paintSaveState(this.state, s, d),
    onConflict: () =>
      conflictBanner(this.banner, "Your instructions changed outside this window while you were editing.", () => this.load(), () => this.save.flush(true)),
  });

  constructor() {
    $("instruction-add").onclick = () => this.add();
  }

  async show(_route: Route) {
    await this.load();
    if (!this.items.length) this.add();
  }

  async leave() {
    await this.save.flush();
  }

  private async load() {
    const f = await api.readInstructions();
    this.banner.hidden = true;
    this.save.reset(f.hash);
    this.items = f.items;
    this.loaded = true;
    this.render();
    paintSaveState(this.state, "idle");
  }

  private add() {
    this.items.push("");
    this.render();
    this.listEl.querySelectorAll("textarea")[this.items.length - 1]?.focus();
  }

  private render() {
    $("count-instructions").textContent = String(this.items.filter((i) => i.trim()).length || "");
    this.listEl.replaceChildren(
      ...this.items.map((text, i) => {
        const area = h("textarea", { value: text, rows: 1, placeholder: "An instruction for your agent", "aria-label": `Instruction ${i + 1}` });
        const grow = () => {
          area.style.height = "auto";
          area.style.height = `${area.scrollHeight}px`;
        };
        area.addEventListener("input", () => {
          this.items[i] = area.value;
          grow();
          this.save.touch();
        });
        area.addEventListener("keydown", (e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            this.add();
          } else if (e.key === "Backspace" && !area.value && this.items.length > 1) {
            e.preventDefault();
            this.remove(i, true);
          }
        });
        requestAnimationFrame(grow);
        const remove = h("button", { class: "icon-btn", type: "button", title: "Remove", "aria-label": `Remove instruction ${i + 1}`, onclick: () => this.remove(i) }, icon("x"));
        return h("li", { class: "instruction" }, area, remove);
      }),
    );
  }

  private remove(i: number, focusPrevious = false) {
    const removed = this.items.splice(i, 1)[0];
    this.render();
    this.save.touch();
    if (focusPrevious) this.listEl.querySelectorAll("textarea")[Math.max(0, i - 1)]?.focus();
    else if (removed.trim()) {
      toast("Instruction removed.", {
        action: {
          label: "Undo",
          run: () => {
            this.items.splice(i, 0, removed);
            this.render();
            this.save.touch();
          },
        },
      });
    }
  }

  async onDiskChange() {
    if (!this.loaded || this.save.busy) return;
    const f = await api.readInstructions();
    if (f.hash === this.save.hash) return;
    if (this.save.dirty) this.save.markConflict();
    else if (!this.section.hidden) await this.load();
    else $("count-instructions").textContent = f.items.length ? String(f.items.length) : "";
  }
}

// ================================================================ skills

export class SkillsView implements View {
  section = $("view-skills");
  private skills: Skill[] = [];
  private current: Skill | null = null;
  private viewer = createLiveEditor({ parent: $("skill-viewer") });

  constructor() {
    this.viewer.setReadOnly(true);
  }

  async show(route: Route) {
    if (!this.skills.length) this.skills = await api.listSkills();
    const name = route.kind === "skills" ? route.name : undefined;
    this.select(this.skills.find((s) => s.name === name) ?? this.current ?? this.skills[0]);
  }

  private select(skill: Skill | undefined) {
    if (!skill) return;
    this.current = skill;
    $("skill-list").replaceChildren(
      ...this.skills.map((s) =>
        h(
          "button",
          { type: "button", class: "skill-card" + (s === skill ? " active" : ""), onclick: () => this.select(s) },
          h("strong", {}, s.name),
          h("span", {}, s.description),
        ),
      ),
    );
    this.viewer.set(skill.content.replace(/^---\n[\s\S]*?\n---\n+/, ""));
  }

  async leave() {}
  async onDiskChange() {}
}

// ================================================================ connect an agent

export class ConnectView implements View {
  section = $("view-connect");

  constructor() {
    for (const b of this.section.querySelectorAll<HTMLButtonElement>("[data-copy]")) {
      b.onclick = () => void copyText(b, $(b.dataset.copy!).textContent ?? "");
    }
  }

  async show() {
    const info = store.info!;
    $("mcp-json").textContent = JSON.stringify(info.mcp_config, null, 2);
    $("data-root").textContent = info.root;
    $("app-version").textContent = info.version;
  }

  async leave() {}
  async onDiskChange() {}
}

// ================================================================ archive

export class ArchiveView implements View {
  section = $("view-archive");

  async show() {
    this.render();
  }

  /** Reads `store.resumes`, which main.ts refreshes before calling this. */
  render() {
    const archived = store.resumes.filter((r) => r.status === "archived");
    $("archive-list").replaceChildren(
      ...archived.map((r) =>
        h(
          "li",
          { class: "archive-item" },
          h(
            "button",
            { class: "archive-open", type: "button", title: `Open ${r.title}`, onclick: () => void app.navigate({ kind: "resume", slug: r.slug }) },
            icon("doc"),
            h("div", {}, h("strong", {}, r.title), h("span", {}, `Edited ${relTime(r.updated_ms)} · ${r.slug}`)),
          ),
          h("button", { class: "btn small", type: "button", onclick: () => void app.restore(r.slug) }, icon("restore"), "Restore"),
          h("button", { class: "btn small danger-text", type: "button", onclick: () => void app.deleteResume(r.slug) }, icon("trash"), "Delete…"),
        ),
      ),
    );
  }

  async leave() {}

  async onDiskChange() {
    if (!this.section.hidden) this.render();
  }
}

// ================================================================ about: version, updates, licenses

const AUTO_UPDATE_KEY = "resuflow.checkForUpdates";

type LicenseDoc = "license" | "notice" | "third_party";

/** A per-device preference, so it lives in the webview, not the data folder. On by default. */
function autoUpdate(): boolean {
  try {
    return localStorage.getItem(AUTO_UPDATE_KEY) !== "off";
  } catch {
    return true;
  }
}

export class AboutView implements View {
  section = $("view-about");
  private status = $("update-status");
  private checkBtn = $<HTMLButtonElement>("update-check");
  private installBtn = $<HTMLButtonElement>("update-install");
  private found: UpdateInfo | null = null;
  private licenses: Licenses | null = null;
  private busy = false;

  constructor() {
    this.checkBtn.onclick = () => void this.check(true);
    this.installBtn.onclick = () => void this.install();
    const auto = $<HTMLInputElement>("update-auto");
    auto.checked = autoUpdate();
    auto.onchange = () => {
      try {
        localStorage.setItem(AUTO_UPDATE_KEY, auto.checked ? "on" : "off");
      } catch {
        /* storage unavailable: the default stays */
      }
    };
    for (const b of this.section.querySelectorAll<HTMLButtonElement>("[data-license]")) {
      b.onclick = () => void this.showLicense(b.dataset.license as LicenseDoc);
    }
  }

  async show() {
    const info = store.info!;
    $("about-version").textContent = info.version;
    $("about-license").textContent = info.license;
    $("about-root").textContent = info.root;
    $("about-format").textContent = String(info.format);
    this.paint();
  }

  /** On start: check quietly, and only speak up when there is something to install. */
  async startupCheck() {
    if (!store.info!.updates || !autoUpdate()) return;
    try {
      const u = await this.check(false);
      if (u) toast(`ResuFlow ${u.version} is available.`, { action: { label: "View", run: () => app.navigate({ kind: "about" }) } });
    } catch {
      /* offline, or GitHub unreachable: try again next start */
    }
  }

  private async check(manual: boolean): Promise<UpdateInfo | null> {
    if (this.busy) return this.found;
    this.busy = true;
    this.paint("Checking for updates…");
    let message: string | undefined;
    try {
      this.found = await api.checkUpdate();
      if (!this.found) message = `You have the latest version (${store.info!.version}).`;
      return this.found;
    } catch (e) {
      message = String(e).replace(/^Error: /, "");
      if (!manual) throw e;
      return null;
    } finally {
      this.busy = false;
      this.paint(message);
    }
  }

  private async install() {
    if (!this.found || this.busy) return;
    this.busy = true;
    this.paint(`Downloading ResuFlow ${this.found.version}…`);
    const bar = $("update-progress");
    const fill = bar.querySelector("span")!;
    fill.style.width = "0";
    bar.hidden = false;
    try {
      await app.saveAll(); // nothing unsaved is lost when the app restarts
      await api.installUpdate((p) => {
        if (p.total) fill.style.width = `${Math.min(100, (p.done / p.total) * 100)}%`;
      });
      this.paint("Restarting…"); // the app restarts (or, on Windows, the installer takes over)
    } catch (e) {
      bar.hidden = true;
      this.busy = false;
      this.paint(String(e).replace(/^Error: /, ""));
    }
  }

  /** Reflect the current state; `message` replaces the status line. */
  private paint(message?: string) {
    const info = store.info!;
    const f = this.found;
    $("update-badge").textContent = f ? "Update" : "";
    this.checkBtn.disabled = this.busy || !info.updates;
    $("update-auto").closest("label")!.hidden = !info.updates;
    this.installBtn.hidden = !f;
    this.installBtn.disabled = this.busy;
    $("update-found").hidden = !f;
    $("update-notes").textContent = f?.notes ?? "";
    if (message !== undefined) this.status.textContent = message;
    else if (!info.updates) {
      this.status.textContent = import.meta.env.DEV
        ? "Updates are off in development builds."
        : "This copy wasn't installed from a ResuFlow release (for example, it came from a package manager or was built from source), so update it the same way you installed it.";
    } else if (f && !this.busy) {
      this.status.textContent = `ResuFlow ${f.version} is available${f.date ? `, released ${f.date}` : ""}. You have ${info.version}.`;
    } else if (!this.status.textContent) this.status.textContent = `You have ResuFlow ${info.version}.`;
  }

  private async showLicense(which: LicenseDoc) {
    this.licenses ??= await api.licenses();
    const body = $("license-body");
    const open = body.dataset.showing === which && !body.hidden;
    for (const b of this.section.querySelectorAll<HTMLElement>("[data-license]")) {
      b.setAttribute("aria-pressed", String(!open && b.dataset.license === which));
    }
    body.hidden = open;
    body.dataset.showing = which;
    if (open) return;
    const l = this.licenses;
    if (which === "license") body.replaceChildren(h("pre", { class: "license-text", tabIndex: 0 }, l.license));
    else if (which === "notice") body.replaceChildren(h("p", { class: "license-notice" }, l.notice.trim()));
    else body.replaceChildren(...thirdParty(l.third_party));
  }

  async leave() {}
  async onDiskChange() {}
}

/** Markdown prose from the licenses file: only `inline code` needs rendering. */
function prose(md: string): HTMLParagraphElement {
  return h("p", {}, ...md.split(/`([^`]+)`/).map((part, i) => (i % 2 ? h("code", {}, part) : part)));
}

/** The third-party notices: what they cover, then every package, each expanding to its license texts. */
function thirdParty(tp: ThirdParty): Node[] {
  const [preamble, ...sections] = tp.prose;
  const summary = h("ul", { class: "license-summary", "aria-label": "Packages by license" },
    ...tp.summary.map(([id, n]) => h("li", {}, h("strong", {}, id), ` ${n}`)));

  const empty = h("p", { class: "hint", hidden: true }, "No package matches.");
  const groups = tp.groups.map((g) => {
    const items = g.packages.map((p) => thirdPartyItem(p, tp));
    const section = h("section", { class: "dep-group" },
      h("h3", {}, g.title, h("span", { class: "badge" }, String(g.packages.length))),
      g.note && prose(g.note),
      h("ul", { class: "dep-list" }, ...items));
    return { section, items };
  });

  const filter = h("input", {
    type: "search", placeholder: "Filter by package or license", "aria-label": "Filter packages",
    oninput: () => {
      const q = filter.value.trim().toLowerCase();
      let shown = 0;
      for (const g of groups) {
        for (const li of g.items) li.hidden = !li.dataset.search!.includes(q);
        const n = g.items.filter((li) => !li.hidden).length;
        g.section.hidden = n === 0;
        shown += n;
      }
      empty.hidden = shown > 0;
    },
  });

  return [
    ...preamble.paragraphs.map(prose),
    summary,
    ...sections.flatMap((s) => [h("h3", {}, s.title), ...s.paragraphs.map(prose)]),
    h("label", { class: "search license-filter" }, icon("search"), filter),
    ...groups.map((g) => g.section),
    empty,
  ];
}

function thirdPartyItem(p: ThirdPartyPackage, tp: ThirdParty): HTMLLIElement {
  const details = h("details", {},
    h("summary", {},
      h("span", { class: "dep-name" }, p.name),
      h("span", { class: "dep-version" }, p.version),
      p.platforms && h("span", { class: "badge" }, p.platforms),
      h("span", { class: "dep-license" }, p.used)));
  // Texts run to hundreds of kilobytes in all, so each is only built when its package is opened.
  details.addEventListener("toggle", () => {
    if (!details.open || details.childElementCount > 1) return;
    const fact = (k: string, v: string) => (v ? [h("dt", {}, k), h("dd", {}, v)] : []);
    details.append(h("div", { class: "dep-body" },
      h("dl", { class: "facts" },
        ...fact("Declared license", p.declared),
        ...fact("Used under", p.used !== p.declared ? p.used : ""),
        ...fact("Platforms", p.platforms),
        ...fact("Included as", p.note),
        ...fact("Source", p.source)),
      ...p.texts.map((i) => {
        const t = tp.texts[i];
        return h("div", { class: "dep-text" },
          h("p", { class: "hint" }, t.kind, t.users > 1 ? ` · the same text covers ${plural(t.users - 1, "other package")}` : ""),
          h("pre", { class: "license-text", tabIndex: 0 }, t.text));
      })));
  });
  const search = [p.name, p.used, p.declared, p.platforms].join(" ").toLowerCase();
  return h("li", { dataset: { search } }, details);
}
