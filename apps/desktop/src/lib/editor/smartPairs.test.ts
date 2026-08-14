import { EditorSelection, EditorState } from "@codemirror/state";
import { closeBrackets, insertBracket } from "@codemirror/autocomplete";
import { describe, expect, it } from "vitest";
import {
  newlineInsidePair,
  spaceInsideMath,
  typstPairLanguageData,
} from "./smartPairs";

function run(command: typeof spaceInsideMath, doc: string, cursor: number) {
  let state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [typstPairLanguageData, closeBrackets()],
  });
  expect(command({ state, dispatch: (transaction) => (state = transaction.state) })).toBe(true);
  return state;
}

describe("Typst smart pairs", () => {
  it("uses CodeMirror's native pair state for math delimiters", () => {
    const state = EditorState.create({ extensions: [typstPairLanguageData, closeBrackets()] });
    const transaction = insertBracket(state, "$");
    expect(transaction?.state.doc.toString()).toBe("$$");
    expect(transaction?.state.selection.main.head).toBe(1);
  });

  it("spaces an empty math pair around the caret", () => {
    const state = run(spaceInsideMath, "$$", 1);
    expect(state.doc.toString()).toBe("$  $");
    expect(state.selection.main.head).toBe(2);
  });

  it("opens a middle line inside math and brackets", () => {
    const math = run(newlineInsidePair, "$  $", 2);
    expect(math.doc.toString()).toBe("$\n  \n$");
    expect(math.selection.main.head).toBe(4);

    const brackets = run(newlineInsidePair, "()", 1);
    expect(brackets.doc.toString()).toBe("(\n  \n)");
    expect(brackets.selection.main.head).toBe(4);
  });
});
