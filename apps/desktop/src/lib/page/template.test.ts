import { describe, expect, it } from "vitest";
import cases from "../../../../../fixtures/templates/resolved.json";
import type { PageGeometry, PageTemplate } from "../model";
import { resolveTemplate, templateSvg, type TemplateShape } from "./template";
import { PAPER_TONES, templateGroups } from "./templates";

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
      // Below MIN_SPACING_PT. Resolving is not the place to raise it, so it draws nothing rather
      // than a million lines; validation on the way to disk is what actually rejects it.
      elements: [
        {
          kind: "horizontal_lines",
          area: { topPt: 0, rightPt: 0, bottomPt: 0, leftPt: 0 },
          spacingPt: 0.01,
          color: "#DDE1E6",
          weightPt: 0.5,
        },
      ],
    };
    expect(resolveTemplate(dense, { widthPt: 600, heightPt: 800 })).toEqual([]);
  });

  // The complaint that produced the centred layout: a 5mm grid must be exactly 5mm and leave the
  // same margin at both edges, on every page size rather than just the one it was tuned on.
  it.each([
    { name: "A5", widthPt: 419.5276, heightPt: 595.2756 },
    { name: "A4", widthPt: 595.2756, heightPt: 841.8898 },
    { name: "A3", widthPt: 841.8898, heightPt: 1190.5512 },
    { name: "Letter", widthPt: 612, heightPt: 792 },
  ])("keeps 5mm squares exact and margins equal on $name", (geometry) => {
    const squared = templateGroups(PAPER_TONES[0])
      .flatMap((group) => group.templates)
      .find((entry) => entry.name === "Squared")!;
    const verticals = resolveTemplate(squared, geometry).filter(
      (shape) => shape.kind === "line" && shape.x1 === shape.x2,
    ) as Extract<TemplateShape, { kind: "line" }>[];

    expect(verticals.length).toBeGreaterThan(4);
    const spacing = 5 * (72 / 25.4);
    for (let index = 1; index < verticals.length; index += 1) {
      expect(verticals[index].x1 - verticals[index - 1].x1).toBeCloseTo(spacing, 9);
    }
    const left = verticals[0].x1 - 36;
    const right = geometry.widthPt - 36 - verticals[verticals.length - 1].x1;
    expect(left).toBeCloseTo(right, 9);
  });
});
