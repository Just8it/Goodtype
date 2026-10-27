import { describe, expect, it } from "vitest";
import { PAPER_TONES, templateGroups } from "./templates";
import { pageTextLayout } from "./pageText";
import { resolveTemplate } from "./template";

describe("page text paper alignment", () => {
  it("follows stored ruled geometry and uses two rows on dense grids", () => {
    const templates = templateGroups(PAPER_TONES[0]).flatMap((group) => group.templates);
    const geometry = { widthPt: 595.2756, heightPt: 841.8898 };
    const ruledTemplate = templates.find((template) => template.id.startsWith("ruled-wide"))!;
    const gridTemplate = templates.find((template) => template.id.startsWith("squared-5mm"))!;
    const ruled = pageTextLayout(
      { kind: "template", template: ruledTemplate },
      geometry,
    );
    const grid = pageTextLayout(
      { kind: "template", template: gridTemplate },
      geometry,
    );

    expect(ruled.lineSpacingPt).toBeCloseTo(8.7 * 72 / 25.4);
    expect(grid.lineSpacingPt).toBeCloseTo(10 * 72 / 25.4);
    expect(ruled.y).toBeGreaterThan(0);
    expect(grid.x).toBeGreaterThan(0);
    const ruledGuide = ruledTemplate.elements.find((element) => element.kind === "horizontal_lines")!;
    const firstRule = Math.min(...resolveTemplate(ruledTemplate, geometry)
      .flatMap((shape) => shape.kind === "line" && shape.y1 === shape.y2 ? [shape.y1] : []));
    expect(ruled.x).toBeCloseTo(ruledGuide.area.leftPt + 2);
    expect(ruled.y + 11).toBeCloseTo(firstRule - 2);
    const firstGridColumn = Math.min(...resolveTemplate(gridTemplate, geometry)
      .flatMap((shape) => shape.kind === "line" && shape.x1 === shape.x2 ? [shape.x1] : []));
    expect(grid.x).toBeCloseTo(firstGridColumn + 2);
  });

});
