import { describe, expect, it } from "vitest";
import type { ShapeStyle } from "../model";
import { shapeDashArray, shapeFromDrag, shapePath, splineFromPoints } from "./geometry";

const style: ShapeStyle = {
  strokeColor: "#16212b",
  strokeWidthPt: 1.6,
  fillColor: null,
  opacity: 1,
};

describe("shape geometry", () => {
  it("constructs a constrained square from either drag direction", () => {
    const draft = shapeFromDrag("rectangle", { x: 80, y: 60 }, { x: 20, y: 30 }, style, true);
    expect(draft).toMatchObject({ x: 20, y: 0, geometry: { widthPt: 60, heightPt: 60 } });
  });

  it("emits one shared path representation for parametric and spline shapes", () => {
    expect(shapePath({ kind: "ellipse", widthPt: 40, heightPt: 20 })).toContain("A 20 10");
    const spline = splineFromPoints(
      [
        { x: 10, y: 20 },
        { x: 20, y: 10 },
        { x: 30, y: 20 },
      ],
      style,
      0.25,
    );
    expect(spline?.geometry.kind).toBe("spline");
    expect(spline && shapePath(spline.geometry)).toContain(" C ");
  });

  it("does not create a shape from a tap", () => {
    expect(shapeFromDrag("line", { x: 0, y: 0 }, { x: 1, y: 1 }, style)).toBeNull();
    expect(splineFromPoints([{ x: 0, y: 0 }], style)).toBeNull();
  });
});

describe("fitting a long freehand drag", () => {
  // A spiral is the worst case for the curve fit: every sample is far enough from the chord
  // through its neighbours to be kept, which is what used to drive the recursion as deep as the
  // stroke was long. The tool has to survive one, and stay inside the store's node ceiling.
  const spiral = Array.from({ length: 20_000 }, (_, index) => {
    const angle = index * 0.02;
    const radius = 4 + index * 0.01;
    return { x: 400 + Math.cos(angle) * radius, y: 400 + Math.sin(angle) * radius };
  });

  it("fits twenty thousand samples without overflowing the stack", () => {
    const draft = splineFromPoints(spiral, style, 0.5);
    expect(draft?.geometry.kind).toBe("spline");
    const nodes = draft?.geometry.kind === "spline" ? draft.geometry.nodes : [];
    expect(nodes.length).toBeGreaterThan(2);
    expect(nodes.length).toBeLessThanOrEqual(256);
    expect(nodes.every((node) => Number.isFinite(node.point.x) && Number.isFinite(node.point.y)))
      .toBe(true);
  });

  it("keeps the ends and drops what the tolerance makes redundant", () => {
    const straight = Array.from({ length: 500 }, (_, index) => ({ x: index, y: 0 }));
    const draft = splineFromPoints(straight, style, 1);
    const nodes = draft?.geometry.kind === "spline" ? draft.geometry.nodes : [];
    expect(nodes).toHaveLength(2);
    expect(nodes[0].point).toEqual({ x: 0, y: 0 });
    expect(nodes[1].point).toEqual({ x: 499, y: 0 });
  });
});

describe("shape line patterns", () => {
  const style = { strokeColor: "#000000", strokeWidthPt: 2, fillColor: null, opacity: 1 };
  it("stays solid unless a pattern is set, including for older shapes", () => {
    expect(shapeDashArray(style)).toBeNull();
    expect(shapeDashArray({ ...style, pattern: "solid" })).toBeNull();
  });
  it("spaces dashes and dots like patterned ink of the same width", () => {
    // Ink: 8pt dashes and 5pt gaps at 2pt; round caps add one width to each visible dash.
    expect(shapeDashArray({ ...style, pattern: "dashed" })).toEqual([6, 7]);
    expect(shapeDashArray({ ...style, pattern: "dotted" })).toEqual([0.01, 5.5]);
    // Spacing stretches only the gap, matching the ink.
    expect(shapeDashArray({ ...style, pattern: "dashed", patternSpacing: 2 })).toEqual([6, 12]);
    expect(shapeDashArray({ ...style, pattern: "dotted", patternSpacing: 2 })).toEqual([0.01, 11]);
  });
});

describe("spline roundness", () => {
  // Curvature along the fitted curve, as a multiple of the true circle's 1/r.
  function roundness(points: { x: number; y: number }[], radius: number): { min: number; max: number } {
    const draft = splineFromPoints(points, style, 1.5)!;
    const nodes = draft.geometry.kind === "spline" ? draft.geometry.nodes : [];
    const dense: { x: number; y: number }[] = [];
    for (let index = 1; index < nodes.length; index += 1) {
      const a = nodes[index - 1];
      const b = nodes[index];
      const p1 = { x: a.point.x + (a.handleOut?.x ?? 0), y: a.point.y + (a.handleOut?.y ?? 0) };
      const p2 = { x: b.point.x + (b.handleIn?.x ?? 0), y: b.point.y + (b.handleIn?.y ?? 0) };
      for (let t = 0; t < 1; t += 0.02) {
        const u = 1 - t;
        dense.push({
          x: u * u * u * a.point.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * b.point.x,
          y: u * u * u * a.point.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * b.point.y,
        });
      }
    }
    const bends: number[] = [];
    for (let index = 1; index < dense.length - 1; index += 1) {
      const [p, q, r] = [dense[index - 1], dense[index], dense[index + 1]];
      const pq = Math.hypot(q.x - p.x, q.y - p.y);
      const qr = Math.hypot(r.x - q.x, r.y - q.y);
      const rp = Math.hypot(p.x - r.x, p.y - r.y);
      const cross = Math.abs((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x));
      if (pq > 0.05 && qr > 0.05) bends.push(((2 * cross) / (pq * qr * rp)) * radius);
    }
    return { min: Math.min(...bends), max: Math.max(...bends) };
  }

  it("keeps a drawn half circle evenly round, ends included", () => {
    for (const radius of [30, 100, 250]) {
      const arc = [];
      for (let angle = 0; angle <= Math.PI; angle += 0.6 / radius) {
        arc.push({ x: radius * Math.cos(angle), y: -radius * Math.sin(angle) });
      }
      // The uniform handle rule this replaced swung between 0.03 and 3.2 times the true bend.
      const { min, max } = roundness(arc, radius);
      expect(min).toBeGreaterThan(0.8);
      expect(max).toBeLessThan(1.2);
    }
  });
});
