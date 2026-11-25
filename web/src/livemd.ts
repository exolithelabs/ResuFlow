// Live-preview Markdown on CodeMirror 6, in the spirit of Obsidian.
// The text stays plain Markdown. Formatting renders in place, and the raw syntax
// (##, **, [](), -) shows only on the lines the cursor is on.

import { Compartment, EditorSelection, EditorState, Transaction, type Extension, type Range } from "@codemirror/state";
import {
  Decoration, EditorView, ViewPlugin, WidgetType, keymap, placeholder as placeholderExt,
  type Command, type DecorationSet, type ViewUpdate,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { Language, LanguageSupport, defineLanguageFacet, syntaxTree } from "@codemirror/language";
import { GFM, parser as mdParser } from "@lezer/markdown";

// Markdown with GitHub extensions, without the HTML/JS/CSS sub-languages
// that @codemirror/lang-markdown would pull in.
const markdown = () => new LanguageSupport(new Language(defineLanguageFacet(), mdParser.configure(GFM), [], "markdown"));

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

function build(view: EditorView, reveal: boolean): DecorationSet {
  const { state } = view;
  const doc = state.doc;
  const active = new Set<number>();
  if (reveal && view.hasFocus) {
    for (const r of state.selection.ranges) {
      for (let n = doc.lineAt(r.from).number; n <= doc.lineAt(r.to).number; n++) active.add(n);
    }
  }
  const isActive = (pos: number) => active.has(doc.lineAt(pos).number);
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
        if (heading) {
          out.push(lineDeco(`md-h md-h${heading[1]}`).range(doc.lineAt(node.from).from));
        } else if (name === "HeaderMark") {
          if (!isActive(node.from)) out.push(hide.range(node.from, withSpace(node.to)));
        } else if (INLINE[name]) {
          if (node.to > node.from) out.push(mark(INLINE[name]).range(node.from, node.to));
        } else if (SYNTAX.has(name)) {
          // In a link only the brackets and URL hide; a bare autolink keeps its URL.
          if (name === "URL" && node.node.parent?.name !== "Link") return;
          if (!isActive(node.from) && node.to > node.from) out.push(hide.range(node.from, node.to));
        } else if (name === "ListItem") {
          out.push(lineDeco("md-li").range(doc.lineAt(node.from).from));
        } else if (name === "ListMark") {
          const bulletMark = /^[-*+]$/.test(doc.sliceString(node.from, node.to));
          if (bulletMark && !isActive(node.from)) out.push(bullet.range(node.from, withSpace(node.to)));
          else out.push(mark("md-listmark").range(node.from, node.to));
        } else if (name === "HorizontalRule") {
          out.push(lineDeco("md-hr").range(doc.lineAt(node.from).from));
          if (!isActive(node.from)) out.push(hide.range(node.from, node.to));
        } else if (BLOCK_LINES[name]) {
          eachLine(node.from, node.to, BLOCK_LINES[name]);
          if (name !== "Blockquote") return false;
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
        // The parser works in the background on long documents, so a newer syntax tree also counts.
        const parsed = syntaxTree(u.state) !== syntaxTree(u.startState);
        if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged || parsed || u.transactions.some((t) => t.reconfigured)) {
          this.decorations = build(u.view, reveal());
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

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

/** Wrap the selection in a Markdown mark, or unwrap it if it already is. */
const toggleMark =
  (m: string): Command =>
  (view) => {
    if (view.state.readOnly) return false;
    const n = m.length;
    view.dispatch(
      view.state.changeByRange((r) => {
        const wrapped = view.state.sliceDoc(r.from - n, r.from) === m && view.state.sliceDoc(r.to, r.to + n) === m;
        return wrapped
          ? { changes: [{ from: r.from - n, to: r.from }, { from: r.to, to: r.to + n }], range: EditorSelection.range(r.from - n, r.to - n) }
          : { changes: [{ from: r.from, insert: m }, { from: r.to, insert: m }], range: EditorSelection.range(r.from + n, r.to + n) };
      }),
      { userEvent: "input" },
    );
    return true;
  };

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

export function createLiveEditor(opts: { parent: HTMLElement; placeholder?: string; onChange?: (text: string) => void }): LiveEditor {
  const ro = new Compartment();
  let readOnly = false;
  let silent = false;

  const readOnlyExt = () => (readOnly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : []);
  const extensions = (): Extension[] => [
    history(),
    keymap.of([
      { key: "Enter", run: continueList },
      { key: "Mod-b", run: toggleMark("**") },
      { key: "Mod-i", run: toggleMark("*") },
      indentWithTab,
      ...defaultKeymap,
      ...historyKeymap,
    ]),
    markdown(),
    EditorView.lineWrapping,
    EditorView.contentAttributes.of({ spellcheck: "true", autocorrect: "on", "aria-multiline": "true" }),
    placeholderExt(opts.placeholder ?? ""),
    livePreview(() => !readOnly),
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
