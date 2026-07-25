import { describe, expect, it } from "vitest";
import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { STEM_SNIPPETS, typstSnippetSource } from "./snippets";

function completeAt(document: string, position: number, explicit: boolean) {
  const state = EditorState.create({ doc: document });
  return typstSnippetSource(new CompletionContext(state, position, explicit));
}

describe("STEM snippet completion", () => {
  it("offers every snippet on explicit invocation", () => {
    const result = completeAt("", 0, true);
    expect(result?.options.length).toBe(STEM_SNIPPETS.length);
  });

  it("matches from a typed prefix but stays quiet for single letters", () => {
    expect(completeAt("ma", 2, false)?.from).toBe(0);
    expect(completeAt("m", 1, false)).toBeNull();
    expect(completeAt("", 0, false)).toBeNull();
  });

  it("inserts ordinary Typst source only", () => {
    for (const snippet of STEM_SNIPPETS) {
      // No script execution, imports, or file access in any template.
      expect(snippet.template).not.toMatch(/#(import|include|read|eval)\b/);
    }
    expect(STEM_SNIPPETS.some((snippet) => snippet.label === "matrix")).toBe(true);
    expect(STEM_SNIPPETS.some((snippet) => snippet.label === "integral")).toBe(true);
  });
});
