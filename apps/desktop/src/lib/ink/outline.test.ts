import { describe, expect, it } from "vitest";
import cases from "../../../../../fixtures/ink/outline.json";
import { limitedTaper, outlinePath, outlinePoints, roundedOutlinePoints, type OutlinePoint } from "./outline";
import roundedCases from "../../../../../fixtures/ink/rounded-outline.json";

type Case = {
  name: string;
  points: [number, number, number][];
  width_pt: number;
  pressure: boolean;
  taper: number;
  expected: [number, number][];
};

// The same fixture `crates/goodtype-core/src/outline.rs` asserts against. Live drawing happens
// here and export happens in Rust; without this the two silently drift and the PDF stops
// matching the screen — which is exactly how pressure got lost before.
describe("outline geometry matches the shared fixture", () => {
  for (const testCase of cases as Case[]) {
    it(testCase.name, () => {
      const points: OutlinePoint[] = testCase.points.map(([x, y, pressure]) => ({
        x,
        y,
        pressure,
      }));
      const produced = outlinePoints(points, {
        widthPt: testCase.width_pt,
        pressure: testCase.pressure,
        taper: testCase.taper,
      });
      expect(produced.map((vertex) => [vertex.x, vertex.y])).toEqual(testCase.expected);
    });
  }
});

describe("outlinePath", () => {
  it("matches the Rust rounded-pen fixtures", () => {
    for (const testCase of roundedCases) {
      const points = testCase.points.map(([x, y, pressure]) => ({ x, y, pressure }));
      const produced = roundedOutlinePoints(points, { widthPt: testCase.width_pt, pressure: testCase.pressure, taper: testCase.taper });
      expect(produced.map((polygon) => polygon.map((p) => [p.x, p.y]))).toEqual(testCase.expected);
    }
  });

  it("keeps sharp bends and reversals additive instead of cutting holes", () => {
    for (const path of [[[0, 0], [10, 0], [10, 10]], [[0, 0], [10, 0], [0, 0]], [[0, 0], [10, 0], [10, -10]]]) {
      const polygons = roundedOutlinePoints(path.map(([x, y]) => ({ x, y, pressure: 1 })), { widthPt: 2, pressure: false, taper: 0 });
      for (const polygon of polygons) {
        const area = polygon.reduce((sum, p, i) => { const q = polygon[(i + 1) % polygon.length]; return sum + p.x * q.y - q.x * p.y; }, 0);
        expect(area).toBeLessThan(0);
      }
    }
  });

  it("resolves a half-length taper cap into the saved fraction without changing the source points", () => {
    const points = [{ x: 0, y: 0, pressure: 1 }, { x: 1000, y: 0, pressure: 1 }];
    expect(limitedTaper(points, 0.12, 2 * 3)).toBe(0.006);
    expect(limitedTaper(points, 0.12, 2 * 6)).toBe(0.012);
    expect(limitedTaper(points, 0.12, 0)).toBe(0);
    expect(points[1].x).toBe(1000);
    expect(limitedTaper([{ x: 0, y: 0, pressure: 1 }, { x: 5, y: 0, pressure: 1 }], 0.12, 6)).toBe(0.12);
  });
  it("keeps light, stationary and jittered taps round at the selected width", () => {
    const point = { x: 10, y: 10, pressure: 0.01 };
    for (const points of [[point], [point, { x: 10.08, y: 10, pressure: 0 }],
      [point, { x: 10.02, y: 10.01, pressure: 0.8 }, { ...point, pressure: 0 }]]) {
      const polygon = outlinePoints(points, { widthPt: 2, pressure: true, taper: 0.12 });
      expect(polygon).toHaveLength(12);
      expect(Math.max(...polygon.map((p) => p.x)) - Math.min(...polygon.map((p) => p.x))).toBe(2);
      expect(Math.max(...polygon.map((p) => p.y)) - Math.min(...polygon.map((p) => p.y))).toBe(2);
      expect(polygon.every((p) => Math.abs(Math.hypot(p.x - 10, p.y - 10) - 1) < 0.001)).toBe(true);
    }
  });

  it("gives a two-sample tapered stroke a visible middle rather than zero area", () => {
    const polygon = outlinePoints([{ x: 0, y: 0, pressure: 1 }, { x: 10, y: 0, pressure: 0 }],
      { widthPt: 2, pressure: true, taper: 0.12 });
    expect(polygon.some((p) => p.y > 0)).toBe(true);
    expect(polygon.some((p) => p.y < 0)).toBe(true);
  });
  it("closes the polygon", () => {
    const path = outlinePath([
      { x: 0, y: 0 },
      { x: 1, y: 0 },
      { x: 1, y: 1 },
    ]);
    expect(path).toBe("M 0 0 L 1 0 L 1 1 Z");
  });

  it("is empty when there is nothing to draw", () => {
    expect(outlinePath([])).toBe("");
    expect(outlinePoints([], { widthPt: 2, pressure: false, taper: 0 })).toEqual([]);
  });
});
