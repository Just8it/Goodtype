import { describe, expect, it } from "vitest";
import { selectStrokesInLasso } from "../ink/selection";
import type { Stroke } from "../model";
import { DEFAULT_LASSO_TARGETS, boxOutline, combinedBounds, selectLassoObjects, shapeLassoItem, transformSelectionPoint } from "./lasso";
import type { ShapeView } from "./pageView";

const polygon = boxOutline(10, 10, 20, 20);
const stroke = (id: string, tool: "pen" | "highlighter", points = [{ x: 0, y: 20 }, { x: 40, y: 20 }]): Stroke => ({
  id, tool, points, transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
} as Stroke);

describe("lasso categories and geometry", () => {
  it("catches sparse handwriting and highlight segments crossing the loop, with independent filters", () => {
    const strokes = [stroke("pen", "pen"), stroke("highlight", "highlighter"), stroke("outside", "pen", [{ x: 40, y: 40 }])];
    expect(selectStrokesInLasso(strokes, polygon)).toEqual(["pen", "highlight"]);
    expect(selectStrokesInLasso(strokes, polygon, stroke => stroke.tool === "pen")).toEqual(["pen"]);
    expect(selectStrokesInLasso(strokes, polygon, stroke => stroke.tool === "highlighter")).toEqual(["highlight"]);
    expect(selectStrokesInLasso(strokes, [])).toEqual([]);
  });

  it("tests transformed ink instead of its original samples", () => {
    const ink = stroke("moved", "pen", [{ x: 0, y: 0 }]);
    ink.transform.translateX = 20; ink.transform.translateY = 20;
    expect(selectStrokesInLasso([ink], polygon)).toEqual(["moved"]);
  });

  it("selects all enabled object kinds and ignores disabled ones, including a lasso within an image", () => {
    const items = (["shapes", "images", "text"] as const).map(kind => ({ id: kind, kind, closed: true, outline: boxOutline(0, 0, 50, 50) }));
    expect(selectLassoObjects(items, polygon, DEFAULT_LASSO_TARGETS)).toEqual(["shapes", "images", "text"]);
    expect(selectLassoObjects(items, polygon, { ...DEFAULT_LASSO_TARGETS, images: false })).toEqual(["shapes", "text"]);
    expect(selectLassoObjects(items, boxOutline(60, 60, 10, 10), DEFAULT_LASSO_TARGETS)).toEqual([]);
  });

  it("uses rotated/scaled shape outlines and does not select the empty corner of an ellipse's box", () => {
    const shape = { id: "shape", x: 20, y: 20, scale: 2, rotation: 90,
      geometry: { kind: "line", start: { x: 0, y: 0 }, end: { x: 30, y: 0 } } } as ShapeView;
    expect(selectLassoObjects([shapeLassoItem(shape)], boxOutline(15, 40, 10, 10), DEFAULT_LASSO_TARGETS)).toEqual(["shape"]);
    shape.geometry = { kind: "ellipse", widthPt: 100, heightPt: 100 };
    shape.x = 0; shape.y = 0; shape.rotation = 0; shape.scale = 1;
    expect(selectLassoObjects([shapeLassoItem(shape)], boxOutline(0, 0, 3, 3), DEFAULT_LASSO_TARGETS)).toEqual([]);
  });

  it("combines ink/object bounds and applies one affine move/scale to every material", () => {
    const item = { id: "image", kind: "images" as const, closed: true, outline: boxOutline(40, 50, 20, 30) };
    expect(combinedBounds([item], { left: 10, top: 15, right: 20, bottom: 25 }))
      .toEqual({ left: 10, top: 15, right: 60, bottom: 80 });
    expect(combinedBounds([], null)).toBeNull();
    expect(transformSelectionPoint({ x: 30, y: 40 }, { x: 5, y: -2 }, 2, { x: 10, y: 10 }))
      .toEqual({ x: 55, y: 68 });
  });
});
