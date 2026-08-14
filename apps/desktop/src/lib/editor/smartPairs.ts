import { EditorSelection, EditorState, type StateCommand } from "@codemirror/state";

const PAIRS = new Map([
  ["(", ")"],
  ["[", "]"],
  ["{", "}"],
  ["'", "'"],
  ['"', '"'],
  ["$", "$"],
]);

export const typstPairLanguageData = EditorState.languageData.of(() => [{
  closeBrackets: {
    brackets: [...PAIRS.keys()],
    before: ")]};:>,",
  },
}]);

/** `$|$` becomes `$ | $`; ordinary spaces everywhere else remain ordinary. */
export const spaceInsideMath: StateCommand = ({ state, dispatch }) => {
  let handled = false;
  const changes = state.changeByRange((range) => {
    if (
      range.empty &&
      range.head >= 1 &&
      state.sliceDoc(range.head - 1, range.head) === "$" &&
      state.sliceDoc(range.head, range.head + 1) === "$"
    ) {
      handled = true;
      return {
        changes: { from: range.head, insert: "  " },
        range: EditorSelection.cursor(range.head + 1),
      };
    }
    return { range };
  });
  if (handled) dispatch(state.update(changes, { userEvent: "input.type" }));
  return handled;
};

/** Enter between an empty pair opens a readable indented line without moving the closer. */
export const newlineInsidePair: StateCommand = ({ state, dispatch }) => {
  let handled = false;
  const changes = state.changeByRange((range) => {
    if (!range.empty || range.head < 1) return { range };
    let from = range.head;
    let to = range.head;
    let open = state.sliceDoc(from - 1, from);
    let close = state.sliceDoc(to, to + 1);

    // Space inside math first produces `$ | $`; Enter should replace those helper spaces.
    if (
      from >= 2 &&
      state.sliceDoc(from - 2, from) === "$ " &&
      state.sliceDoc(to, to + 2) === " $"
    ) {
      from -= 1;
      to += 1;
      open = "$";
      close = "$";
    }
    if (PAIRS.get(open) !== close) return { range };

    const line = state.doc.lineAt(range.head);
    const indent = /^\s*/.exec(line.text)?.[0] ?? "";
    const insert = `\n${indent}  \n${indent}`;
    handled = true;
    return {
      changes: { from, to, insert },
      range: EditorSelection.cursor(from + 1 + indent.length + 2),
    };
  });
  if (handled) dispatch(state.update(changes, { userEvent: "input.type" }));
  return handled;
};
