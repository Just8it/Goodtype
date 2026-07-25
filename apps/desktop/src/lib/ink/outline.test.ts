import { describe, expect, it } from "vitest";
import cases from "../../../../../fixtures/ink/outline.json";
import { outlinePath, outlinePoints, type OutlinePoint } from "./outline";

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
