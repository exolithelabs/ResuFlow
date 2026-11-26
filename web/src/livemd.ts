// Live-preview Markdown on CodeMirror 6, in the spirit of Obsidian.
// The text stays plain Markdown. Formatting renders in place, and the raw syntax
// shows only where the cursor is: block syntax (##, -, ---) on the cursor's lines, inline
// syntax (**, `, [](), <Text>) only while the cursor touches that span, so it folds away as
// soon as you type past the closing mark. Everything on the page,
// the header included, is this one text: Markdown plus the two MDX styling components that
// render.rs prints, <Text color underline underlineColor size caps spacing font>…</Text> and <Rule color />.

import { Compartment, EditorSelection, EditorState, Transaction, type Extension, type Range, type Text } from "@codemirror/state";
import {
  Decoration, EditorView, ViewPlugin, WidgetType, keymap, placeholder as placeholderExt,
  type Command, type DecorationSet, type ViewUpdate,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { Language, LanguageSupport, defineLanguageFacet, syntaxTree } from "@codemirror/language";
import { GFM, parser as mdParser, type MarkdownConfig } from "@lezer/markdown";

type SyntaxNode = ReturnType<ReturnType<typeof syntaxTree>["resolveInner"]>;

// ---------------------------------------------------------------- MDX components (render.rs `component`)

/** What a component tag asks for: the CSS of a `<Text>` or `<Rule />`, or the end of a `<Text>`. */
type Component = { kind: "text" | "rule"; css: string } | { kind: "end" };

/** CSS colours allowed into a style attribute (render.rs `css_color`). */
const CSS_COLOR = /^(?:#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})|[a-z]{1,32}|(?:rgba?|hsla?)\([\d .,%/deg-]*\))$/i;
/** CSS lengths allowed into a style attribute (render.rs `css_length`). */
const CSS_LENGTH = /^-?\d{1,3}(?:\.\d{1,3})?(?:rem|pt|px|em|%)$/;
/** The fonts a `<Text font="…">` can ask for (render.rs `font_family`). */
const FONTS: Record<string, string> = {
  serif: 'Georgia, "Times New Roman", serif',
  sans: '"Inter", "Segoe UI", "Helvetica Neue", Arial, sans-serif',
  mono: 'ui-monospace, "SF Mono", Menlo, monospace',
};
const on = (v: string | undefined) => v !== undefined && v !== "false";

/** An opening tag's name and attributes, first one winning; a bare attribute (`caps`) reads as "true" (render.rs `component`). */
function parseTag(tag: string): { name: string; attrs: Map<string, string>; selfClosing: boolean } | null {
  const t = tag.trim();
  if (t.length < 2 || !t.startsWith("<") || !t.endsWith(">")) return null;
  let inner = t.slice(1, -1);
  const selfClosing = inner.endsWith("/");
  if (selfClosing) inner = inner.slice(0, -1);
  const name = /^[A-Za-z0-9]*/.exec(inner)![0];
  const attrs = new Map<string, string>();
  let rest = inner.slice(name.length);
  for (;;) {
    rest = rest.trimStart();
    if (!rest) break;
    const key = /^[A-Za-z0-9_-]*/.exec(rest)![0];
    if (!key) return null;
    const after = rest.slice(key.length).trimStart();
    let value = "true";
    rest = after;
    if (after.startsWith("=")) {
      const v = after.slice(1).trimStart();
      const end = v[0] === '"' || v[0] === "'" ? v.indexOf(v[0], 1) : -1;
      if (end < 0) return null;
      value = v.slice(1, end);
      rest = v.slice(end + 1);
    }
    if (!attrs.has(key)) attrs.set(key, value);
  }
  return { name, attrs, selfClosing };
}

/** Read one tag, such as `<Text color="red">`. Anything that is not exactly a component tag is null. */
export function component(tag: string): Component | null {
  const t = tag.trim();
  if (/^<\/\s*Text\s*>$/.test(t)) return { kind: "end" };
  const m = parseTag(t);
  if (!m) return null;
  const { attrs } = m;
  const checked = (k: string, re: RegExp) => {
    const v = attrs.get(k)?.trim();
    return v && re.test(v) ? v : undefined;
  };
  const color = (k: string) => checked(k, CSS_COLOR);
  if (m.name === "Text" && !m.selfClosing) {
    const css: string[] = [];
    const c = color("color");
    const underlineColor = color("underlineColor");
    const size = checked("size", CSS_LENGTH);
    const spacing = checked("spacing", CSS_LENGTH);
    const font = FONTS[attrs.get("font")?.trim() ?? ""];
    if (c) css.push(`color: ${c}`);
    if (on(attrs.get("underline")) || underlineColor) css.push("text-decoration-line: underline");
    if (underlineColor) css.push(`text-decoration-color: ${underlineColor}`);
    if (size) css.push(`font-size: ${size}`);
    if (on(attrs.get("caps"))) css.push("text-transform: uppercase");
    if (spacing) css.push(`letter-spacing: ${spacing}`);
    if (font) css.push(`font-family: ${font}`);
    return { kind: "text", css: css.join("; ") };
  }
  if (m.name === "Rule" && m.selfClosing) {
    const c = color("color");
    return { kind: "rule", css: c ? `border-top-color: ${c}` : "" };
  }
  return null;
}

const isRuleLine = (text: string) => component(text)?.kind === "rule";

/** The text with component tags removed, for counting words (render.rs `strip_components`). */
export function stripComponents(text: string) {
  let out = "";
  let rest = text;
  for (let i = rest.indexOf("<"); i >= 0; i = rest.indexOf("<")) {
    out += rest.slice(0, i);
    const e = rest.indexOf(">", i) + 1;
    if (e > 0 && component(rest.slice(i, e))) rest = rest.slice(e);
    else {
      out += "<";
      rest = rest.slice(i + 1);
    }
  }
  return out + rest;
}

/** A `<Rule />` alone on an unindented line is a block of its own, even straight under text (render.rs `isolate_rules`). */
const ruleBlock: MarkdownConfig = {
  defineNodes: [{ name: "ComponentRule", block: true }],
  parseBlock: [
    {
      name: "ComponentRule",
      before: "HTMLBlock",
      parse(cx, line) {
        if (line.pos !== 0 || !isRuleLine(line.text)) return false;
        cx.addElement(cx.elt("ComponentRule", cx.lineStart, cx.lineStart + line.text.length));
        cx.nextLine();
        return true;
      },
      endLeaf: (_cx, line) => line.pos === 0 && isRuleLine(line.text),
    },
  ],
};

// Markdown with GitHub extensions and the rule component, without the HTML/JS/CSS
// sub-languages that @codemirror/lang-markdown would pull in.
const markdown = () => new LanguageSupport(new Language(defineLanguageFacet(), mdParser.configure([GFM, ruleBlock]), [], "markdown"));

class BulletWidget extends WidgetType {
  eq() {
    return true;
  }
  // Let clicks through, so clicking the bullet puts the cursor there like clicking text does.
  ignoreEvent() {
    return false;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "md-bullet";
    s.textContent = "•";
    return s;
  }
}

/** A horizontal rule drawn as a line with room around it to click into. `css` colours a <Rule />. */
class RuleWidget extends WidgetType {
  constructor(readonly css: string) {
    super();
  }
  eq(other: RuleWidget) {
    return other.css === this.css;
  }
  ignoreEvent() {
    return false; // a click places the cursor on the rule, which reveals its source
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "md-rule";
    s.style.cssText = this.css;
    return s;
  }
}

const bullet = Decoration.replace({ widget: new BulletWidget() });
const rule = (css = "") => Decoration.replace({ widget: new RuleWidget(css) });
const styled = (css: string) => Decoration.mark({ attributes: { style: css } });
/** Blocks a <Text> can style; one left open styles the rest of its block (render.rs `markdown`). */
const TEXT_BLOCK = /^(?:Paragraph|ATXHeading\d|SetextHeading\d|TableCell)$/;
const hide = Decoration.replace({});
const mark = (cls: string) => Decoration.mark({ class: cls });
const lineDeco = (cls: string) => Decoration.line({ class: cls });

const INLINE: Record<string, string> = {
  Emphasis: "md-em",
  StrongEmphasis: "md-strong",
  InlineCode: "md-code",
  Strikethrough: "md-strike",
  Link: "md-link",
};
const SYNTAX = new Set(["EmphasisMark", "CodeMark", "StrikethroughMark", "LinkMark", "URL", "QuoteMark"]);
const BLOCK_LINES: Record<string, string> = {
  Blockquote: "md-quote",
  FencedCode: "md-codeblock",
  CodeBlock: "md-codeblock",
  Table: "md-table",
};

/** A line opening a Markdown block other than a paragraph (render.rs `starts_block`). */
const STARTS_BLOCK = /^(?:#|>|\||```|~~~|[-*+]\s|\d+[.)]\s|([-*_])(?:\s*\1){2,}\s*$)/;
const looksLikeContact = (t: string) => /·| \| |@|\]\(|:\/\//.test(t) || (t.match(/\d/g)?.length ?? 0) >= 7;

/**
 * The resume header: the `# ` name line opening the document and the plain lines under it,
 * classified as in render.rs `split_header`. They look like any other text; this only matters
 * for a `---` straight under them, which is a rule rather than a heading underline.
 */
function headerLines(doc: Text): { name: number; headline: number[]; contact: number[] } | null {
  let n = 1;
  while (n <= doc.lines && !doc.line(n).text.trim()) n++;
  if (n > doc.lines || !doc.line(n).text.startsWith("# ")) return null;
  const name = n;
  const found: number[] = [];
  for (n++; n <= doc.lines && found.length < 12; n++) {
    const t = doc.line(n).text.trim();
    if (!t && !found.length) continue;
    if (!t || STARTS_BLOCK.test(t) || isRuleLine(t)) break;
    found.push(n);
  }
  if (found.length === 1) {
    const contact = looksLikeContact(doc.line(found[0]).text);
    return { name, headline: contact ? [] : found, contact: contact ? found : [] };
  }
  return { name, headline: found.slice(0, 1), contact: found.slice(1) };
}

function build(view: EditorView, reveal: boolean, header: boolean): DecorationSet {
  const { state } = view;
  const doc = state.doc;
  const active = new Set<number>();
  if (reveal && view.hasFocus) {
    for (const r of state.selection.ranges) {
      for (let n = doc.lineAt(r.from).number; n <= doc.lineAt(r.to).number; n++) active.add(n);
    }
  }
  const isActive = (pos: number) => active.has(doc.lineAt(pos).number);
  // Inline syntax shows while the selection touches its span, the edges included.
  const touches = (from: number, to: number) =>
    reveal && view.hasFocus && state.selection.ranges.some((r) => r.from <= to && r.to >= from);
  // The end of a mark plus the single space after it ("## ", "- "), so both hide together.
  const withSpace = (to: number) => (doc.sliceString(to, to + 1) === " " ? to + 1 : to);
  const out: Range<Decoration>[] = [];
  const eachLine = (from: number, to: number, cls: string) => {
    for (let p = from; p <= to; ) {
      const l = doc.lineAt(p);
      out.push(lineDeco(cls).range(l.from));
      p = l.to + 1;
    }
  };

  const headerEnd = header ? headerEndLine(doc) : 0;
  const inHeader = (pos: number) => doc.lineAt(pos).number <= headerEnd;
  const drawRule = (from: number, to: number, css = "") => {
    // Off the cursor: a drawn rule. On it: the raw --- or <Rule /> to edit.
    if (isActive(from)) out.push(mark("md-mark").range(from, to));
    else out.push(lineDeco("md-hr").range(doc.lineAt(from).from), rule(css).range(from, to));
  };
  // Component tags hide like other syntax, and show faded while the cursor is in their span.
  const tag = (from: number, to: number, shown: boolean) => {
    out.push((shown ? mark("md-mark") : hide).range(from, to));
  };
  // <Text> spans waiting for their </Text>: the tag, where the styled words start, and where their block ends.
  let open: { tag: number; from: number; css: string; end: number }[] = [];
  const style = (from: number, to: number, css: string) => css && to > from && out.push(styled(css).range(from, to));
  const closeBefore = (pos: number) => {
    for (const o of open.filter((o) => o.end <= pos)) {
      style(o.from, o.end, o.css);
      tag(o.tag, o.from, touches(o.tag, o.end));
    }
    open = open.filter((o) => o.end > pos);
  };

  for (const { from, to } of view.visibleRanges) {
    for (let p = from; p <= to; ) {
      const l = doc.lineAt(p);
      if (l.length === 0 && !active.has(l.number)) out.push(lineDeco("md-blank").range(l.from));
      p = l.to + 1;
    }
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        const heading = /^(?:ATX|Setext)Heading(\d)$/.exec(name);
        // The header's lines are not a paragraph: a --- right under them is a rule, not a heading underline.
        const headerSetext = name.startsWith("Setext") && inHeader(node.from);
        if (heading && !headerSetext) {
          out.push(lineDeco(`md-h md-h${heading[1]}`).range(doc.lineAt(node.from).from));
        } else if (name === "HeaderMark") {
          if (node.node.parent?.name.startsWith("Setext") && inHeader(node.node.parent.from)) drawRule(node.from, node.to);
          else if (!isActive(node.from)) out.push(hide.range(node.from, withSpace(node.to)));
        } else if (INLINE[name]) {
          if (node.to > node.from) out.push(mark(INLINE[name]).range(node.from, node.to));
        } else if (SYNTAX.has(name)) {
          // In a link only the brackets and URL hide; a bare autolink keeps its URL.
          const parent = node.node.parent;
          if (name === "URL" && parent?.name !== "Link") return;
          // A quote's > is block syntax; the rest belong to the inline span around them.
          const shown = name === "QuoteMark" || !parent ? isActive(node.from) : touches(parent.from, parent.to);
          if (!shown && node.to > node.from) out.push(hide.range(node.from, node.to));
        } else if (name === "ListItem") {
          out.push(lineDeco("md-li").range(doc.lineAt(node.from).from));
        } else if (name === "ListMark") {
          const bulletMark = /^[-*+]$/.test(doc.sliceString(node.from, node.to));
          if (bulletMark && !isActive(node.from)) out.push(bullet.range(node.from, withSpace(node.to)));
          else out.push(mark("md-listmark").range(node.from, node.to));
        } else if (name === "HorizontalRule") {
          drawRule(node.from, node.to);
        } else if (name === "ComponentRule") {
          const c = component(doc.sliceString(node.from, node.to));
          drawRule(node.from, node.to, c?.kind === "rule" ? c.css : "");
        } else if (name === "HTMLTag") {
          closeBefore(node.from);
          const c = component(doc.sliceString(node.from, node.to));
          if (!c) return; // other HTML stays visible as text, as it prints
          if (c.kind === "rule") return drawRule(node.from, node.to, c.css);
          if (c.kind === "text") {
            let block = node.node.parent;
            while (block && !TEXT_BLOCK.test(block.name)) block = block.parent;
            open.push({ tag: node.from, from: node.to, css: c.css, end: block ? block.to : doc.lineAt(node.to).to });
            return;
          }
          const o = open.pop();
          if (!o) return; // a stray </Text> prints as text
          style(o.from, node.from, o.css);
          const shown = touches(o.tag, node.to);
          tag(o.tag, o.from, shown);
          tag(node.from, node.to, shown);
        } else if (name === "Escape") {
          if (!touches(node.from, node.to)) out.push(hide.range(node.from, node.from + 1));
        } else if (BLOCK_LINES[name]) {
          eachLine(node.from, node.to, BLOCK_LINES[name]);
          if (name !== "Blockquote") return false;
        }
      },
    });
    closeBefore(Infinity);
  }
  return Decoration.set(out, true);
}

const livePreview = (reveal: () => boolean, header: boolean) =>
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view, reveal(), header);
      }
      update(u: ViewUpdate) {
        // The parser works in the background on long documents, so a newer syntax tree also counts.
        const parsed = syntaxTree(u.state) !== syntaxTree(u.startState);
        if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged || parsed || u.transactions.some((t) => t.reconfigured)) {
          this.decorations = build(u.view, reveal(), header);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

/**
 * ↑/↓ by visual line, but never past a whole line. CodeMirror aims half a line beyond the
 * current one, which overshoots the short lines this page draws (blank lines, rules).
 */
const moveLine =
  (forward: boolean, extend: boolean): Command =>
  (view) => {
    const { doc } = view.state;
    const ranges = view.state.selection.ranges.map((r) => {
      const next = view.moveVertically(r, forward);
      const from = doc.lineAt(r.head);
      let head = next.head;
      if (Math.abs(doc.lineAt(head).number - from.number) > 1) {
        const target = doc.line(from.number + (forward ? 1 : -1));
        head = target.from + Math.min(r.head - from.from, target.length);
      }
      return extend ? EditorSelection.range(r.anchor, head, next.goalColumn) : EditorSelection.cursor(head, 0, undefined, next.goalColumn);
    });
    view.dispatch({ selection: EditorSelection.create(ranges, view.state.selection.mainIndex), scrollIntoView: true, userEvent: "select" });
    return true;
  };

/** Enter in a list item starts the next one; Enter in an empty item ends the list. */
const continueList: Command = (view) => {
  const { state } = view;
  const sel = state.selection.main;
  if (!sel.empty) return false;
  const line = state.doc.lineAt(sel.head);
  const m = /^(\s*)([-*+]|\d+[.)])(\s+)/.exec(line.text);
  if (!m || sel.head < line.from + m[0].length) return false;
  if (line.text.slice(m[0].length).trim() === "") {
    view.dispatch({ changes: { from: line.from, to: line.to }, selection: { anchor: line.from }, userEvent: "delete" });
    return true;
  }
  const num = /^(\d+)([.)])$/.exec(m[2]);
  const marker = num ? `${Number(num[1]) + 1}${num[2]}` : m[2];
  const insert = `\n${m[1]}${marker}${m[3]}`;
  view.dispatch({ changes: { from: sel.head, insert }, selection: { anchor: sel.head + insert.length }, scrollIntoView: true, userEvent: "input" });
  return true;
};

// ---------------------------------------------------------------- formatting (keys and the right-click menu)

/** The innermost `name` node that holds all of from..to, if any. */
function enclosing(state: EditorState, from: number, to: number, name: string): SyntaxNode | null {
  for (let n: SyntaxNode | null = syntaxTree(state).resolveInner(from, 1); n; n = n.parent) {
    if (n.name === name && n.from <= from && n.to >= to) return n;
  }
  return null;
}

/**
 * Wrap the selection (or the word at the cursor) in a Markdown mark, or unwrap it if it already
 * sits inside one. `node` is the syntax node the mark makes, e.g. StrongEmphasis for "**".
 */
const toggleMark =
  (m: string, node: string): Command =>
  (view) => {
    const { state } = view;
    if (state.readOnly) return false;
    const n = m.length;
    view.dispatch(
      state.changeByRange((r) => {
        const inside = enclosing(state, r.from, r.to, node);
        const open = inside?.firstChild;
        const close = inside?.lastChild;
        if (inside && open && close && open !== close) {
          const changes = state.changes([{ from: open.from, to: open.to }, { from: close.from, to: close.to }]);
          return { changes, range: r.map(changes) };
        }
        const word = r.empty ? state.wordAt(r.head) : null;
        const from = word ? word.from : r.from;
        const to = word ? word.to : r.to;
        return { changes: [{ from, insert: m }, { from: to, insert: m }], range: EditorSelection.range(r.anchor + n, r.head + n) };
      }),
      { userEvent: "input", scrollIntoView: true },
    );
    return true;
  };

/** What a line is: plain text, a section (##) or role (###) heading, or a list item. */
export type Block = "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol";
const PREFIX = /^(\s*)(?:(#{1,6})(?:\s+|$)|([-*+])\s+|(\d+)[.)]\s+)?/;

function blockOf(text: string): Block {
  const m = PREFIX.exec(text)!;
  if (m[2]) return `h${m[2].length}` as Block;
  return m[3] ? "ul" : m[4] ? "ol" : "p";
}

/** The lines the selection touches; blank ones are skipped when there are others. */
function selectedLines(state: EditorState) {
  const seen = new Map<number, { from: number; text: string }>();
  for (const r of state.selection.ranges) {
    for (let n = state.doc.lineAt(r.from).number; n <= state.doc.lineAt(r.to).number; n++) {
      const l = state.doc.line(n);
      seen.set(n, { from: l.from, text: l.text });
    }
  }
  const lines = [...seen.values()];
  const filled = lines.filter((l) => l.text.trim());
  return filled.length ? filled : lines;
}

/** Turn the selected lines into `kind`, or back into plain text if they all already are. */
const setBlock =
  (kind: Block): Command =>
  (view) => {
    const { state } = view;
    if (state.readOnly) return false;
    const lines = selectedLines(state);
    const target = kind !== "p" && lines.every((l) => blockOf(l.text) === kind) ? "p" : kind;
    let num = 0;
    const changes = state.changes(
      lines.map((l) => {
        const m = PREFIX.exec(l.text)!;
        const list = target === "ul" || target === "ol";
        const mark = target === "p" ? "" : target === "ul" ? "- " : target === "ol" ? `${++num}. ` : `${"#".repeat(Number(target[1]))} `;
        return { from: l.from, to: l.from + m[0].length, insert: (list ? m[1] : "") + mark };
      }),
    );
    view.dispatch({ changes, selection: state.selection.map(changes, 1), userEvent: "input", scrollIntoView: true });
    return true;
  };

/**
 * Inside a link: unwrap it, keeping its text. Otherwise wrap the selection (or the word at the
 * cursor) as [text](https://) with the URL selected, ready to type or paste over.
 */
const toggleLink: Command = (view) => {
  const { state } = view;
  if (state.readOnly) return false;
  const r = state.selection.main;
  const link = enclosing(state, r.from, r.to, "Link");
  if (link) {
    const marks = link.getChildren("LinkMark");
    const label = marks.length >= 2 ? state.sliceDoc(marks[0].to, marks[1].from) : state.sliceDoc(link.from, link.to);
    view.dispatch({
      changes: { from: link.from, to: link.to, insert: label },
      selection: EditorSelection.range(link.from, link.from + label.length),
      userEvent: "input",
    });
    return true;
  }
  const word = r.empty ? state.wordAt(r.head) : null;
  const from = word ? word.from : r.from;
  const to = word ? word.to : r.to;
  const text = state.sliceDoc(from, to);
  const isUrl = /^(?:https?:\/\/|mailto:)\S+$/.test(text);
  const label = isUrl ? "" : text;
  const url = isUrl ? text : "https://";
  const insert = `[${label}](${url})`;
  const urlFrom = from + label.length + 3;
  view.dispatch({
    changes: { from, to, insert },
    // No label yet: the cursor goes between the brackets. Otherwise the URL is selected.
    selection: label ? EditorSelection.range(urlFrom, urlFrom + url.length) : EditorSelection.cursor(from + 1),
    userEvent: "input",
    scrollIntoView: true,
  });
  return true;
};

/** Put a horizontal rule below the current line, with the blank line Markdown needs above it. */
const insertRule: Command = (view) => {
  const { state } = view;
  if (state.readOnly) return false;
  const line = state.doc.lineAt(state.selection.main.head);
  const prevFilled = line.number > 1 && state.doc.line(line.number - 1).text.trim() !== "";
  // Straight under a line of text, --- would turn that text into a heading instead.
  const [from, before] = line.text.trim() ? [line.to, "\n\n"] : [line.from, prevFilled ? "\n" : ""];
  const insert = `${before}---\n`;
  view.dispatch({
    changes: { from, to: line.text.trim() ? from : line.to, insert },
    selection: EditorSelection.cursor(from + insert.length),
    userEvent: "input",
    scrollIntoView: true,
  });
  return true;
};

// ---------------------------------------------------------------- MDX styling (the right-click menu)

/** <Text> attributes in the order they are written. */
const ATTR_ORDER = ["color", "underline", "underlineColor", "size", "caps", "spacing", "font"];
type Attrs = Map<string, string>;

/** A <Text> span on one line: where its two tags are, and its attributes. */
interface Span {
  open: { from: number; to: number };
  close: { from: number; to: number };
  attrs: Attrs;
}

function spansOn(state: EditorState, pos: number): Span[] {
  const line = state.doc.lineAt(pos);
  const stack: { from: number; to: number; attrs: Attrs }[] = [];
  const spans: Span[] = [];
  for (const m of line.text.matchAll(/<\/?Text\b[^>]*>/g)) {
    const c = component(m[0]);
    const from = line.from + m.index;
    const to = from + m[0].length;
    if (c?.kind === "text") stack.push({ from, to, attrs: parseTag(m[0])!.attrs });
    else if (c?.kind === "end") {
      const o = stack.pop();
      if (o) spans.push({ open: { from: o.from, to: o.to }, close: { from, to }, attrs: o.attrs });
    }
  }
  return spans;
}

/** The innermost <Text> span holding all of from..to (its tags count as inside). */
function spanAt(state: EditorState, from: number, to: number): Span | null {
  if (state.doc.lineAt(from).number !== state.doc.lineAt(to).number) return null;
  const holding = spansOn(state, from).filter((s) => s.open.from <= from && to <= s.close.to);
  return holding.sort((a, b) => b.open.from - a.open.from)[0] ?? null;
}

const openTag = (attrs: Attrs) => {
  const rank = (k: string) => (ATTR_ORDER.includes(k) ? ATTR_ORDER.indexOf(k) : ATTR_ORDER.length);
  const keys = [...attrs.keys()].sort((a, b) => rank(a) - rank(b));
  return `<Text${keys.map((k) => (attrs.get(k) === "true" ? ` ${k}` : ` ${k}="${attrs.get(k)}"`)).join("")}>`;
};

/** What styling targets when nothing is selected: a whole heading or header line, else the word at the cursor. */
function defaultTarget(state: EditorState, pos: number): { from: number; to: number } | null {
  const line = state.doc.lineAt(pos);
  const head = headerLines(state.doc);
  const inHeader = head && [...head.headline, ...head.contact].includes(line.number);
  const heading = /^(#{1,6}\s+)\S/.exec(line.text);
  if (heading || inHeader) {
    const start = line.from + (heading ? heading[1].length : line.text.length - line.text.trimStart().length);
    return { from: start, to: line.from + line.text.trimEnd().length };
  }
  return state.wordAt(pos);
}

/**
 * Set or remove <Text> attributes (`null` removes one). Inside a span, or with all of a span's
 * words selected, that span changes, and goes away once it has no attributes left. Otherwise
 * the selection, or what `defaultTarget` picks, is wrapped in a new span.
 */
const setText =
  (patch: Record<string, string | null>): Command =>
  (view) => {
    const { state } = view;
    if (state.readOnly) return false;
    const r = state.selection.main;
    const span = spanAt(state, r.from, r.to);
    let changes;
    if (span && (r.empty || (r.from <= span.open.to && r.to >= span.close.from))) {
      const attrs = new Map(span.attrs);
      for (const [k, v] of Object.entries(patch)) v === null ? attrs.delete(k) : attrs.set(k, v);
      changes = attrs.size
        ? state.changes({ from: span.open.from, to: span.open.to, insert: openTag(attrs) })
        : state.changes([span.open, span.close]);
    } else {
      const attrs: Attrs = new Map(Object.entries(patch).filter((e): e is [string, string] => e[1] !== null));
      if (!attrs.size) return true;
      const target = r.empty ? defaultTarget(state, r.head) : r;
      const { from, to } = target ?? { from: r.head, to: r.head };
      changes = state.changes([
        { from, insert: openTag(attrs) },
        { from: to, insert: "</Text>" },
      ]);
    }
    view.dispatch({ changes, selection: state.selection.map(changes, 1), userEvent: "input", scrollIntoView: true });
    return true;
  };

/** Remove the <Text> span at the cursor, keeping its words. */
const clearText: Command = (view) => {
  const r = view.state.selection.main;
  const span = spanAt(view.state, r.from, r.to);
  if (!span || view.state.readOnly) return false;
  const changes = view.state.changes([span.open, span.close]);
  view.dispatch({ changes, selection: view.state.selection.map(changes), userEvent: "delete" });
  return true;
};

/** The rule on the cursor's line (a --- or a <Rule />), and its colour. */
function ruleAt(state: EditorState): { color?: string } | null {
  const line = state.doc.lineAt(state.selection.main.head);
  if (isRuleLine(line.text)) return { color: parseTag(line.text)!.attrs.get("color") };
  if (enclosing(state, line.from, line.from, "HorizontalRule")) return {};
  // A --- straight under the header parses as a heading underline, but it is drawn and printed as a rule.
  return /^\s*([-*_])(?:\s*\1){2,}\s*$/.test(line.text) && line.number === headerEndLine(state.doc) + 1 ? {} : null;
}

/** The last line of the resume header, or 0 without one. */
const headerEndLine = (doc: Text) => {
  const head = headerLines(doc);
  return head ? Math.max(head.name, ...head.headline, ...head.contact) : 0;
};

/** Colour the rule on the cursor's line, or (with null) make it plain again. */
const setRuleColor =
  (color: string | null): Command =>
  (view) => {
    const { state } = view;
    const line = state.doc.lineAt(state.selection.main.head);
    if (state.readOnly || !ruleAt(state)) return false;
    // A --- straight under text would turn that text into a heading, so a plain rule there stays a <Rule />.
    const above = line.number > 1 ? state.doc.line(line.number - 1).text.trim() : "";
    // Under the header, --- is a rule anyway (render.rs `split_header` ends the header there).
    const dashesSafe =
      !above || above.startsWith("#") || isRuleLine(above) || /^([-*_])(?:\s*\1){2,}$/.test(above) || line.number === headerEndLine(state.doc) + 1;
    const insert = color ? `<Rule color="${color}" />` : dashesSafe ? "---" : "<Rule />";
    view.dispatch({ changes: { from: line.from, to: line.to, insert }, selection: { anchor: line.from }, userEvent: "input" });
    return true;
  };

/** The look of the default template (workspace.rs RESUME_TEMPLATE), as <Text> attributes. */
export const RESUME_STYLE = {
  headline: { color: "#555b66", size: "11.5pt" },
  contact: { color: "#555b66", size: "9.5pt" },
  section: { color: "#1f4e8c", size: "10pt", caps: "true", spacing: "0.08em" },
  dates: { color: "#555b66", size: "9.5pt" },
};

/**
 * Give a plain resume the template's look in one undoable edit: the headline and contact
 * lines, every `##` heading and the italic dates line under each `###`. Lines that already
 * have <Text> styling are left as they are.
 */
const applyResumeStyle: Command = (view) => {
  const { state } = view;
  if (state.readOnly) return false;
  const { doc } = state;
  const head = headerLines(doc);
  const wrap = (n: number, skip: number, style: Record<string, string>) => {
    const l = doc.line(n);
    const text = l.text.slice(skip).trim();
    if (!text || text.includes("<Text")) return [];
    const from = l.from + l.text.indexOf(text, skip);
    return [
      { from, insert: openTag(new Map(Object.entries(style))) },
      { from: from + text.length, insert: "</Text>" },
    ];
  };
  const changes = [
    ...(head?.headline ?? []).flatMap((n) => wrap(n, 0, RESUME_STYLE.headline)),
    ...(head?.contact ?? []).flatMap((n) => wrap(n, 0, RESUME_STYLE.contact)),
  ];
  let fenced = false;
  for (let n = 1; n <= doc.lines; n++) {
    const t = doc.line(n).text;
    if (/^\s*(```|~~~)/.test(t)) fenced = !fenced;
    if (fenced) continue;
    const h2 = /^##\s+/.exec(t);
    if (h2) changes.push(...wrap(n, h2[0].length, RESUME_STYLE.section));
    if (/^###\s/.test(t) && n < doc.lines && /^\s*([*_])(?!\1).*\1\s*$/.test(doc.line(n + 1).text)) {
      changes.push(...wrap(n + 1, 0, RESUME_STYLE.dates));
    }
  }
  if (!changes.length) return true;
  const set = state.changes(changes);
  view.dispatch({ changes: set, selection: state.selection.map(set), userEvent: "input" });
  return true;
};

export const format = {
  bold: toggleMark("**", "StrongEmphasis"),
  italic: toggleMark("*", "Emphasis"),
  strike: toggleMark("~~", "Strikethrough"),
  link: toggleLink,
  block: setBlock,
  rule: insertRule,
  text: setText,
  underline: ((view) => {
    const r = view.state.selection.main;
    const a = spanAt(view.state, r.from, r.to)?.attrs;
    const underlined = !!a && (on(a.get("underline")) || a.has("underlineColor"));
    return setText(underlined ? { underline: null, underlineColor: null } : { underline: "true" })(view);
  }) as Command,
  clearText,
  ruleColor: setRuleColor,
  resumeStyle: applyResumeStyle,
};

/** Which formatting applies at the main selection, for checkmarks in the menu. */
export function formatAt(state: EditorState) {
  const { from, to, head } = state.selection.main;
  const has = (name: string) => !!enclosing(state, from, to, name);
  return {
    bold: has("StrongEmphasis"),
    italic: has("Emphasis"),
    strike: has("Strikethrough"),
    link: has("Link"),
    block: blockOf(state.doc.lineAt(head).text),
    /** Attributes of the <Text> span at the cursor, if any. */
    text: spanAt(state, from, to)?.attrs ?? null,
    rule: ruleAt(state),
  };
}

export interface LiveEditor {
  view: EditorView;
  get(): string;
  /** Replace the text. `keepPlace` keeps the cursor and scroll position (for reloads from disk). */
  set(text: string, keepPlace?: boolean): void;
  setReadOnly(ro: boolean): void;
  /** Select a 1-based line and scroll it into view. */
  gotoLine(n: number): void;
  focus(): void;
}

export function createLiveEditor(opts: {
  parent: HTMLElement;
  placeholder?: string;
  onChange?: (text: string) => void;
  /** The text opens with a resume header (`# Name` and the lines under it); see `headerLines`. */
  header?: boolean;
}): LiveEditor {
  const ro = new Compartment();
  let readOnly = false;
  let silent = false;

  const readOnlyExt = () => (readOnly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : []);
  const extensions = (): Extension[] => [
    history(),
    keymap.of([
      { key: "Enter", run: continueList },
      { key: "ArrowUp", run: moveLine(false, false), shift: moveLine(false, true) },
      { key: "ArrowDown", run: moveLine(true, false), shift: moveLine(true, true) },
      { key: "Mod-b", run: format.bold },
      { key: "Mod-i", run: format.italic },
      { key: "Mod-u", run: format.underline },
      indentWithTab,
      ...defaultKeymap,
      ...historyKeymap,
    ]),
    markdown(),
    EditorView.lineWrapping,
    EditorView.contentAttributes.of({ spellcheck: "true", autocorrect: "on", "aria-multiline": "true" }),
    placeholderExt(opts.placeholder ?? ""),
    livePreview(() => !readOnly, !!opts.header),
    ro.of(readOnlyExt()),
    EditorView.updateListener.of((u) => {
      if (u.docChanged && !silent) opts.onChange?.(u.state.doc.toString());
    }),
  ];
  const view = new EditorView({ parent: opts.parent, state: EditorState.create({ doc: "", extensions: extensions() }) });

  return {
    view,
    get: () => view.state.doc.toString(),
    set(text, keepPlace = false) {
      if (!keepPlace) {
        // A different document: start with a fresh undo history.
        view.setState(EditorState.create({ doc: text, extensions: extensions() }));
        return;
      }
      if (text === view.state.doc.toString()) return;
      const sel = view.state.selection.main;
      silent = true;
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
        selection: { anchor: Math.min(sel.anchor, text.length), head: Math.min(sel.head, text.length) },
        annotations: Transaction.addToHistory.of(false),
      });
      silent = false;
    },
    setReadOnly(value) {
      readOnly = value;
      view.dispatch({ effects: ro.reconfigure(readOnlyExt()) });
    },
    gotoLine(n) {
      const l = view.state.doc.line(Math.max(1, Math.min(n, view.state.doc.lines)));
      view.focus();
      view.dispatch({ selection: { anchor: l.from, head: l.to }, effects: EditorView.scrollIntoView(l.from, { y: "center" }) });
    },
    focus: () => view.focus(),
  };
}
