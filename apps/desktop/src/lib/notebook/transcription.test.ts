import { describe, expect, it } from "vitest";

import type { Stroke } from "../model";
import { editDistance, inkPolylines, polylineBounds, proseToTypst, shapePolylines, transcriptionPlacement, transcriptionSource } from "./transcription";
import type { ShapeView } from "./pageView";
import { toLocalPoint, toPagePoint } from "../shape/hit";

describe("transcription", () => {
  it("escapes prose so Typst reads it back literally", () => {
    expect(proseToTypst("Druck p = 5 #bar")).toBe("Druck p = 5 \\#bar");
    expect(proseToTypst("a_1 * b $x$ <ref> @cite [1] ~ km/h")).toBe(
      "a\\_1 \\* b \\$x\\$ \\<ref\\> \\@cite \\[1\\] \\~ km\\/h",
    );
    expect(proseToTypst("// not a comment")).toBe("\\/\\/ not a comment");
    expect(proseToTypst("= not a heading")).toBe("\\= not a heading");
    expect(proseToTypst("- not a list")).toBe("\\- not a list");
    expect(proseToTypst("1. Hauptsatz")).toBe("1\\. Hauptsatz");
    expect(proseToTypst("a--b")).toBe("a\\--b");
  });

  it("joins handwritten line breaks into one paragraph", () => {
    expect(proseToTypst("  Die Strömung\nist  reibungsfrei \n")).toBe("Die Strömung ist reibungsfrei");
  });

  it("wraps math content as display math", () => {
    expect(transcriptionSource("math", " u = nabla phi ")).toBe("$ u = nabla phi $");
    expect(transcriptionSource("text", "x")).toBe("x");
  });

  it("counts character corrections", () => {
    expect(editDistance("", "")).toBe(0);
    expect(editDistance("phi", "phi")).toBe(0);
    expect(editDistance("Stromung", "Strömung")).toBe(1);
    expect(editDistance("alpha", "")).toBe(5);
    expect(editDistance("kitten", "sitting")).toBe(3);
  });

  it("sends transformed page points of the selected strokes only", () => {
    const stroke = (id: string, translateX: number): Stroke => ({
      id,
      widthPt: 1.5,
      points: [{ x: 1, y: 2, pressure: 0.5 }],
      transform: { translateX, translateY: 0, scaleX: 2, scaleY: -3, rotation: 0 },
    }) as unknown as Stroke;
    expect(inkPolylines([stroke("a", 10), stroke("b", 0)], ["a"])).toEqual([{ widthPt: 4.5, points: [[12, -6]] }]);
  });

  it("sends selected shape lines in page points with their width", () => {
    const style = { strokeColor: "#000", strokeWidthPt: 1.5, fillColor: null, opacity: 1 };
    const bar: ShapeView = {
      id: "bar", x: 100, y: 50, rotation: 0, scale: 2, zIndex: 1, readingOrder: 0,
      geometry: { kind: "line", start: { x: 0, y: 0 }, end: { x: 30, y: 0 } }, style,
    };
    const other: ShapeView = { ...bar, id: "other" };
    expect(shapePolylines([bar, other], ["bar", "unrelated"])).toEqual([{ widthPt: 3, points: [[100, 50], [160, 50]] }]);
    const box: ShapeView = { ...bar, id: "box", scale: 1, geometry: { kind: "rectangle", widthPt: 10, heightPt: 4, cornerRadiusPt: 0 } };
    expect(shapePolylines([box], ["box"])[0].points).toEqual([[100, 50], [110, 50], [110, 54], [100, 54], [100, 50]]);
    const ring: ShapeView = { ...bar, id: "ring", scale: 1, geometry: { kind: "ellipse", widthPt: 20, heightPt: 10 } };
    expect(polylineBounds(shapePolylines([ring], ["ring"]))).toEqual({ left: 100, top: 50, right: 120, bottom: 60 });
  });

  it("maps shape points to the page as the exact inverse of hit testing", () => {
    const shape = { x: 12, y: -4, rotation: 33, scale: 1.7 } as ShapeView;
    const page = toPagePoint(shape, { x: 5, y: -8 });
    const back = toLocalPoint(shape, page);
    expect(back.x).toBeCloseTo(5, 9);
    expect(back.y).toBeCloseTo(-8, 9);
  });

  it("bounds ink and shapes together", () => {
    expect(polylineBounds([])).toBeNull();
    expect(polylineBounds([{ widthPt: 1, points: [[3, 9], [7, 2]] }, { widthPt: 1, points: [[-1, 4]] }]))
      .toEqual({ left: -1, top: 2, right: 7, bottom: 9 });
  });

  it("places the block under the ink and keeps it on the page", () => {
    const page = { widthPt: 595, heightPt: 842 };
    expect(transcriptionPlacement({ left: 60, top: 100, right: 300, bottom: 140 }, page)).toEqual({ x: 60, y: 146, layoutWidthPt: 240 });
    expect(transcriptionPlacement({ left: 60, top: 100, right: 80, bottom: 140 }, page).layoutWidthPt).toBe(120);
    const low = transcriptionPlacement({ left: 560, top: 800, right: 590, bottom: 830 }, page);
    expect(low.y).toBeLessThan(800);
    expect(low.x + low.layoutWidthPt).toBeLessThanOrEqual(page.widthPt - 12);
  });

  it("bounds selections at the backend point limit without overflowing the call stack", () => {
    const points = Array.from({ length: 200_000 }, (_, i): [number, number] => [i % 1000, i % 500]);
    expect(polylineBounds([{ widthPt: 1, points }])).toEqual({ left: 0, top: 0, right: 999, bottom: 499 });
  });
});
