// Obsidian-style live preview for Markdown on CodeMirror 6.
// The text stays plain Markdown; formatting renders in place, and the raw syntax
// (##, **, [](), - ) shows only on the line(s) the cursor is on.

import { Compartment, EditorState, type Range } from "@codemirror/state";
import {
  Decoration, EditorView, ViewPlugin, WidgetType, keymap, placeholder as placeholderExt,
  type DecorationSet, type ViewUpdate,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { Language, LanguageSupport, defineLanguageFacet, syntaxTree } from "@codemirror/language";
import { GFM, parser as mdParser } from "@lezer/markdown";

// Markdown + GitHub extensions (tables, strikethrough), without the HTML/JS/CSS
// sub-languages @codemirror/lang-markdown would pull in.
const markdown = () =>
  new LanguageSupport(new Language(defineLanguageFacet(), mdParser.configure(GFM), [], "markdown"));

class BulletWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "md-bullet";
    s.textContent = "•";
    return s;
  }
}
const bullet = Decoration.replace({ widget: new BulletWidget() });
const hide = Decoration.replace({});
const mark = (cls: string) => Decoration.mark({ class: cls });
const line = (cls: string) => Decoration.line({ class: cls });

const INLINE: Record<string, string> = {
  Emphasis: "md-em",
  StrongEmphasis: "md-strong",
  InlineCode: "md-code",
  Strikethrough: "md-strike",
  Link: "md-link",
};
const HIDDEN_MARKS = new Set(["EmphasisMark", "CodeMark", "StrikethroughMark", "LinkMark", "URL", "QuoteMark"]);

function activeLines(view: EditorView, reveal: boolean): Set<number> {
  const set = new Set<number>();
  if (!reveal || !view.hasFocus) return set;
  for (const r of view.state.selection.ranges) {
    const a = view.state.doc.lineAt(r.from).number;
    const b = view.state.doc.lineAt(r.to).number;
    for (let n = a; n <= b; n++) set.add(n);
  }
  return set;
}

function build(view: EditorView, reveal: boolean): DecorationSet {
  const { state } = view;
  const doc = state.doc;
  const active = activeLines(view, reveal);
  const out: Range<Decoration>[] = [];
  const isActive = (pos: number) => active.has(doc.lineAt(pos).number);

  for (const { from, to } of view.visibleRanges) {
    for (let pos = from; pos <= to; ) {
      const l = doc.lineAt(pos);
      if (l.length === 0 && !active.has(l.number)) out.push(line("md-blank").range(l.from));
      pos = l.to + 1;
    }
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        const heading = /^(?:ATX|Setext)Heading(\d)$/.exec(name);
        if (heading) {
          out.push(line(`md-h md-h${heading[1]}`).range(doc.lineAt(node.from).from));
          return;
        }
        if (name === "HeaderMark") {
          if (isActive(node.from)) return;
          const end = doc.sliceString(node.to, node.to + 1) === " " ? node.to + 1 : node.to;
          if (end > node.from) out.push(hide.range(node.from, end));
          return;
        }
        if (INLINE[name]) {
          if (node.to > node.from) out.push(mark(INLINE[name]).range(node.from, node.to));
          return;
        }
        if (HIDDEN_MARKS.has(name)) {
          // Inside a link only the [ ] ( ) and URL are hidden; autolinks keep their URL.
          if (name === "URL" && node.node.parent?.name !== "Link") return;
          if (!isActive(node.from) && node.to > node.from) out.push(hide.range(node.from, node.to));
          return;
        }
        if (name === "ListItem") {
          out.push(line("md-li").range(doc.lineAt(node.from).from));
          return;
        }
        if (name === "ListMark") {
          const text = doc.sliceString(node.from, node.to);
          if (/^[-*+]$/.test(text) && !isActive(node.from)) {
            const end = doc.sliceString(node.to, node.to + 1) === " " ? node.to + 1 : node.to;
            out.push(bullet.range(node.from, end));
          } else out.push(mark("md-listmark").range(node.from, node.to));
          return;
        }
        if (name === "HorizontalRule") {
          out.push(line("md-hr").range(doc.lineAt(node.from).from));
          if (!isActive(node.from)) out.push(hide.range(node.from, node.to));
          return;
        }
        if (name === "Blockquote") {
          for (let p = node.from; p <= node.to; ) {
            const l = doc.lineAt(p);
            out.push(line("md-quote").range(l.from));
            p = l.to + 1;
          }
          return;
        }
        if (name === "FencedCode" || name === "CodeBlock") {
          for (let p = node.from; p <= node.to; ) {
            const l = doc.lineAt(p);
            out.push(line("md-codeblock").range(l.from));
            p = l.to + 1;
          }
          return false;
        }
        if (name === "Table") {
          for (let p = node.from; p <= node.to; ) {
            const l = doc.lineAt(p);
            out.push(line("md-table").range(l.from));
            p = l.to + 1;
          }
          return false;
        }
      },
    });
  }
  return Decoration.set(out, true);
}

const livePreview = (reveal: () => boolean) =>
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view, reveal());
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged || u.transactions.some((t) => t.reconfigured)) {
          this.decorations = build(u.view, reveal());
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

export interface LiveEditor {
  view: EditorView;
  get(): string;
  /** Replace the text. `keepPlace` keeps the cursor and scroll (for reloads from disk). */
  set(text: string, keepPlace?: boolean): void;
  setReadOnly(ro: boolean): void;
  gotoLine(n: number): void;
  focus(): void;
}

export function createLiveEditor(opts: {
  parent: HTMLElement;
  placeholder?: string;
  onChange?: (text: string) => void;
}): LiveEditor {
  const ro = new Compartment();
  let readOnly = false;
  let silent = false;

  const view = new EditorView({
    parent: opts.parent,
    state: EditorState.create({
      doc: "",
      extensions: [
        history(),
        keymap.of([indentWithTab, ...defaultKeymap, ...historyKeymap]),
        markdown(),
        EditorView.lineWrapping,
        EditorView.contentAttributes.of({ spellcheck: "true", autocorrect: "on" }),
        placeholderExt(opts.placeholder ?? ""),
        livePreview(() => !readOnly),
        ro.of([]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged && !silent) opts.onChange?.(u.state.doc.toString());
        }),
      ],
    }),
  });

  return {
    view,
    get: () => view.state.doc.toString(),
    set(text, keepPlace = false) {
      if (text === view.state.doc.toString()) return;
      const sel = view.state.selection.main;
      const scroll = view.scrollDOM.scrollTop;
      silent = true;
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
        selection: keepPlace ? { anchor: Math.min(sel.anchor, text.length), head: Math.min(sel.head, text.length) } : { anchor: 0 },
      });
      silent = false;
      if (keepPlace) view.scrollDOM.scrollTop = scroll;
      else view.scrollDOM.scrollTop = 0;
    },
    setReadOnly(value) {
      readOnly = value;
      view.dispatch({
        effects: ro.reconfigure(value ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : []),
      });
    },
    gotoLine(n) {
      const l = view.state.doc.line(Math.max(1, Math.min(n, view.state.doc.lines)));
      view.focus();
      view.dispatch({ selection: { anchor: l.from, head: l.to }, effects: EditorView.scrollIntoView(l.from, { y: "center" }) });
    },
    focus: () => view.focus(),
  };
}
