import { describe, expect, it } from "vitest";
import type { ShapeGeometry } from "../model";
import type { ShapeView } from "../notebook/pageView";
import { snapPoint, snapTargets } from "./snap";

const shape = (id: string, geometry: ShapeGeometry, overrides: Partial<ShapeView> = {}): ShapeView => ({
  id,
  x: 100,
  y: 100,
  rotation: 0,
  scale: 1,
  zIndex: 1,
  readingOrder: 0,
  geometry,
  style: { strokeColor: "#16212b", strokeWidthPt: 2, fillColor: null, opacity: 1 },
  ...overrides,
});

const line = shape("line", { kind: "line", start: { x: 0, y: 0 }, end: { x: 50, y: 0 } });
const box = shape("box", { kind: "rectangle", widthPt: 40, heightPt: 20, cornerRadiusPt: 4 }, { x: 200 });

describe("end snapping", () => {
  it("offers line ends, rectangle corners and open spline ends, but nothing on an ellipse", () => {
    const spline = shape("spline", { kind: "spline", closed: false, nodes: [
      { point: { x: 0, y: 0 }, handleIn: null, handleOut: null }, { point: { x: 10, y: 10 }, handleIn: null, handleOut: null }, { point: { x: 20, y: 0 }, handleIn: null, handleOut: null },
    ] }, { x: 0, y: 0 });
    const ellipse = shape("ellipse", { kind: "ellipse", widthPt: 30, heightPt: 30 });
    expect(snapTargets([line, box, spline, ellipse])).toEqual([
      { x: 100, y: 100 }, { x: 150, y: 100 },
      { x: 200, y: 100 }, { x: 240, y: 100 }, { x: 240, y: 120 }, { x: 200, y: 120 },
      { x: 0, y: 0 }, { x: 20, y: 0 },
    ]);
  });

  it("picks the nearest target and ignores those outside the radius", () => {
    const targets = snapTargets([line, box]);
    expect(snapPoint({ x: 147, y: 103 }, targets, 10)).toEqual({ x: 150, y: 100 });
    expect(snapPoint({ x: 238, y: 118 }, targets, 10)).toEqual({ x: 240, y: 120 });
    expect(snapPoint({ x: 175, y: 100 }, targets, 10)).toBeNull();
  });

  it("leaves out the shape being edited", () => {
    expect(snapPoint({ x: 150, y: 100 }, snapTargets([line, box], "line"), 10)).toBeNull();
  });

  it("places the ends of a rotated, scaled shape where the page shows them", () => {
    const turned = { ...line, rotation: 90, scale: 2 };
    const [start, end] = snapTargets([turned]);
    expect(start).toEqual({ x: 100, y: 100 });
    expect(end.x).toBeCloseTo(100);
    expect(end.y).toBeCloseTo(200);
  });
});
