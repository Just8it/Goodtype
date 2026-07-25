import { describe, expect, it } from "vitest";
import { fromByteOffset, toByteOffset } from "./completion";

// Rust counts UTF-8 bytes, CodeMirror counts UTF-16 units. Any Typst source with a math symbol
// makes the two disagree, so the round trip is worth pinning down.
describe("offset conversion", () => {
  it("agrees with the index for ASCII", () => {
    const text = "#image()";
    expect(toByteOffset(text, 6)).toBe(6);
    expect(fromByteOffset(text, 6)).toBe(6);
  });

  it("converts across a multi-byte math symbol", () => {
    // "∑" is 3 UTF-8 bytes but 1 UTF-16 unit.
    const text = "$ ∑ x $";
    expect(toByteOffset(text, 3)).toBe(5);
    expect(fromByteOffset(text, 5)).toBe(3);
  });

  it("round-trips every index of a mixed string", () => {
    const text = "a∑b→c𝕜d";
    for (let index = 0; index <= text.length; index += 1) {
      // Surrogate halves have no byte boundary of their own; skip them.
      const code = text.charCodeAt(index);
      if (code >= 0xdc00 && code <= 0xdfff) continue;
      expect(fromByteOffset(text, toByteOffset(text, index))).toBe(index);
    }
  });

  it("clamps an offset past the end to the document length", () => {
    const text = "= Title";
    expect(fromByteOffset(text, 999)).toBe(text.length);
  });
});
