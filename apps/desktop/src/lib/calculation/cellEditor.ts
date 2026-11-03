import { basicSetup, EditorView } from "codemirror";
import { EditorState, Prec } from "@codemirror/state";
import { keymap } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { python } from "@codemirror/lang-python";
import { markdown } from "@codemirror/lang-markdown";
import { editorPhrases } from "../editor/phrases";
import { translate } from "../i18n";

type Entry = { state: EditorState; type: string };

// Use semantic token colors rather than filtering rendered text, which also inverts
// selections, bracket decorations and Markdown styles. Variables follow the workspace palette.
const cellHighlighting = syntaxHighlighting(HighlightStyle.define([
  { tag: tags.keyword, color: "var(--calculation-keyword, #d2a8ff)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--calculation-string, #a5d6a7)" },
  { tag: [tags.number, tags.bool, tags.null], color: "var(--calculation-number, #f2b07b)" },
  { tag: tags.comment, color: "var(--muted, #aeb5be)", fontStyle: "italic" },
  { tag: [tags.function(tags.variableName), tags.definition(tags.variableName)], color: "var(--calculation-name, #8dbdff)" },
  { tag: tags.typeName, color: "var(--calculation-type, #e8c07d)" },
  { tag: tags.heading, color: "var(--calculation-name, #8dbdff)", fontWeight: "bold" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strong, fontWeight: "bold" },
  { tag: tags.link, color: "var(--calculation-name, #8dbdff)", textDecoration: "underline" },
  { tag: tags.invalid, color: "var(--calculation-error, #ff8b94)" },
]));

/**
 * One live view moves between the inline and side hosts. Each cell keeps its own state, so the
 * caret, selection and local undo survive both a host move and a switch to another cell.
 * A cached state is only a view of the canonical source: `reset` and `forget` discard it
 * whenever canonical history, a type change or a deletion makes it stale.
 */
export function createCellEditor(onChange: (key: string, source: string) => void, onRun: (advance: boolean) => void) {
  let view: EditorView | undefined;
  let active = "";
  let activeType = "";
  const states = new Map<string, Entry>();
  function create(key: string, source: string, type: string): EditorState {
    return EditorState.create({ doc: source, extensions: [
      basicSetup, cellHighlighting, EditorView.lineWrapping, type === "code" ? python() : markdown(),
      EditorState.phrases.of(editorPhrases(translate)),
      Prec.highest(keymap.of([
        { key: "Shift-Enter", run: () => { onRun(true); return true; } },
        { key: "Mod-Enter", run: () => { onRun(false); return true; } },
      ])),
      EditorView.updateListener.of(update => { if (update.docChanged && active === key) onChange(key, update.state.doc.toString()); }),
      EditorView.theme({
        "&": { fontSize: "11px", color: "var(--text, #e9ebee)", backgroundColor: "#1f2329" },
        ".cm-gutters": { backgroundColor: "#1f2329", color: "var(--quiet, #6a727c)", border: "none" },
        "&.cm-focused": { outline: "none" },
        ".cm-content": { fontFamily: "'DejaVu Sans Mono', Consolas, monospace", padding: "10px 0", minHeight: "36px", caretColor: "var(--text, #e9ebee)" },
        ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text, #e9ebee)" },
        ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--wash, #ffffff0a)" },
        "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--calculation-selection, #4c8df044)" },
        ".cm-panels, .cm-tooltip": { backgroundColor: "var(--panel, #23272f)", color: "var(--text, #e9ebee)" },
        ".cm-scroller": { overflow: "auto" },
      }, { dark: true }),
    ] });
  }
  const current = (key: string) => key === active && view ? view.state : states.get(key)?.state;
  /** Make `key` the edited cell. Without a cached state of the same cell type, one is built from `source`. */
  function select(key: string, source: string, type: string, focus = true) {
    if (active === key && activeType === type) { if (focus && view?.dom.isConnected) view.focus(); return; }
    if (view && active) states.set(active, { state: view.state, type: activeType });
    const cached = states.get(key);
    const state = cached && cached.type === type ? cached.state : create(key, source, type);
    active = key; activeType = type;
    if (view) view.setState(state);
    else view = new EditorView({ state });
    if (focus && view.dom.isConnected) view.focus();
  }
  /** Drop a cached state so the next `select` rebuilds it from canonical source. Rebuild the live one immediately. */
  function reset(key: string, source: string, type: string) {
    states.delete(key);
    if (key === active && view) { activeType = type; view.setState(create(key, source, type)); }
  }
  function forget(key: string) {
    states.delete(key);
    if (key === active) release();
  }
  /** Detach the live view from the edited cell without losing the cached state. */
  function release() {
    if (view && active) states.set(active, { state: view.state, type: activeType });
    active = ""; activeType = "";
    view?.dom.remove();
  }
  /** Commit an active IME composition before its host can unmount. */
  function settleComposition(): Promise<void> {
    if (!view?.composing) return Promise.resolve();
    const current = view;
    return new Promise(resolve => {
      const done = () => requestAnimationFrame(() => resolve());
      current.dom.addEventListener("compositionend", done, { once: true });
      current.contentDOM.blur();
    });
  }
  function mount(node: HTMLElement) {
    let cancel: (() => void) | undefined;
    const place = () => {
      cancel = undefined;
      if (!view) return;
      node.append(view.dom); view.requestMeasure(); view.focus();
    };
    // Moving the DOM node would cancel an IME composition. Wait until the composition ends.
    if (view?.composing) {
      const dom = view.dom;
      const done = () => place();
      dom.addEventListener("compositionend", done, { once: true });
      cancel = () => dom.removeEventListener("compositionend", done);
    } else place();
    return { destroy() { cancel?.(); if (view?.dom.parentElement === node) view.dom.remove(); } };
  }
  return {
    select, reset, forget, release, mount, settleComposition,
    has: (key: string) => key === active || states.has(key),
    text: (key: string) => current(key)?.doc.toString(),
    type: (key: string) => key === active ? activeType : states.get(key)?.type,
    keys: () => [...new Set([...states.keys(), ...(active ? [active] : [])])],
    focus: () => view?.focus(),
    dispose() { view?.destroy(); view = undefined; active = ""; states.clear(); },
  };
}
export type CellEditor = ReturnType<typeof createCellEditor>;
