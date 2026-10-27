import { describe, expect, it } from "vitest";
import cases from "../../../../../fixtures/templates/resolved.json";
import type { PageGeometry, PageTemplate } from "../model";
import { MAX_SHAPES, resolveTemplate, templatePreviewSvg, templateSvg, type TemplateShape } from "./template";
import { render } from "svelte/server";
import NotebookCover from "../library/NotebookCover.svelte";
import { PAPER_TONES, templateGroups } from "./templates";

type Case = {
  name: string;
  geometry: PageGeometry;
  template: PageTemplate;
  expected: TemplateShape[];
};

describe("template resolution", () => {
  const geometry = { widthPt: 100, heightPt: 100 };
  const area = { topPt: 0, rightPt: 0, bottomPt: 0, leftPt: 0 };
  const blank: PageTemplate = { id: "custom", name: "Custom", backgroundColor: "#fFfFfF80", elements: [] };

  it.each([
    '#ffffff"/><style>body{opacity:0}</style><rect fill="',
    '#ffffff" onload="alert(1)',
    '&quot;/><foreignObject><form></form></foreignObject>',
    'url(https://example.invalid/paint.svg#x)',
  ])("refuses untrusted SVG paints in backgrounds, lines, dots, and major rules: %s", (payload) => {
    const templates: PageTemplate[] = [
      { ...blank, backgroundColor: payload },
      { ...blank, elements: [{ kind: "horizontal_lines", area, spacingPt: 10, color: payload, weightPt: 1 }] },
      { ...blank, elements: [{ kind: "dots", area, spacingPt: 10, color: payload, radiusPt: 1 }] },
      { ...blank, elements: [{ kind: "grid", area, spacingPt: 10, color: "#abcdef", weightPt: 1, major: { every: 2, color: payload, weightPt: 2 } }] },
    ];
    for (const template of templates) {
      for (const draw of [templateSvg, templatePreviewSvg]) expect(draw(template, geometry)).toBe("");
      const { body } = render(NotebookCover, { props: {
        paper: { geometry, background: { kind: "template", template } }, path: null, widthPx: 100,
      } });
      expect(body).not.toMatch(/<(?:style|form|foreignObject)\b|onload="alert/);
    }
  });

  it("retains canonical mixed-case and alpha paints in page and preview markup", () => {
    const template: PageTemplate = { ...blank, elements: [{ kind: "horizontal_lines", area, spacingPt: 10, color: "#AbCdEf", weightPt: 1 }] };
    for (const draw of [templateSvg, templatePreviewSvg]) {
      const svg = draw(template, geometry);
      expect(svg).toContain('fill="#fFfFfF80"');
      expect(svg).toContain('stroke="#AbCdEf"');
    }
    expect(templatePreviewSvg(template, geometry)).toContain('viewBox="0 0 45 45"');
  });

  it("bounds dotted shelf paper before allocating the full grid", () => {
    const template: PageTemplate = { ...blank, elements: [{
      kind: "dots", area, spacingPt: 3, color: "#abcdef", radiusPt: 1,
    }] };
    const largestPage = { widthPt: 100_000, heightPt: 100_000 };
    expect(resolveTemplate(template, largestPage)).toHaveLength(MAX_SHAPES);
    expect(templateSvg(template, largestPage).match(/<circle /g)).toHaveLength(MAX_SHAPES);
  }, 2_000);

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
    // Symmetric about the page centre, which is the same statement as equal margins without
    // depending on what the library happens to use as its inset today.
    const first = verticals[0].x1;
    const last = verticals[verticals.length - 1].x1;
    expect(first).toBeCloseTo(geometry.widthPt - last, 9);

    // And the grid's corners are closed: every rule stops on the outermost line of the other axis.
    const horizontals = resolveTemplate(squared, geometry).filter(
      (shape) => shape.kind === "line" && shape.y1 === shape.y2,
    ) as Extract<TemplateShape, { kind: "line" }>[];
    for (const rule of horizontals) {
      expect([rule.x1, rule.x2]).toEqual([first, last]);
    }
  });
});
