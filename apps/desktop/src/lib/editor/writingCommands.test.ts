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
  it("colours Unicode text with ordinary Typst in one focused transaction", () => {
    const result = run("Grüße ∑", 0, 7, "text-color:#ABC");
    expect(result.state.doc.toString()).toBe('#text(fill: rgb("#aabbcc"))[Grüße ∑]');
    expect(result.state.sliceDoc(result.state.selection.main.from, result.state.selection.main.to)).toBe("Grüße ∑");
    expect({ dispatches: result.dispatches, focuses: result.focuses }).toEqual({ dispatches: 1, focuses: 1 });
  });

  it("inserts selected placeholder text or a colour expression at the caret", () => {
    const text = run("", 0, 0, "text-color:#4c8df0");
    expect(text.state.doc.toString()).toBe('#text(fill: rgb("#4c8df0"))[coloured text]');
    expect(text.state.sliceDoc(text.state.selection.main.from, text.state.selection.main.to)).toBe("coloured text");
    const code = run("#let accent = red", 14, 17, "color-value:#4c8df0");
    expect(code.state.doc.toString()).toBe('#let accent = rgb("#4c8df0")');
    expect(code.state.selection.main.empty).toBe(true);
  });

  it("rejects invalid colour input without editing the document", () => {
    const state = EditorState.create({ doc: "original" });
    const target = { state, dispatch() { throw new Error("Unexpected edit"); } } as unknown as EditorView;
    expect(applyWritingCommand(target, 'text-color:red"))[oops]')).toBe(false);
  });

  it("uses a math call and code colour argument inside inline and display equations", () => {
    for (const doc of ["$x^2 + y$", "$\n  x^2 + y\n$"]) {
      const from = doc.indexOf("x^2");
      const result = run(doc, from, from + 3, "text-color:#4c8df0");
      expect(result.state.doc.toString()).toBe(doc.replace("x^2", 'text(fill: #rgb("#4c8df0"), x^2)'));
      expect(result.state.sliceDoc(result.state.selection.main.from, result.state.selection.main.to)).toBe("x^2");
    }
    const empty = run("$  $", 2, 2, "text-color:#4c8df0");
    expect(empty.state.doc.toString()).toBe('$ text(fill: #rgb("#4c8df0"), x) $');
    expect(empty.state.sliceDoc(empty.state.selection.main.from, empty.state.selection.main.to)).toBe("x");
    const value = run('$text(fill: red, x)$', 12, 15, "color-value:#4c8df0");
    expect(value.state.doc.toString()).toBe('$text(fill: #rgb("#4c8df0"), x)$');
  });

  it("ignores escaped dollars, comments, raw text, and quoted code when detecting math", () => {
    for (const prefix of ['\\$ ', '// $\n', '/* $ /* nested */ */ ', '`$` ', '```typ\n$\n```\n', '#let price = "$"\n']) {
      const doc = `${prefix}hello`;
      const result = run(doc, prefix.length, doc.length, "text-color:#4c8df0");
      expect(result.state.doc.toString()).toBe(`${prefix}#text(fill: rgb("#4c8df0"))[hello]`);
    }
    const whole = run("$x^2$", 0, 5, "text-color:#4c8df0");
    expect(whole.state.doc.toString()).toBe('#text(fill: rgb("#4c8df0"))[$x^2$]');
  });

  it("detects quoted math and markup content nested inside an equation", () => {
    const math = '$"$" + x$';
    const from = math.indexOf("x");
    expect(run(math, from, from + 1, "text-color:#4c8df0").state.doc.toString())
      .toBe('$"$" + text(fill: #rgb("#4c8df0"), x)$');
    const markup = '$x + #text[hello]$';
    const start = markup.indexOf("hello");
    expect(run(markup, start, start + 5, "text-color:#4c8df0").state.doc.toString())
      .toBe('$x + #text[#text(fill: rgb("#4c8df0"))[hello]]$');
  });

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
