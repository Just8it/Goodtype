import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";

export type EditorHighlight = { kind: string; modifiers: string[]; from: number; to: number };

export const setTypstHighlights = StateEffect.define<EditorHighlight[]>();

export const typstHighlighting = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(highlights, transaction) {
    highlights = highlights.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (!effect.is(setTypstHighlights)) continue;
      const semantic = effect.value
        .filter(({ from, to }) => from >= 0 && from < to && to <= transaction.newDoc.length)
        .map(({ kind, modifiers, from, to }) => Decoration.mark({
          class: [`cm-typst-${kind}`, ...modifiers.map((modifier) => `cm-typst-${modifier}`)].join(" "),
        }).range(from, to));
      highlights = Decoration.set(semantic, true);
    }
    return highlights;
  },
  provide: (field) => EditorView.decorations.from(field),
});
