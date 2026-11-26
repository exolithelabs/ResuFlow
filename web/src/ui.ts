// Small UI toolkit: element helpers, icons, toasts, dialogs and hash-guarded autosave.

import { isConflict } from "./api";

export const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

type Child = Node | string | null | undefined | false;
type Props = { [k: string]: unknown; class?: string; dataset?: Record<string, string> };

/** Create an element. Props starting with "on" become listeners; others become properties or attributes. */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Props = {}, ...children: Child[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") el.className = String(v);
    else if (k === "dataset") Object.assign(el.dataset, v);
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v as EventListener);
    else if (k in el && !k.includes("-")) (el as unknown as Record<string, unknown>)[k] = v;
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) if (c) el.append(c);
  return el;
}

// ---------------------------------------------------------------- icons (16px, stroked)

const PATHS = {
  plus: "M8 3v10M3 8h10",
  search: "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM14 14l-3.5-3.5",
  template: "M3 2.5h10v11H3zM5.5 5.5h5M5.5 8h5M5.5 10.5h3",
  rules: "M5 4h8M5 8h8M5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01",
  spark: "M8 1.5l1.6 4.9 4.9 1.6-4.9 1.6L8 14.5l-1.6-4.9L1.5 8l4.9-1.6z",
  plug: "M6 1.5v3M10 1.5v3M4 4.5h8v3a4 4 0 0 1-8 0zM8 11.5v3",
  trash: "M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4",
  more: "M3.5 8h.01M8 8h.01M12.5 8h.01",
  panel: "M2 3h12v10H2zM10 3v10",
  download: "M8 2v8M4.5 6.5 8 10l3.5-3.5M2.5 13.5h11",
  printer: "M4 6V2h8v4M4 11.5H2.5V6h11v5.5H12M4 9.5h8v4.5H4z",
  copy: "M5.5 5.5h8v8h-8zM10.5 5.5v-3h-8v8h3",
  check: "M3 8.5l3 3 7-7",
  x: "M4 4l8 8M12 4l-8 8",
  restore: "M2.5 8a5.5 5.5 0 1 0 1.6-3.9M2.5 2v3.5H6",
  doc: "M4 1.5h5.5L12.5 4.5v10h-8.5zM9.5 1.5v3h3",
  folder: "M1.5 3.5h4.5l1.5 1.5h7v8h-13z",
  target: "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM8 8h.01",
  duplicate: "M5.5 5.5h8v8h-8zM10.5 5.5v-3h-8v8h3M9.5 7.5v4M7.5 9.5h4",
  archive: "M1.5 2.5h13v3h-13zM2.5 5.5v8h11v-8M6 8.5h4",
  info: "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 7.5v4M8 5h.01",
} as const;
export type IconName = keyof typeof PATHS;

export function icon(name: IconName, cls = "icon"): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("class", cls);
  svg.setAttribute("aria-hidden", "true");
  const p = document.createElementNS(ns, "path");
  p.setAttribute("d", PATHS[name]);
  svg.append(p);
  return svg;
}

/** Replace `[data-icon]` placeholders in static markup with SVG icons. */
export function hydrateIcons(root: ParentNode = document) {
  for (const el of root.querySelectorAll<HTMLElement>("[data-icon]")) {
    el.replaceWith(icon(el.dataset.icon as IconName));
  }
}

// ---------------------------------------------------------------- toasts

export interface ToastOptions {
  error?: boolean;
  action?: { label: string; run: () => void | Promise<void> };
  ms?: number;
}

export function toast(message: string, opts: ToastOptions = {}) {
  const host = $("toasts");
  const close = () => {
    t.classList.add("leaving");
    setTimeout(() => t.remove(), 160);
  };
  const t = h(
    "div",
    { class: "toast" + (opts.error ? " error" : ""), role: opts.error ? "alert" : "status" },
    h("span", { class: "toast-text" }, message),
    opts.action &&
      h("button", {
        class: "btn ghost small",
        type: "button",
        textContent: opts.action.label,
        onclick: () => {
          close();
          void opts.action!.run();
        },
      }),
    h("button", { class: "icon-btn", type: "button", title: "Dismiss", "aria-label": "Dismiss", onclick: close }, icon("x")),
  );
  host.append(t);
  while (host.children.length > 3) host.firstElementChild?.remove();
  setTimeout(close, opts.ms ?? (opts.action ? 7000 : opts.error ? 6000 : 3000));
}

export const showError = (e: unknown) => toast(String(e).replace(/^Error: /, ""), { error: true });

// ---------------------------------------------------------------- dialogs

/** Show a modal `<dialog>` containing a `<form method="dialog">` and resolve with its return value. */
export function runDialog(dlg: HTMLDialogElement): Promise<string> {
  dlg.returnValue = "";
  dlg.showModal();
  return new Promise((res) => dlg.addEventListener("close", () => res(dlg.returnValue), { once: true }));
}

export async function copyText(btn: HTMLElement, text: string) {
  await navigator.clipboard.writeText(text);
  btn.classList.add("copied");
  const label = btn.querySelector(".label");
  const before = label?.textContent;
  if (label) label.textContent = "Copied";
  setTimeout(() => {
    btn.classList.remove("copied");
    if (label && before) label.textContent = before;
  }, 1400);
}

// ---------------------------------------------------------------- formatting

export function relTime(ms: number): string {
  const s = (Date.now() - ms) / 1000;
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  const days = Math.floor(s / 86400);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short", year: days > 300 ? "numeric" : undefined });
}

export const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
export const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export function wordCount(text: string) {
  return text.split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w)).length;
}

// ---------------------------------------------------------------- autosave

export type SaveState = "idle" | "editing" | "saving" | "saved" | "conflict" | "error";

const SAVE_TEXT: Record<SaveState, string> = {
  idle: "",
  editing: "Edited",
  saving: "Saving…",
  saved: "Saved",
  conflict: "Changed elsewhere",
  error: "Not saved",
};

/** Render a save state into a small status element. */
export function paintSaveState(el: HTMLElement, s: SaveState, detail?: string) {
  el.dataset.state = s;
  el.textContent = SAVE_TEXT[s];
  el.title = detail ?? "";
}

/**
 * Debounced, serialised, hash-guarded saving for one file.
 *
 * `write(hash)` saves the current content and resolves to the new hash. A `conflict:` error
 * (the file changed on disk since it was read) stops autosaving until the caller resolves it
 * with `reset` (take theirs) or `flush(true)` (keep mine).
 */
export class Autosave {
  hash = "";
  dirty = false;
  conflict = false;
  private timer = 0;
  private running: Promise<void> | null = null;

  constructor(
    private o: {
      write: (hash: string | undefined) => Promise<string>;
      state: (s: SaveState, detail?: string) => void;
      onConflict: () => void;
      delay?: number;
    },
  ) {}

  /** Start over with a freshly read file. */
  reset(hash: string) {
    clearTimeout(this.timer);
    this.hash = hash;
    this.dirty = false;
    this.conflict = false;
  }

  touch() {
    this.dirty = true;
    if (this.conflict) return;
    this.o.state("editing");
    clearTimeout(this.timer);
    this.timer = window.setTimeout(() => void this.flush(), this.o.delay ?? 500);
  }

  get busy() {
    return this.running !== null;
  }

  /** The file changed on disk while there were unsaved edits. */
  markConflict() {
    clearTimeout(this.timer);
    this.conflict = true;
    this.o.state("conflict");
    this.o.onConflict();
  }

  /** Save now if there are unsaved changes. `force` overwrites a conflicting file. */
  async flush(force = false) {
    clearTimeout(this.timer);
    while (this.running) await this.running;
    if (!this.dirty || (this.conflict && !force)) return;
    await this.run(async (hash) => {
      this.dirty = false; // edits made while saving set it again
      return this.o.write(force ? undefined : hash);
    });
  }

  /** Run another guarded write (a header field, say) after pending edits are saved. */
  async apply(write: (hash: string) => Promise<string>) {
    await this.flush();
    while (this.running) await this.running;
    if (this.conflict) return;
    await this.run(write);
  }

  private async run(write: (hash: string) => Promise<string>) {
    this.running = (async () => {
      this.o.state("saving");
      try {
        this.hash = await write(this.hash);
        this.conflict = false;
        this.o.state(this.dirty ? "editing" : "saved");
      } catch (e) {
        this.dirty = true;
        if (isConflict(e)) {
          this.conflict = true;
          this.o.state("conflict");
          this.o.onConflict();
        } else {
          this.o.state("error", String(e));
        }
      }
    })();
    try {
      await this.running;
    } finally {
      this.running = null;
    }
  }
}
