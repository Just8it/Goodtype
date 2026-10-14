import { describe, expect, it } from "vitest";
import { patternRuns, patternedOutlinePoints } from "./pattern";

const line = [
  { x: 0, y: 0, pressure: 1 },
  { x: 20, y: 0, pressure: 1 },
];

function signedArea(points: { x: number; y: number }[]): number {
  return points.reduce((area, point, index) => {
    const next = points[(index + 1) % points.length];
    return area + point.x * next.y - next.x * point.y;
  }, 0);
}

describe("ink patterns", () => {
  it("measures dashes along the stroke in page points", () => {
    const runs = patternRuns(line, 2, "dashed");
    expect(runs.map((run) => [run[0].x, run.at(-1)?.x])).toEqual([
      [0, 8],
      [13, 20],
    ]);
  });

  it("anchors evenly spaced dots at the stroke origin", () => {
    const runs = patternRuns(line, 2, "dotted");
    expect(runs.map(([point]) => point.x)).toEqual([0, 5.5, 11, 16.5]);
  });

  it("does not move existing marks as the stroke grows", () => {
    const longerLine = [...line, { x: 40, y: 0, pressure: 1 }];
    const dotsBefore = patternRuns(line, 2, "dotted");
    const dotsAfter = patternRuns(longerLine, 2, "dotted");
    expect(dotsAfter.slice(0, dotsBefore.length)).toEqual(dotsBefore);

    const dashPrefix = [
      { x: 0, y: 0, pressure: 1 },
      { x: 20, y: 0, pressure: 1 },
      { x: 24, y: 0, pressure: 1 },
    ];
    const completedDashes = patternRuns(dashPrefix, 2, "dashed");
    const extendedDashes = patternRuns(
      [...dashPrefix, { x: 40, y: 0, pressure: 1 }],
      2,
      "dashed",
    );
    expect(extendedDashes.slice(0, completedDashes.length)).toEqual(completedDashes);
  });

  it("renders dots as round filled marks instead of square taps", () => {
    const polygons = patternedOutlinePoints(line, { widthPt: 2, pressure: false, taper: 0 }, "dotted");
    expect(polygons).toHaveLength(4);
    expect(polygons.every((polygon) => polygon.length === 12)).toBe(true);
  });

  it("keeps dash bodies and caps additive in a compound SVG path", () => {
    const polygons = patternedOutlinePoints(line, { widthPt: 2, pressure: false, taper: 0 }, "dashed");
    expect(polygons).toHaveLength(6);
    expect(polygons.every((polygon) => signedArea(polygon) < 0)).toBe(true);

    const tap = patternedOutlinePoints(
      [{ x: 4, y: 8, pressure: 1 }],
      { widthPt: 2, pressure: false, taper: 0 },
      "dashed",
    );
    expect(tap).toHaveLength(1);
  });

  it("widens or narrows only the gaps when spacing changes", () => {
    const line = [
      { x: 0, y: 0, pressure: 1 },
      { x: 100, y: 0, pressure: 1 },
    ];
    const dots = (spacing: number) => patternRuns(line, 2, "dotted", spacing).map(([dot]) => dot.x);
    expect(dots(1).slice(0, 3)).toEqual([0, 5.5, 11]);
    expect(dots(2).slice(0, 3)).toEqual([0, 11, 22]);
    const dashes = patternRuns(line, 2, "dashed", 2);
    // 8pt dashes, gaps doubled from 5pt to 10pt.
    expect(dashes[0].at(-1)?.x).toBe(8);
    expect(dashes[1][0].x).toBe(18);
  });
});
