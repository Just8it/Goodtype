import { describe, expect, it } from "vitest";
import type { ShapeGeometry } from "../model";
import type { ShapeView } from "../prototype/pageView";
import { shapeTouchedAt, topmostShapeAt } from "./hit";

const shape = (geometry: ShapeGeometry, overrides: Partial<ShapeView> = {}): ShapeView => ({
  id: "shape-1",
  x: 100,
  y: 100,
  rotation: 0,
  scale: 1,
  zIndex: 5,
  readingOrder: 0,
  geometry,
  style: { strokeColor: "#16212b", strokeWidthPt: 2, fillColor: null, opacity: 1 },
  ...overrides,
});

const rectangle = shape({ kind: "rectangle", widthPt: 80, heightPt: 40, cornerRadiusPt: 4 });
const ring = shape({ kind: "ellipse", widthPt: 80, heightPt: 40 });

describe("what the eraser touches", () => {
  it("catches the outline and ignores the empty middle", () => {
    // The middle of an unfilled rectangle has nothing drawn in it. Reaching there should no more
    // erase it than reaching outside it does — the bounding box is not the mark.
    expect(shapeTouchedAt(rectangle, { x: 100, y: 100 }, 6)).toBe(true);
    expect(shapeTouchedAt(rectangle, { x: 140, y: 100 }, 6)).toBe(true);
    expect(shapeTouchedAt(rectangle, { x: 140, y: 120 }, 6)).toBe(false);
    expect(shapeTouchedAt(rectangle, { x: 200, y: 120 }, 6)).toBe(false);
  });

  it("counts the fill of a filled shape as part of it", () => {
    const filled = shape(
      { kind: "rectangle", widthPt: 80, heightPt: 40, cornerRadiusPt: 4 },
      { style: { ...rectangle.style, fillColor: "#16212b24" } },
    );
    expect(shapeTouchedAt(filled, { x: 140, y: 120 }, 6)).toBe(true);
    expect(shapeTouchedAt(filled, { x: 200, y: 120 }, 6)).toBe(false);
  });

  it("follows an ellipse's outline rather than its box", () => {
    // The corner of an ellipse's bounding box is outside the ellipse, and nothing is drawn there.
    expect(shapeTouchedAt(ring, { x: 100, y: 120 }, 4)).toBe(true);
    expect(shapeTouchedAt(ring, { x: 101, y: 101 }, 4)).toBe(false);
  });

  it("follows the shape once it is rotated and scaled", () => {
    const turned = shape(
      { kind: "rectangle", widthPt: 80, heightPt: 40, cornerRadiusPt: 4 },
      { rotation: 90, scale: 2 },
    );
    // Rotated a quarter turn and doubled, the far corner lands at (100, 100) + (-80, 160).
    expect(shapeTouchedAt(turned, { x: 20, y: 260 }, 6)).toBe(true);
    expect(shapeTouchedAt(turned, { x: 180, y: 100 }, 6)).toBe(false);
  });

  it("lets a thicker stroke be caught by its own width", () => {
    const hairline = shape({ kind: "line", start: { x: 0, y: 0 }, end: { x: 80, y: 0 } });
    const thick = shape(
      { kind: "line", start: { x: 0, y: 0 }, end: { x: 80, y: 0 } },
      { style: { ...rectangle.style, strokeWidthPt: 24 } },
    );
    expect(shapeTouchedAt(hairline, { x: 140, y: 111 }, 1)).toBe(false);
    expect(shapeTouchedAt(thick, { x: 140, y: 111 }, 1)).toBe(true);
  });

  it("reaches an open curve but not the region it almost encloses", () => {
    const curve = shape({
      kind: "spline",
      closed: false,
      nodes: [
        { point: { x: 0, y: 0 }, handleIn: null, handleOut: { x: 20, y: 0 } },
        { point: { x: 60, y: 60 }, handleIn: { x: -20, y: 0 }, handleOut: null },
      ],
    });
    expect(shapeTouchedAt(curve, { x: 100, y: 100 }, 4)).toBe(true);
    expect(shapeTouchedAt(curve, { x: 160, y: 160 }, 4)).toBe(true);
    expect(shapeTouchedAt(curve, { x: 220, y: 100 }, 4)).toBe(false);
  });
});

describe("choosing between overlapping shapes", () => {
  it("takes the one on top", () => {
    const under = shape({ kind: "rectangle", widthPt: 80, heightPt: 40, cornerRadiusPt: 0 }, {
      id: "under",
      zIndex: 1,
    });
    const over = shape({ kind: "rectangle", widthPt: 80, heightPt: 40, cornerRadiusPt: 0 }, {
      id: "over",
      zIndex: 9,
    });
    expect(topmostShapeAt([under, over], { x: 100, y: 100 }, 6)?.id).toBe("over");
    expect(topmostShapeAt([over, under], { x: 100, y: 100 }, 6)?.id).toBe("over");
  });

  it("finds nothing where nothing is drawn", () => {
    expect(topmostShapeAt([rectangle, ring], { x: 400, y: 400 }, 6)).toBeNull();
  });
});
