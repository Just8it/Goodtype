import { describe, expect, it } from "vitest";
import type { EraserSweep } from "../ink/erase";
import type { BezierNode, ShapeGeometry, ShapeStyle } from "../model";
import type { ShapeView } from "../prototype/pageView";
import { cutShape, eraseShapesThrough } from "./erase";
import { splinePolyline } from "./geometry";

const style: ShapeStyle = { strokeColor: "#16212b", strokeWidthPt: 2, fillColor: null, opacity: 1 };

function shape(geometry: ShapeGeometry, overrides: Partial<ShapeView> = {}): ShapeView {
  return { id: "shape", x: 0, y: 0, rotation: 0, scale: 1, zIndex: 3, readingOrder: 1, geometry, style, ...overrides };
}

let counter = 0;
const newId = () => `piece-${++counter}`;
const vertical = (x: number, from = -20, to = 20, radius = 5): EraserSweep => ({ from: { x, y: from }, to: { x, y: to }, radius });

function nodesOf(piece: ShapeView): BezierNode[] {
  if (piece.geometry.kind !== "spline") throw new Error(`expected a spline, got ${piece.geometry.kind}`);
  return piece.geometry.nodes;
}

describe("precise eraser on shapes", () => {
  it("cuts a line into two lines whose round caps just clear the eraser", () => {
    const line = shape({ kind: "line", start: { x: 0, y: 0 }, end: { x: 100, y: 0 } });
    const pieces = cutShape(line, [vertical(50)], newId)!;
    expect(pieces).toHaveLength(2);
    const [left, right] = pieces.map((piece) => piece.geometry) as Extract<ShapeGeometry, { kind: "line" }>[];
    expect(left.kind).toBe("line");
    expect(left.start).toEqual({ x: 0, y: 0 });
    expect(left.end.x).toBeLessThanOrEqual(44);
    expect(left.end.x).toBeGreaterThan(43.99);
    expect(right.start.x).toBeGreaterThanOrEqual(56);
    expect(right.start.x).toBeLessThan(56.01);
    expect(right.end).toEqual({ x: 100, y: 0 });
    expect(pieces[0].id).toBe("shape");
    expect(pieces[1].readingOrder).toBe(1);
    expect(pieces[1].zIndex).toBe(3);
  });

  it("opens a rectangle into one spline that runs round its corners without handles", () => {
    const box = shape({ kind: "rectangle", widthPt: 100, heightPt: 60, cornerRadiusPt: 0 });
    const [piece] = cutShape(box, [vertical(50)], newId)!;
    const nodes = nodesOf(piece);
    const points = nodes.map((node) => [Number(node.point.x.toFixed(6)), Number(node.point.y.toFixed(6))]);
    expect(points.slice(1, -1)).toEqual([[100, 0], [100, 60], [0, 60], [0, 0]]);
    expect(points[0][0]).toBeGreaterThanOrEqual(56);
    expect(points.at(-1)![0]).toBeLessThanOrEqual(44);
    expect(nodes.every((node) => node.handleIn === null && node.handleOut === null)).toBe(true);
    expect(piece.geometry.kind === "spline" && piece.geometry.closed).toBe(false);
  });

  it("keeps a rounded corner exactly", () => {
    const box = shape({ kind: "rectangle", widthPt: 100, heightPt: 60, cornerRadiusPt: 10 });
    const [piece] = cutShape(box, [vertical(50)], newId)!;
    // The quadratic corner at the top right, raised to a cubic: its handles sit 2/3 of the way
    // from each end to the quadratic's control point (100, 0).
    const corner = nodesOf(piece).find((node) => Math.abs(node.point.x - 90) < 1e-9 && node.point.y === 0)!;
    expect(corner.handleOut!.x).toBeCloseTo(20 / 3, 9);
    expect(corner.handleOut!.y).toBeCloseTo(0, 9);
  });

  it("turns a cut ellipse into arcs that stay on the ellipse", () => {
    const oval = shape({ kind: "ellipse", widthPt: 300, heightPt: 200 });
    const [piece] = cutShape(oval, [{ from: { x: 290, y: 100 }, to: { x: 310, y: 100 }, radius: 4 }], newId)!;
    const walk = splinePolyline(nodesOf(piece), false, 64);
    const worst = Math.max(...walk.map((point) =>
      Math.abs(Math.hypot((point.x - 150) / 150, (point.y - 100) / 100) - 1) * 100));
    // Radial error of 45° cubic arcs is 4.2e-6 of the radius.
    expect(worst).toBeLessThan(0.004);
    expect(walk[0].y).toBeLessThan(100 - 4);
    expect(walk.at(-1)!.y).toBeGreaterThan(100 + 4);
  });

  it("splits a spline exactly along the curve it was", () => {
    const curve = shape({ kind: "spline", closed: false, nodes: [
      { point: { x: 0, y: 0 }, handleIn: null, handleOut: { x: 40, y: -60 } },
      { point: { x: 100, y: 0 }, handleIn: { x: -40, y: -60 }, handleOut: null },
    ] });
    const pieces = cutShape(curve, [vertical(50, -80, 0)], newId)!;
    expect(pieces).toHaveLength(2);
    const original = splinePolyline((curve.geometry as Extract<ShapeGeometry, { kind: "spline" }>).nodes, false, 4000);
    const nearest = (x: number, y: number) => Math.min(...original.map((point) => Math.hypot(point.x - x, point.y - y)));
    for (const piece of pieces) {
      for (const point of splinePolyline(nodesOf(piece), false, 16)) {
        expect(nearest(point.x, point.y)).toBeLessThan(0.05);
      }
    }
  });

  it("continues a dashed outline's dashes from where each piece began", () => {
    const dashed = { ...style, pattern: "dashed" as const };
    const line = shape({ kind: "line", start: { x: 0, y: 0 }, end: { x: 100, y: 0 } }, { style: dashed });
    const [left, right] = cutShape(line, [vertical(50)], newId)!;
    expect(left.style.patternOffsetPt).toBeUndefined();
    const start = (right.geometry as Extract<ShapeGeometry, { kind: "line" }>).start.x;
    expect(right.style.patternOffsetPt).toBeCloseTo(start, 6);
  });

  it("keeps a patterned closed outline in two pieces at its seam so no dash moves", () => {
    const dashed = { ...style, pattern: "dashed" as const };
    const box = shape({ kind: "rectangle", widthPt: 100, heightPt: 60, cornerRadiusPt: 0 }, { style: dashed });
    const pieces = cutShape(box, [vertical(50)], newId)!;
    expect(pieces).toHaveLength(2);
    expect(pieces[0].style.patternOffsetPt).toBeUndefined();
    expect(nodesOf(pieces[0])[0].point).toEqual({ x: 0, y: 0 });
    expect(pieces[1].style.patternOffsetPt).toBeCloseTo(nodesOf(pieces[1])[0].point.x, 6);
  });

  it("erases a filled shape whole when touched, including through its tint", () => {
    const filled = shape({ kind: "rectangle", widthPt: 100, heightPt: 60, cornerRadiusPt: 0 },
      { style: { ...style, fillColor: "#ff000024" } });
    expect(cutShape(filled, [{ from: { x: 50, y: 30 }, to: { x: 52, y: 30 }, radius: 3 }], newId)).toEqual([]);
    expect(cutShape(filled, [vertical(150)], newId)).toBeNull();
  });

  it("cuts a rotated, scaled shape where it is drawn on the page", () => {
    // A 50 pt line at twice the size, turned a quarter: on the page it runs down x = 10.
    const line = shape({ kind: "line", start: { x: 0, y: 0 }, end: { x: 50, y: 0 } }, { x: 10, y: 0, scale: 2, rotation: 90 });
    const [top, bottom] = cutShape(line, [{ from: { x: 0, y: 50 }, to: { x: 20, y: 50 }, radius: 5 }], newId)!;
    // Half-width 2 on the page (2 pt × scale 2 / 2), so the cut ends sit 7 pt from y = 50.
    expect((top.geometry as Extract<ShapeGeometry, { kind: "line" }>).end.x * 2).toBeLessThanOrEqual(43 + 1e-9);
    expect((bottom.geometry as Extract<ShapeGeometry, { kind: "line" }>).start.x * 2).toBeGreaterThanOrEqual(57 - 1e-9);
  });

  it("returns the same list when nothing was touched", () => {
    const shapes = [shape({ kind: "line", start: { x: 0, y: 0 }, end: { x: 100, y: 0 } })];
    expect(eraseShapesThrough(shapes, [vertical(50, 20, 40)], newId).shapes).toBe(shapes);
  });
});
