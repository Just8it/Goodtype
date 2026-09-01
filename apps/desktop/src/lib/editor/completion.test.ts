import { describe, expect, it } from "vitest";
import { fromByteOffset, toByteOffset, toSnippetTemplate } from "./completion";

// Rust counts UTF-8 bytes, CodeMirror counts UTF-16 units. Any Typst source with a math symbol
// makes the two disagree, so the round trip is worth pinning down.
describe("offset conversion", () => {
  it("converts, round-trips Unicode boundaries, and clamps past the end", () => {
    const text = "$ ∑ x $";
    expect(toByteOffset(text, 3)).toBe(5);
    expect(fromByteOffset(text, 5)).toBe(3);
    const mixed = "a∑b→c𝕜d";
    for (let index = 0; index <= mixed.length; index += 1) {
      const code = mixed.charCodeAt(index);
      if (code >= 0xdc00 && code <= 0xdfff) continue;
      expect(fromByteOffset(mixed, toByteOffset(mixed, index))).toBe(index);
    }
    expect(fromByteOffset(mixed, 999)).toBe(mixed.length);
  });
});

describe("Tinymist snippets", () => {
  it("turns numbered LSP tab stops into CodeMirror fields", () => {
    expect(toSnippetTemplate("align(${1:body})$0")).toBe("align(#{body})#{}");
  });
});
