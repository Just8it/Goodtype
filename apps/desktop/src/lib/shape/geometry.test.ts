import { describe, expect, it } from "vitest";
import type { ShapeStyle } from "../model";
import { shapeFromDrag, shapePath, splineFromPoints } from "./geometry";

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
