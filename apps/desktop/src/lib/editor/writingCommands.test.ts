import { EditorSelection, EditorState, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import { applyWritingCommand, type WritingCommand } from "./writingCommands";

function run(doc: string, from: number, to: number, command: WritingCommand) {
  let state = EditorState.create({ doc, selection: EditorSelection.range(from, to) });
  let dispatches = 0;
  let focuses = 0;
  const target = {
    get state() { return state; },
    dispatch(spec: TransactionSpec) { dispatches += 1; state = state.update(spec).state; },
    focus() { focuses += 1; },
  } as unknown as EditorView;
  expect(applyWritingCommand(target, command)).toBe(true);
  return { state, dispatches, focuses };
}

describe("Page text writing commands", () => {
  it("wraps and unwraps Unicode selections in one focused transaction", () => {
    const wrapped = run("Grüße ∑", 0, 7, "bold");
    expect(wrapped.state.doc.toString()).toBe("*Grüße ∑*");
    expect(wrapped.state.sliceDoc(wrapped.state.selection.main.from, wrapped.state.selection.main.to)).toBe("Grüße ∑");
    expect({ dispatches: wrapped.dispatches, focuses: wrapped.focuses }).toEqual({ dispatches: 1, focuses: 1 });

    const unwrapped = run("*Grüße ∑*", 1, 8, "bold");
    expect(unwrapped.state.doc.toString()).toBe("Grüße ∑");
  });

  it("toggles prefixes across selected lines", () => {
    const listed = run("alpha\nbeta", 0, 10, "bullet-list");
    expect(listed.state.doc.toString()).toBe("- alpha\n- beta");
    const plain = run("- alpha\n- beta", 0, 14, "bullet-list");
    expect(plain.state.doc.toString()).toBe("alpha\nbeta");
  });

  it("selects the named placeholder for an empty selection", () => {
    const result = run("", 0, 0, "underline");
    expect(result.state.doc.toString()).toBe("#underline[underlined text]");
    expect(result.state.sliceDoc(result.state.selection.main.from, result.state.selection.main.to)).toBe("underlined text");

    const equation = run("", 0, 0, "snippet:math");
    expect(equation.state.doc.toString()).toBe("$ content $");
    expect(equation.state.sliceDoc(equation.state.selection.main.from, equation.state.selection.main.to)).toBe("content");
  });
});
