import { describe, expect, it } from "vitest";
import type { ShapeGeometry } from "../model";
import type { ShapeView } from "../prototype/pageView";
import {
  anchorPoints,
  canCloseSpline,
  moveShapeAnchor,
  removeSplineNode,
  resizeShape,
  rotateShape,
  rotationHandlePoint,
  toggleSplineClosed,
} from "./edit";
import { MIN_SHAPE_SIZE_PT, shapeBounds } from "./geometry";

const rectangle: ShapeView = {
  id: "shape-1",
  x: 10,
  y: 20,
  rotation: 0,
  scale: 1,
  zIndex: 1,
  readingOrder: 0,
  geometry: { kind: "rectangle", widthPt: 80, heightPt: 40, cornerRadiusPt: 4 },
  style: { strokeColor: "#16212b", strokeWidthPt: 1.6, fillColor: null, opacity: 1 },
};

describe("shape editing", () => {
  it("keeps the opposite corner fixed when a rectangle anchor moves", () => {
    const changed = moveShapeAnchor(rectangle, "north-west", { x: 10, y: 5 });
    expect(changed).toMatchObject({
      x: 20,
      y: 25,
      geometry: { widthPt: 70, heightPt: 35 },
    });
  });

  it("supports proportional bounding resize without changing stroke style", () => {
    const changed = resizeShape(rectangle, { x: 20, y: 0 }, true);
    expect(changed.geometry).toMatchObject({ widthPt: 100, heightPt: 50 });
    expect(changed.style).toBe(rectangle.style);
  });
});

describe("shape editing keeps geometry the store will accept", () => {
  it("refuses to collapse a line onto its own endpoint", () => {
    const line: ShapeView = {
      ...rectangle,
      id: "line-1",
      geometry: { kind: "line", start: { x: 0, y: 0 }, end: { x: 40, y: 0 } },
    };
    const dragged = moveShapeAnchor(line, "line-start", { x: 60, y: 0 });
    const geometry = dragged.geometry as Extract<ShapeGeometry, { kind: "line" }>;
    expect(geometry.start).not.toEqual(geometry.end);
    expect(Math.hypot(geometry.start.x - geometry.end.x, geometry.start.y - geometry.end.y))
      .toBeGreaterThanOrEqual(MIN_SHAPE_SIZE_PT);
  });

  it("offers corners and edges for both boxed shapes", () => {
    const ellipse: ShapeGeometry = { kind: "ellipse", widthPt: 80, heightPt: 40 };
    expect(anchorPoints(rectangle.geometry).map((item) => item.anchor)).toEqual(
      anchorPoints(ellipse).map((item) => item.anchor),
    );
    expect(anchorPoints(ellipse)).toHaveLength(8);
  });

  it("shrinks and grows at the same rate when proportions are kept", () => {
    const grown = resizeShape(rectangle, { x: 40, y: 0 }, true).geometry;
    const shrunk = resizeShape(rectangle, { x: -40, y: 0 }, true).geometry;
    expect(grown).toMatchObject({ widthPt: 120, heightPt: 60 });
    expect(shrunk).toMatchObject({ widthPt: 40, heightPt: 20 });
  });
});

describe("shape rotation", () => {
  const centreOf = (shape: ShapeView) => {
    const bounds = shapeBounds(shape.geometry);
    const local = {
      x: (bounds.left + bounds.right) / 2,
      y: (bounds.top + bounds.bottom) / 2,
    };
    const angle = (shape.rotation * Math.PI) / 180;
    const x = local.x * shape.scale;
    const y = local.y * shape.scale;
    return {
      x: shape.x + x * Math.cos(angle) - y * Math.sin(angle),
      y: shape.y + x * Math.sin(angle) + y * Math.cos(angle),
    };
  };

  it("turns the shape in place rather than swinging it around its origin", () => {
    const before = centreOf(rectangle);
    const grabbed = rotationHandlePoint(rectangle.geometry, 28);
    const turned = rotateShape(rectangle, grabbed, { x: 40, y: 40 });
    const after = centreOf(turned);
    expect(turned.rotation).not.toBeCloseTo(0);
    expect(after.x).toBeCloseTo(before.x, 6);
    expect(after.y).toBeCloseTo(before.y, 6);
  });

  it("snaps to fifteen degree steps when asked", () => {
    const grabbed = rotationHandlePoint(rectangle.geometry, 28);
    const turned = rotateShape(rectangle, grabbed, { x: 40, y: 40 }, true);
    expect(turned.rotation % 15).toBeCloseTo(0, 6);
  });
});

describe("spline editing", () => {
  const spline = (count: number): ShapeView => ({
    ...rectangle,
    id: "spline-1",
    geometry: {
      kind: "spline",
      closed: false,
      nodes: Array.from({ length: count }, (_, index) => ({
        point: { x: index * 10, y: index % 2 === 0 ? 0 : 10 },
        handleIn: null,
        handleOut: null,
      })),
    },
  });

  it("closes only a curve with enough anchors to enclose anything", () => {
    expect(canCloseSpline(spline(2).geometry)).toBe(false);
    expect(toggleSplineClosed(spline(2)).geometry).toMatchObject({ closed: false });
    expect(toggleSplineClosed(spline(4)).geometry).toMatchObject({ closed: true });
  });

  it("keeps enough anchors behind when one is removed", () => {
    expect(removeSplineNode(spline(4), 1).geometry).toMatchObject({
      nodes: [{ point: { x: 0 } }, { point: { x: 20 } }, { point: { x: 30 } }],
    });
    expect(removeSplineNode(spline(2), 0).geometry).toMatchObject({
      nodes: [{ point: { x: 0 } }, { point: { x: 10 } }],
    });
    const closed = toggleSplineClosed(spline(3));
    expect(removeSplineNode(closed, 0).geometry).toMatchObject({ closed: true });
    expect(
      (removeSplineNode(closed, 0).geometry as Extract<ShapeGeometry, { kind: "spline" }>).nodes,
    ).toHaveLength(3);
  });
});
