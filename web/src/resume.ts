// The resume view: one live-preview page that looks like the printout (edit it in place,
// Obsidian-style), and a side panel with checks, the target job posting and details.

import { api, STATUSES, str, list, type Parts, type Report, type Status } from "./api";
import { attachEditMenu } from "./editmenu";
import { createLiveEditor, stripComponents } from "./livemd";
import { app, store, type Route, type View } from "./store";
import { $, Autosave, capitalize, h, icon, paintSaveState, plural, relTime, showError, toast, wordCount } from "./ui";

type Tab = "review" | "target" | "details";

const pref = {
  get: (k: string) => {
    try {
      return localStorage.getItem(`resuflow.${k}`);
    } catch {
      return null;
    }
  },
  set: (k: string, v: string) => {
    try {
      localStorage.setItem(`resuflow.${k}`, v);
    } catch {
      /* private mode: preferences just don't persist */
    }
  },
};

export class ResumeView implements View {
  section = $("view-resume");
  slug: string | null = null;
  private fields: Parts["fields"] = {};
  private bodyOffset = 0;
  private postingHash = "";
  private checkTimer = 0;

  private el = {
    title: $<HTMLInputElement>("title"),
    status: $<HTMLSelectElement>("status"),
    saveState: $("save-state"),
    banner: $("banner"),
    pageScroll: $("page-scroll"),
    workarea: this.section.querySelector<HTMLElement>(".workarea")!,
    inspectorBtn: $("toggle-inspector"),
    findings: $("findings"),
    posting: $<HTMLTextAreaElement>("posting"),
    postingState: $("posting-state"),
    tags: $("tags"),
    tagEntry: $<HTMLInputElement>("tag-entry"),
  };

  private editor = createLiveEditor({
    parent: $("editor-host"),
    placeholder: "# Your name, then a headline line and a contact line. ## starts a section, ### a role, - a bullet.",
    header: true,
    onChange: (text) => {
      $("stat-words").textContent = String(wordCount(stripComponents(text)));
      this.save.touch();
    },
  });

  /** The resume file: body edits and meta changes share one hash. */
  private save = new Autosave({
    write: async (hash) => {
      const p = await api.writeBody(this.file, this.editor.get(), hash);
      this.applyParts(p);
      this.afterSave();
      return p.hash;
    },
    state: (s, d) => paintSaveState(this.el.saveState, s, d),
    onConflict: () =>
      this.conflict("This resume changed outside the editor while you had unsaved edits.", async () => {
        await this.load(true);
      }, () => this.save.flush(true)),
  });

  private postingSave = new Autosave({
    delay: 700,
    write: async (hash) => {
      const f = await api.writePosting(this.slug!, this.el.posting.value, hash);
      this.postingHash = f.hash;
      this.afterSave();
      return f.hash;
    },
    state: (s, d) => paintSaveState(this.el.postingState, s, d),
    onConflict: () =>
      this.conflict("The target job posting changed outside the editor while you were editing it.", async () => {
        await this.loadPosting();
      }, () => this.postingSave.flush(true)),
  });

  constructor() {
    attachEditMenu(this.editor);
    this.el.status.replaceChildren(...STATUSES.map((s) => new Option(capitalize(s), s)));
    this.el.status.addEventListener("change", () => void this.setMeta({ status: this.el.status.value as Status }));
    this.el.title.addEventListener("change", () => {
      const t = this.el.title.value.trim();
      if (t) void this.setMeta({ title: t });
      else this.el.title.value = str(this.fields.title);
    });
    this.el.title.addEventListener("keydown", (e) => {
      if (e.key === "Enter") this.el.title.blur();
      if (e.key === "Escape") {
        this.el.title.value = str(this.fields.title);
        this.el.title.blur();
      }
    });

    this.el.inspectorBtn.onclick = () => this.toggleInspector();
    const inspector = pref.get("inspector");
    this.toggleInspector(inspector ? inspector === "1" : innerWidth >= 1180); // narrow windows: it would cover the page
    for (const b of this.section.querySelectorAll<HTMLButtonElement>("[data-tab]")) {
      b.onclick = () => this.setTab(b.dataset.tab as Tab);
    }
    this.setTab((pref.get("tab") as Tab) || "review");

    this.el.posting.addEventListener("input", () => this.postingSave.touch());
    this.el.posting.addEventListener("blur", () => void this.postingSave.flush());

    this.el.tags.addEventListener("click", () => this.el.tagEntry.focus());
    this.el.tagEntry.addEventListener("keydown", (e) => {
      const v = this.el.tagEntry.value.trim().replace(/,$/, "");
      if ((e.key === "Enter" || e.key === ",") && v) {
        e.preventDefault();
        this.el.tagEntry.value = "";
        void this.setMeta({ tags: [...list(this.fields.tags), v] });
      } else if (e.key === "Backspace" && !this.el.tagEntry.value) {
        const tags = list(this.fields.tags);
        if (tags.length) void this.setMeta({ tags: tags.slice(0, -1) });
      }
    });

    $("export").onclick = () => void this.exportPdf();
    $("print").onclick = () => void this.print();
    $("duplicate").onclick = () => this.slug && app.newResume(this.slug);
    $("delete").onclick = () => this.slug && void app.deleteResume(this.slug);

    if (!store.info?.pdf_export) {
      $("export").hidden = true;
      $("print").classList.add("primary");
      $("print").title = "Print or save as PDF (Ctrl+P)";
    }
  }

  private get file() {
    return `resume:${this.slug}` as const;
  }

  // ---------------------------------------------------------------- loading

  async show(route: Route) {
    if (route.kind !== "resume") return;
    if (route.slug === this.slug) return;
    this.slug = route.slug;
    this.hideBanner();
    await this.load(false);
    this.el.pageScroll.scrollTop = 0;
    this.editor.focus();
  }

  async leave() {
    await Promise.all([this.save.flush(), this.postingSave.flush()]);
  }

  /** Forget the open resume without saving (it was deleted). */
  close() {
    this.save.reset("");
    this.postingSave.reset("");
    this.slug = null;
  }

  private async load(keepPlace: boolean) {
    const slug = this.slug;
    if (!slug) return;
    const p = await api.readParts(this.file);
    if (slug !== this.slug) return;
    this.hideBanner();
    this.save.reset(p.hash);
    this.applyParts(p);
    const scroll = this.el.pageScroll.scrollTop;
    this.editor.set(p.body, keepPlace);
    if (keepPlace) this.el.pageScroll.scrollTop = scroll;
    $("stat-words").textContent = String(wordCount(stripComponents(p.body)));
    paintSaveState(this.el.saveState, "idle");
    await this.loadPosting();
    await this.refreshChecks();
  }

  private async loadPosting() {
    const f = await api.readPosting(this.slug!);
    this.postingHash = f.hash;
    this.postingSave.reset(f.hash);
    this.el.posting.value = f.content;
    paintSaveState(this.el.postingState, "idle");
  }

  private applyParts(p: Parts) {
    this.fields = p.fields;
    this.bodyOffset = p.body_offset;
    const summary = store.resumes.find((r) => r.slug === this.slug);
    if (document.activeElement !== this.el.title) this.el.title.value = str(p.fields.title) || this.slug || "";
    const status = STATUSES.includes(str(p.fields.status) as Status) ? str(p.fields.status) : "draft";
    this.el.status.value = status;
    this.el.status.dataset.status = status;
    this.renderTags();
    $("fact-slug").textContent = `resumes/${this.slug}/resume.mdx`;
    $("fact-updated").textContent = summary ? relTime(summary.updated_ms) : "just now";
  }

  private afterSave() {
    clearTimeout(this.checkTimer);
    this.checkTimer = window.setTimeout(() => {
      void this.refreshChecks();
      void app.refreshList();
    }, 250);
  }

  // ---------------------------------------------------------------- edits

  async setMeta(meta: { title?: string; status?: Status; tags?: string[] }) {
    if (!this.slug) return;
    await this.save.apply(async (hash) => {
      const p = await api.setMeta(this.slug!, meta, hash);
      this.applyParts(p);
      await app.refreshList();
      return p.hash;
    });
  }

  private renderTags() {
    const tags = list(this.fields.tags);
    const chips = tags.map((t) =>
      h(
        "span",
        { class: "tag" },
        t,
        h("button", {
          type: "button",
          title: `Remove ${t}`,
          "aria-label": `Remove tag ${t}`,
          textContent: "×",
          onclick: (e: Event) => {
            e.stopPropagation();
            void this.setMeta({ tags: tags.filter((x) => x !== t) });
          },
        }),
      ),
    );
    this.el.tags.replaceChildren(...chips, this.el.tagEntry);
  }

  // ---------------------------------------------------------------- disk changes & conflicts

  async onDiskChange() {
    if (!this.slug) return;
    const slug = this.slug;
    let disk: Parts;
    try {
      disk = await api.readParts(this.file);
    } catch {
      return; // deleted elsewhere: main.ts moves on once the list refreshes
    }
    if (slug !== this.slug) return;
    let changed = false;
    if (disk.hash !== this.save.hash && !this.save.busy) {
      changed = true;
      if (this.save.dirty) {
        this.save.markConflict();
      } else {
        await this.load(true);
        toast("Updated with changes made outside the editor.");
        return;
      }
    }
    const posting = await api.readPosting(slug);
    if (posting.hash !== this.postingHash && !this.postingSave.busy) {
      changed = true;
      if (this.postingSave.dirty) this.postingSave.markConflict();
      else await this.loadPosting();
    }
    if (changed) await this.refreshChecks();
  }

  private conflict(message: string, takeTheirs: () => Promise<void>, keepMine: () => Promise<void>) {
    const b = this.el.banner;
    b.hidden = false;
    b.replaceChildren(
      h("span", { class: "msg" }, message),
      h("button", { class: "btn small", type: "button", textContent: "Load their version", onclick: () => void takeTheirs().then(() => this.hideBanner()) }),
      h("button", { class: "btn small", type: "button", textContent: "Keep mine", onclick: () => void keepMine().then(() => this.hideBanner()) }),
    );
  }

  private hideBanner() {
    this.el.banner.hidden = true;
  }

  // ---------------------------------------------------------------- checks & target job

  async refreshChecks() {
    if (!this.slug) return;
    const slug = this.slug;
    try {
      const r = await api.checkResume(slug);
      if (slug === this.slug) this.renderReport(r);
    } catch {
      /* the resume vanished; the next poll handles it */
    }
  }

  private renderReport(r: Report) {
    $("stat-words").textContent = String(r.words);
    $("stat-bullets").textContent = `${r.bullets_with_numbers}/${r.bullets}`;
    $("stat-pages").textContent = `~${r.est_pages.toFixed(1)}`;

    const items = r.findings.map((f) => {
      const bodyLine = f.line ? f.line - this.bodyOffset : 0;
      const content = [h("span", { class: "sev", title: f.level }), h("span", {}, f.message, bodyLine > 0 && h("small", {}, `line ${bodyLine}`))];
      return h(
        "li",
        {},
        bodyLine > 0
          ? h("button", { type: "button", class: `finding ${f.level}`, title: "Go to line", onclick: () => this.gotoLine(bodyLine) }, ...content)
          : h("div", { class: `finding ${f.level}` }, ...content),
      );
    });
    if (!items.length) items.push(h("li", { class: "all-clear" }, icon("check"), "No issues found."));
    this.el.findings.replaceChildren(...items);

    const issues = r.findings.filter((f) => f.level !== "info").length;
    const reviewTab = this.section.querySelector<HTMLElement>('[data-tab="review"]')!;
    reviewTab.replaceChildren("Review", issues ? h("span", { class: "badge" }, String(issues)) : "");

    const k = r.keywords;
    $("coverage").hidden = !k;
    $("target-badge").textContent = k ? `${k.coverage}%` : "";
    if (!k) return;
    $("coverage-pct").textContent = `${k.coverage}%`;
    const bar = $("coverage-bar");
    bar.style.width = `${k.coverage}%`;
    bar.classList.toggle("good", k.coverage >= 60);
    const chips = (terms: string[], cls: string, none: string) =>
      terms.length ? terms.map((t) => h("span", { class: `kw ${cls}` }, t)) : [h("span", { class: "kw-none" }, none)];
    $("kw-missing").replaceChildren(...chips(k.missing, "missing", "None. Every key term appears."));
    $("kw-found").replaceChildren(...chips(k.found, "found", "None yet."));
  }

  private gotoLine(line: number) {
    this.editor.gotoLine(line);
  }

  // ---------------------------------------------------------------- layout

  toggleInspector(open = this.el.workarea.classList.contains("no-inspector")) {
    this.el.workarea.classList.toggle("no-inspector", !open);
    this.el.inspectorBtn.classList.toggle("on", open);
    pref.set("inspector", open ? "1" : "0");
  }

  private setTab(tab: Tab) {
    for (const b of this.section.querySelectorAll<HTMLElement>("[data-tab]")) {
      b.classList.toggle("on", b.dataset.tab === tab);
      b.setAttribute("aria-selected", String(b.dataset.tab === tab));
    }
    for (const p of this.section.querySelectorAll<HTMLElement>("[data-pane]")) p.hidden = p.dataset.pane !== tab;
    pref.set("tab", tab);
  }

  // ---------------------------------------------------------------- output

  async exportPdf() {
    if (!this.slug) return;
    if (!store.info?.pdf_export) return this.print();
    await this.leave();
    try {
      const path = await api.savePdf(this.slug);
      if (path) toast(`Saved ${path}`);
    } catch (e) {
      showError(`PDF not saved: ${e}`);
    }
  }

  async print() {
    if (!this.slug) return;
    await this.leave();
    await api.printResume(this.slug).catch(showError);
  }

  /** For the sidebar: how a resume reads in a list row. */
  static describe(words: number, tags: string[], updated: number) {
    return [relTime(updated), plural(words, "word"), ...tags].join(" · ");
  }
}
