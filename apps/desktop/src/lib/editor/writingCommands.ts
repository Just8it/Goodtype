import { EditorView } from "@codemirror/view";
import { TYPST_SNIPPETS } from "./snippets";

export type WritingCommand =
  | "bold" | "italic" | "underline" | "inline-math"
  | "heading-1" | "heading-2" | "heading-3"
  | "bullet-list" | "numbered-list"
  | `snippet:${string}`;

const wrappers: Record<"bold" | "italic" | "underline" | "inline-math", [string, string, string]> = {
  bold: ["*", "*", "bold text"],
  italic: ["_", "_", "emphasized text"],
  underline: ["#underline[", "]", "underlined text"],
  "inline-math": ["$", "$", "x"],
};

export function applyWritingCommand(view: EditorView, command: WritingCommand): boolean {
  if (command in wrappers) return wrap(view, wrappers[command as keyof typeof wrappers]);
  if (command.startsWith("heading-")) return prefixLines(view, `${"=".repeat(Number(command.at(-1)))} `, /^={1,3} /, "Heading");
  if (command === "bullet-list") return prefixLines(view, "- ", /^[-+] /, "List item");
  if (command === "numbered-list") return prefixLines(view, "+ ", /^[-+] /, "List item");
  if (command.startsWith("snippet:")) return insertSnippet(view, command.slice(8));
  return false;
}

function wrap(view: EditorView, [open, close, placeholder]: [string, string, string]): boolean {
  const { from, to } = view.state.selection.main;
  const selected = view.state.sliceDoc(from, to);
  if (from !== to && selected.startsWith(open) && selected.endsWith(close)) {
    const inner = selected.slice(open.length, -close.length);
    return replace(view, from, to, inner, from, from + inner.length);
  }
  if (from >= open.length && view.state.sliceDoc(from - open.length, from) === open && view.state.sliceDoc(to, to + close.length) === close) {
    return replace(view, from - open.length, to + close.length, selected, from - open.length, to - open.length);
  }
  const inner = selected || placeholder;
  return replace(view, from, to, `${open}${inner}${close}`, from + open.length, from + open.length + inner.length);
}

function prefixLines(view: EditorView, prefix: string, anyPrefix: RegExp, placeholder: string): boolean {
  const selection = view.state.selection.main;
  if (selection.empty) {
    return replace(view, selection.from, selection.to, `${prefix}${placeholder}`, selection.from + prefix.length, selection.from + prefix.length + placeholder.length);
  }
  const first = view.state.doc.lineAt(selection.from);
  const last = view.state.doc.lineAt(selection.to > selection.from && view.state.sliceDoc(selection.to - 1, selection.to) === "\n" ? selection.to - 1 : selection.to);
  const original = view.state.sliceDoc(first.from, last.to);
  const lines = original.split("\n");
  const active = lines.filter(Boolean).every((line) => line.startsWith(prefix));
  const changed = lines.map((line) => !line ? line : active ? line.slice(prefix.length) : `${prefix}${line.replace(anyPrefix, "")}`).join("\n");
  return replace(view, first.from, last.to, changed, first.from, first.from + changed.length);
}

function insertSnippet(view: EditorView, label: string): boolean {
  const snippet = TYPST_SNIPPETS.find((candidate) => candidate.label === label);
  if (!snippet) return false;
  const { from, to } = view.state.selection.main;
  const selected = view.state.sliceDoc(from, to);
  const firstMarker = /\$\{([^}]+)\}/.exec(snippet.template);
  let first = true;
  let firstValue = "";
  const text = snippet.template.replace(/\$\{([^}]+)\}/g, (_, name: string) => {
    const value = first && selected ? selected : name;
    if (first) firstValue = value;
    first = false;
    return value;
  });
  const placeholderFrom = firstMarker?.index ?? text.length;
  return replace(view, from, to, text, from + placeholderFrom, from + placeholderFrom + firstValue.length);
}

function replace(view: EditorView, from: number, to: number, insert: string, anchor: number, head: number): boolean {
  view.dispatch({ changes: { from, to, insert }, selection: { anchor, head }, scrollIntoView: true });
  view.focus();
  return true;
}
