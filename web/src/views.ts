// Workspace views: the default template, instructions, built-in skills, connecting an agent, and the trash.

import { api, type Skill, type TrashEntry } from "./api";
import { attachEditMenu } from "./editmenu";
import { createLiveEditor } from "./livemd";
import { app, store, type Route, type View } from "./store";
import { $, Autosave, copyText, h, icon, paintSaveState, relTime, showError, toast } from "./ui";

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
    $("skill-copy").onclick = (e) => this.current && void copyText(e.currentTarget as HTMLElement, this.current.content);
    $("install-skills").onclick = async () => {
      try {
        const dir = await api.installSkills();
        toast(`Installed ${this.skills.length} skills in ${dir}`);
      } catch (e) {
        showError(e);
      }
    };
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

// ================================================================ trash

export class TrashView implements View {
  section = $("view-trash");

  async show() {
    await this.render();
  }

  private async render(entries?: TrashEntry[]) {
    entries ??= await api.listTrash();
    $("count-trash").textContent = entries.length ? String(entries.length) : "";
    $("trash-list").replaceChildren(
      ...entries.map((t) =>
        h(
          "li",
          { class: "trash-item" },
          icon("doc"),
          h("div", {}, h("strong", {}, t.title), h("span", {}, `Deleted ${relTime(t.deleted_ms)} · ${t.slug}`)),
          h("button", { class: "btn small", type: "button", onclick: () => void restore(t) }, icon("restore"), "Restore"),
        ),
      ),
    );
  }

  async leave() {}

  async onDiskChange() {
    const entries = await api.listTrash();
    if (!this.section.hidden) await this.render(entries);
    else $("count-trash").textContent = entries.length ? String(entries.length) : "";
  }
}

/** Bring a resume back from the trash and open it. */
export async function restore(t: TrashEntry) {
  try {
    const slug = await api.restoreResume(t.id);
    await app.refreshList();
    await app.navigate({ kind: "resume", slug });
    toast(slug === t.slug ? `Restored "${t.title}".` : `Restored "${t.title}" as ${slug}.`);
  } catch (e) {
    showError(e);
  }
}
