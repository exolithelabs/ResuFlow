// The app's only right-click menu. In a live Markdown editor it offers formatting, MDX styling
// (colour, underline, font, rule colour) and the clipboard; every choice writes visible Markdown or
// <Text>/<Rule>. In other text fields, and over selected text, it offers just the clipboard. The
// webview's own menu (Reload, Inspect, Back) never opens.

import type { EditorView } from "@codemirror/view";
import { format, formatAt, type Block, type LiveEditor } from "./livemd";
import { h, icon, toast } from "./ui";

type Action = { label: string; key?: string; on?: boolean; disabled?: boolean; swatch?: string; run: () => unknown };
type Command = (view: EditorView) => unknown;
type Field = HTMLInputElement | HTMLTextAreaElement;
/** A separator, a menu row, or a labelled row of small buttons (colour swatches, font chips). */
type Item = "-" | Action | { row: string; chips: Action[] };

/** Text colours: the template's ink, soft greys and a few muted accents that print well. */
const SHADES: [string, string][] = [
  ["Ink", "#16181d"], ["Muted grey", "#555b66"], ["Soft grey", "#8a919c"], ["Blue", "#1f4e8c"], ["Teal", "#0f766e"],
  ["Green", "#2e7d32"], ["Amber", "#b45309"], ["Red", "#b42318"], ["Purple", "#6d28d9"],
];
const RULE_SHADES: [string, string][] = [
  ["Soft grey", "#d6d9de"], ["Grey", "#8a919c"], ["Blue", "#1f4e8c"], ["Teal", "#0f766e"], ["Red", "#b42318"], ["Ink", "#16181d"],
];

const menu = h("div", { class: "menu edit-menu", role: "menu", "aria-label": "Edit", hidden: true });
document.body.append(menu);
// Clicking the menu must not take focus from the editor, or the raw Markdown under it would hide and reflow.
menu.addEventListener("mousedown", (e) => e.preventDefault());

/** What gets focus back when the menu closes. */
let current: { focus(): void } | null = null;

function editorItems(view: EditorView): Item[] {
  const f = formatAt(view.state);
  const on = (cmd: Command) => () => cmd(view);
  const blockItem = (label: string, kind: Block, key?: string): Item => ({ label, key, on: f.block === kind, run: on(format.block(kind)) });
  /** A row of colour swatches setting one attribute, with a "none" swatch that removes it. */
  const swatches = (shades: [string, string][], current: string | undefined, set: (c: string | null) => Command): Action[] => [
    ...shades.map(([label, c]) => ({ label, swatch: c, on: current?.toLowerCase() === c, run: on(set(c)) })),
    { label: "None", swatch: "none", on: !current, run: on(set(null)) },
  ];
  const empty = view.state.selection.ranges.every((r) => r.empty);
  const t = f.text;
  const attr = (k: string) => t?.get(k);
  const isOn = (k: string) => attr(k) !== undefined && attr(k) !== "false";
  /** A font chip that sets `k` to `v`, or removes it when it already is. */
  const chip = (label: string, k: string, v = "true"): Action => {
    const active = v === "true" ? isOn(k) : attr(k) === v;
    return { label, on: active, run: on(format.text({ [k]: active ? null : v })) };
  };
  return [
    { label: "Bold", key: "Ctrl+B", on: f.bold, run: on(format.bold) },
    { label: "Italic", key: "Ctrl+I", on: f.italic, run: on(format.italic) },
    { label: "Underline", key: "Ctrl+U", on: isOn("underline") || !!attr("underlineColor"), run: on(format.underline) },
    { label: "Strikethrough", on: f.strike, run: on(format.strike) },
    { label: f.link ? "Remove link" : "Link", run: on(format.link) },
    "-",
    { row: "Colour", chips: swatches(SHADES, attr("color"), (c) => format.text({ color: c })) },
    { row: "Underline", chips: swatches(SHADES, attr("underlineColor"), (c) => format.text({ underlineColor: c })) },
    {
      row: "Font",
      chips: [chip("Small", "size", "9.5pt"), chip("Large", "size", "12pt"), chip("Caps", "caps"), chip("Spaced", "spacing", "0.08em"), chip("Serif", "font", "serif"), chip("Mono", "font", "mono")],
    },
    { label: "Remove styling", disabled: !t, run: on(format.clearText) },
    "-",
    blockItem("Section heading", "h2", "##"),
    blockItem("Role heading", "h3", "###"),
    blockItem("Bulleted list", "ul", "-"),
    blockItem("Numbered list", "ol", "1."),
    blockItem("Plain text", "p"),
    { label: "Horizontal line", key: "---", run: on(format.rule) },
    ...(f.rule ? [{ row: "Rule colour", chips: swatches(RULE_SHADES, f.rule.color, format.ruleColor) }] : []),
    "-",
    { label: "Apply resume style", run: on(format.resumeStyle) },
    "-",
    ...clipboardItems(
      empty,
      view.state.readOnly,
      () => view.state.selection.ranges.map((r) => view.state.sliceDoc(r.from, r.to)).join("\n"),
      (text) => view.dispatch(view.state.replaceSelection(text), { userEvent: text ? "input.paste" : "delete.cut", scrollIntoView: true }),
      () => view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } }),
    ),
  ];
}

/** Cut, Copy, Paste and Select all over some selection; `replace("")` deletes it. */
function clipboardItems(empty: boolean, readOnly: boolean, selected: () => string, replace: (text: string) => unknown, selectAll: () => unknown): Action[] {
  return [
    { label: "Cut", key: "Ctrl+X", disabled: empty || readOnly, run: async () => (await writeClipboard(selected(), "X")) && replace("") },
    { label: "Copy", key: "Ctrl+C", disabled: empty, run: () => writeClipboard(selected(), "C") },
    { label: "Paste", key: "Ctrl+V", disabled: readOnly, run: async () => {
      const text = await readClipboard();
      if (text !== null) replace(text);
    } },
    { label: "Select all", key: "Ctrl+A", run: selectAll },
  ];
}

function fieldItems(el: Field): Item[] {
  const [from, to] = [el.selectionStart ?? 0, el.selectionEnd ?? 0];
  return clipboardItems(from === to, el.readOnly || el.disabled, () => el.value.slice(from, to), (text) => insertInto(el, text), () => el.select());
}

/** Read-only text, such as the rendered preview or a config snippet: copy only. */
function selectionItems(): Item[] {
  const text = getSelection()?.toString() ?? "";
  return text ? [{ label: "Copy", key: "Ctrl+C", run: () => writeClipboard(text, "C") }] : [];
}

/** Replace the field's selection the way typing would, so undo and the field's input listeners still work. */
function insertInto(el: Field, text: string) {
  el.focus();
  if (document.execCommand(text ? "insertText" : "delete", false, text)) return;
  el.setRangeText(text, el.selectionStart ?? 0, el.selectionEnd ?? 0, "end");
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

async function writeClipboard(text: string, key: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    toast(`Couldn't reach the clipboard. Press Ctrl+${key} instead.`, { error: true });
    return false;
  }
}

async function readClipboard(): Promise<string | null> {
  try {
    return await navigator.clipboard.readText();
  } catch {
    toast("Couldn't read the clipboard. Press Ctrl+V instead.", { error: true });
    return null;
  }
}

function button(it: Action, cls: string, ...children: (Node | string | false | undefined)[]) {
  return h(
    "button",
    {
      type: "button",
      class: `menu-item ${cls}${it.on ? " on" : ""}`,
      role: it.on === undefined ? "menuitem" : "menuitemcheckbox",
      "aria-checked": it.on === undefined ? undefined : String(it.on),
      "aria-label": it.swatch ? it.label : undefined,
      title: it.swatch ? it.label : undefined,
      disabled: it.disabled,
      onclick: () => {
        close(true);
        void it.run();
      },
    },
    ...children,
  );
}

function render(list: Item[]) {
  const rows = list.map((it) => {
    if (it === "-") return h("div", { class: "menu-sep", role: "separator" });
    if ("row" in it) {
      return h(
        "div",
        { class: "menu-row", role: "group", "aria-label": it.row },
        h("span", { class: "menu-row-label" }, it.row),
        h(
          "div",
          { class: "menu-chips" },
          ...it.chips.map((c) => {
            if (!c.swatch) return button(c, "chip", c.label);
            const b = button(c, c.swatch === "none" ? "swatch none" : "swatch");
            if (c.swatch !== "none") b.style.setProperty("--swatch", c.swatch);
            return b;
          }),
        ),
      );
    }
    return button(it, "", h("span", { class: "menu-check" }, it.on && icon("check")), h("span", {}, it.label), it.key && h("span", { class: "menu-key" }, it.key));
  });
  menu.replaceChildren(...rows);
}

function open(target: { focus(): void } | null, list: Item[], x: number, y: number, viaKeyboard: boolean) {
  close(false);
  current = target;
  render(list);
  // Swatch and chip rows need the wide formatting layout; a clipboard-only menu stays narrow.
  menu.classList.toggle("edit-menu", list.some((it) => typeof it === "object" && "row" in it));
  menu.hidden = false;
  menu.style.left = `${Math.max(8, Math.min(x, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, innerHeight - menu.offsetHeight - 8))}px`;
  addEventListener("keydown", onKey, true);
  addEventListener("mousedown", onOutside, true);
  addEventListener("scroll", onScroll, true);
  addEventListener("resize", onScroll);
  addEventListener("blur", onScroll);
  if (viaKeyboard) step(1);
}

function close(refocus: boolean) {
  if (menu.hidden) return;
  menu.hidden = true;
  removeEventListener("keydown", onKey, true);
  removeEventListener("mousedown", onOutside, true);
  removeEventListener("scroll", onScroll, true);
  removeEventListener("resize", onScroll);
  removeEventListener("blur", onScroll);
  if (refocus) current?.focus();
  current = null;
}

/** Move focus through the enabled items, wrapping around. */
function step(by: 1 | -1) {
  const list = [...menu.querySelectorAll<HTMLButtonElement>(".menu-item:not(:disabled)")];
  const i = list.indexOf(document.activeElement as HTMLButtonElement);
  list[i < 0 ? (by > 0 ? 0 : list.length - 1) : (i + by + list.length) % list.length]?.focus();
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    close(true);
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Tab") {
    e.preventDefault();
    e.stopPropagation();
    step(e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey) ? -1 : 1);
  } else if (!((e.key === "Enter" || e.key === " ") && menu.contains(document.activeElement))) {
    close(true); // anything else goes on to the editor as usual
  }
}

const onOutside = (e: MouseEvent) => !menu.contains(e.target as Node) && close(false);
const onScroll = (e: Event) => !(e.target instanceof Node && menu.contains(e.target)) && close(false);

/** Give an editor the formatting menu on right-click and the context-menu key. */
export function attachEditMenu(editor: LiveEditor) {
  const view = editor.view;
  view.dom.addEventListener("contextmenu", (e) => {
    if (view.state.readOnly) return; // the app-wide menu below offers Copy
    e.preventDefault();
    // The context-menu key (or Shift+F10) reports no pointer position: open at the cursor.
    const viaKeyboard = e.button !== 2 && !e.clientX && !e.clientY;
    if (!viaKeyboard) {
      // Right-clicking outside the selection moves the cursor there first, as in other editors.
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
      if (pos !== null && !view.state.selection.ranges.some((r) => pos >= r.from && pos <= r.to)) {
        view.dispatch({ selection: { anchor: pos } });
      }
      view.focus();
      return open(view, editorItems(view), e.clientX, e.clientY, false);
    }
    const at = view.coordsAtPos(view.state.selection.main.head);
    open(view, editorItems(view), at?.left ?? 0, at?.bottom ?? 0, true);
  });
}

function textField(t: EventTarget | null): Field | null {
  if (t instanceof HTMLTextAreaElement) return t;
  return t instanceof HTMLInputElement && ["text", "search", "url", "email", "tel"].includes(t.type) ? t : null;
}

// Everywhere else: a clipboard menu over text fields and selected text, and nothing at all elsewhere.
// Menus of their own (the editor's, a resume row's) have already called preventDefault.
addEventListener("contextmenu", (e) => {
  if (e.defaultPrevented) return;
  e.preventDefault();
  const field = textField(e.target);
  const list = field ? fieldItems(field) : selectionItems();
  if (!list.length) return;
  const viaKeyboard = e.button !== 2 && !e.clientX && !e.clientY;
  if (!viaKeyboard) return open(field, list, e.clientX, e.clientY, false);
  const at = (e.target as Element).getBoundingClientRect();
  open(field, list, at.left, at.bottom, true);
});
