import { describe, expect, it } from "vitest";
import { blockFont, withBlockFont } from "./blockFont";

describe("inline block font", () => {
  it("replaces its own setting while preserving the entire original body", () => {
    const source = '= Notes\n\nExample: `#set text(font: "Arial")`\n$ F = m a $';
    const chosen = withBlockFont(source, "Segoe Print");
    expect(chosen).toContain('#set text(font: "Segoe Print")');
    expect(blockFont(chosen)).toBe("Segoe Print");
    expect(withBlockFont(chosen, "Arial")).toBe(withBlockFont(source, "Arial"));
    expect(withBlockFont(chosen, "")).toBe(source);
    expect(withBlockFont(chosen.replaceAll("\n", "\r\n"), "")).toBe(source.replaceAll("\n", "\r\n"));
  });
  it("escapes font names as Typst strings and does not edit ordinary source rules", () => {
    const source = '#set text(font: "Libertinus Serif")\nHello';
    expect(withBlockFont(source, 'Family "A"\\B')).toBe('// Goodtype block font\n#set text(font: "Family \\"A\\"\\\\B")\n\n' + source);
    expect(withBlockFont(source, "")).toBe(source);
    expect(blockFont(withBlockFont(source, 'Family "A"\\B'))).toBe('Family "A"\\B');
    expect(blockFont('// Goodtype block font\n#set text(font: "\\q")\n\nbody')).toBe("");
  });
});
