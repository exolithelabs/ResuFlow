// Right-click menu for a live Markdown editor: formatting, MDX styling (colour, underline,
// font, rule colour) and the clipboard. Every choice writes visible Markdown or <Text>/<Rule>.
// Shift+right-click still opens the webview's own menu, for spelling suggestions.

import type { EditorView } from "@codemirror/view";
import { format, formatAt, type Block, type LiveEditor } from "./livemd";
import { h, icon, toast } from "./ui";

type Action = { label: string; key?: string; on?: boolean; disabled?: boolean; swatch?: string; run: (view: EditorView) => unknown };
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

const menu = h("div", { class: "menu edit-menu", role: "menu", "aria-label": "Format", hidden: true });
document.body.append(menu);
// Clicking the menu must not take focus from the editor, or the raw Markdown under it would hide and reflow.
menu.addEventListener("mousedown", (e) => e.preventDefault());

let current: EditorView | null = null;

const blockItem = (label: string, kind: Block, f: Block, key?: string): Item => ({ label, key, on: f === kind, run: format.block(kind) });

/** A row of colour swatches setting one attribute, with a "none" swatch that removes it. */
function swatches(shades: [string, string][], current: string | undefined, set: (c: string | null) => (v: EditorView) => unknown): Action[] {
  return [
    ...shades.map(([label, c]) => ({ label, swatch: c, on: current?.toLowerCase() === c, run: set(c) })),
    { label: "None", swatch: "none", on: !current, run: set(null) },
  ];
}

function items(view: EditorView): Item[] {
  const f = formatAt(view.state);
  const empty = view.state.selection.ranges.every((r) => r.empty);
  const t = f.text;
  const attr = (k: string) => t?.get(k);
  const isOn = (k: string) => attr(k) !== undefined && attr(k) !== "false";
  /** A font chip that sets `k` to `v`, or removes it when it already is. */
  const chip = (label: string, k: string, v = "true"): Action => {
    const active = v === "true" ? isOn(k) : attr(k) === v;
    return { label, on: active, run: format.text({ [k]: active ? null : v }) };
  };
  return [
    { label: "Bold", key: "Ctrl+B", on: f.bold, run: format.bold },
    { label: "Italic", key: "Ctrl+I", on: f.italic, run: format.italic },
    { label: "Underline", key: "Ctrl+U", on: isOn("underline") || !!attr("underlineColor"), run: format.underline },
    { label: "Strikethrough", on: f.strike, run: format.strike },
    { label: f.link ? "Remove link" : "Link", run: format.link },
    "-",
    { row: "Colour", chips: swatches(SHADES, attr("color"), (c) => format.text({ color: c })) },
    { row: "Underline", chips: swatches(SHADES, attr("underlineColor"), (c) => format.text({ underlineColor: c })) },
    {
      row: "Font",
      chips: [chip("Small", "size", "9.5pt"), chip("Large", "size", "12pt"), chip("Caps", "caps"), chip("Spaced", "spacing", "0.08em"), chip("Serif", "font", "serif"), chip("Mono", "font", "mono")],
    },
    { label: "Remove styling", disabled: !t, run: format.clearText },
    "-",
    blockItem("Section heading", "h2", f.block, "##"),
    blockItem("Role heading", "h3", f.block, "###"),
    blockItem("Bulleted list", "ul", f.block, "-"),
    blockItem("Numbered list", "ol", f.block, "1."),
    blockItem("Plain text", "p", f.block),
    { label: "Horizontal line", key: "---", run: format.rule },
    ...(f.rule ? [{ row: "Rule colour", chips: swatches(RULE_SHADES, f.rule.color, format.ruleColor) }] : []),
    "-",
    { label: "Apply resume style", run: format.resumeStyle },
    "-",
    { label: "Cut", key: "Ctrl+X", disabled: empty, run: (v) => copy(v, true) },
    { label: "Copy", key: "Ctrl+C", disabled: empty, run: (v) => copy(v, false) },
    { label: "Paste", key: "Ctrl+V", run: paste },
    { label: "Select all", key: "Ctrl+A", run: (v) => v.dispatch({ selection: { anchor: 0, head: v.state.doc.length } }) },
  ];
}

async function copy(view: EditorView, cut: boolean) {
  const text = view.state.selection.ranges.map((r) => view.state.sliceDoc(r.from, r.to)).join("\n");
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    return toast(`Couldn't reach the clipboard. Press Ctrl+${cut ? "X" : "C"} instead.`, { error: true });
  }
  if (cut) view.dispatch(view.state.replaceSelection(""), { userEvent: "delete.cut", scrollIntoView: true });
}

async function paste(view: EditorView) {
  let text: string;
  try {
    text = await navigator.clipboard.readText();
  } catch {
    return toast("Couldn't read the clipboard. Press Ctrl+V instead.", { error: true });
  }
  view.dispatch(view.state.replaceSelection(text), { userEvent: "input.paste", scrollIntoView: true });
}

function button(view: EditorView, it: Action, cls: string, ...children: (Node | string | false | undefined)[]) {
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
        void it.run(view);
      },
    },
    ...children,
  );
}

function render(view: EditorView) {
  const rows = items(view).map((it) => {
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
            if (!c.swatch) return button(view, c, "chip", c.label);
            const b = button(view, c, c.swatch === "none" ? "swatch none" : "swatch");
            if (c.swatch !== "none") b.style.setProperty("--swatch", c.swatch);
            return b;
          }),
        ),
      );
    }
    return button(view, it, "", h("span", { class: "menu-check" }, it.on && icon("check")), h("span", {}, it.label), it.key && h("span", { class: "menu-key" }, it.key));
  });
  menu.replaceChildren(...rows, h("div", { class: "menu-hint" }, "Shift+right-click for spelling"));
}

function open(view: EditorView, x: number, y: number, viaKeyboard: boolean) {
  close(false);
  current = view;
  render(view);
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
    if (e.shiftKey || view.state.readOnly) return;
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
      return open(view, e.clientX, e.clientY, false);
    }
    const at = view.coordsAtPos(view.state.selection.main.head);
    open(view, at?.left ?? 0, at?.bottom ?? 0, true);
  });
}
