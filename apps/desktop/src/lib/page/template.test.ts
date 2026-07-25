import { describe, expect, it } from "vitest";
import cases from "../../../../../fixtures/templates/resolved.json";
import type { PageGeometry, PageTemplate } from "../model";
import { resolveTemplate, templateSvg, type TemplateShape } from "./template";

type Case = {
  name: string;
  geometry: PageGeometry;
  template: PageTemplate;
  expected: TemplateShape[];
};

describe("template resolution", () => {
  // The same fixture `goodtype_core::template` asserts against. If the two resolvers drift, one
  // of them fails here and the screen cannot start disagreeing with the PDF unnoticed.
  it.each(cases as Case[])("matches the shared fixture: $name", (specimen) => {
    expect(resolveTemplate(specimen.template, specimen.geometry)).toEqual(specimen.expected);
  });

  it("draws nothing but the paper when a template has no elements", () => {
    const blank: PageTemplate = {
      id: "blank",
      name: "Blank",
      backgroundColor: "#FBFBF9",
      marginPt: 0,
      elements: [],
    };
    const svg = templateSvg(blank, { widthPt: 100, heightPt: 100 });
    expect(svg).toContain(`fill="#FBFBF9"`);
    expect(svg).not.toContain("<line");
    expect(svg).not.toContain("<circle");
  });

  it("refuses spacings dense enough to flood the page", () => {
    const dense: PageTemplate = {
      id: "dense",
      name: "Dense",
      backgroundColor: "#FBFBF9",
      marginPt: 0,
      // Below MIN_SPACING_PT. Resolving is not the place to raise it, so it draws nothing rather
      // than a million lines; validation on the way to disk is what actually rejects it.
      elements: [
        { kind: "horizontal_lines", spacingPt: 0.01, offsetPt: 0, color: "#DDE1E6", weightPt: 0.5 },
      ],
    };
    expect(resolveTemplate(dense, { widthPt: 600, heightPt: 800 })).toEqual([]);
  });
});
