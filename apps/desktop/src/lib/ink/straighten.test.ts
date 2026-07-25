import { describe, expect, it } from "vitest";
import type { StrokePoint } from "../model";
import { DEFAULT_STRAIGHTEN, chordDeviation, straightenStroke } from "./straighten";

function stroke(coordinates: [number, number][]): StrokePoint[] {
  return coordinates.map(([x, y], index) => ({
    x,
    y,
    pressure: 0.5,
    timeMs: index * 8,
    tiltX: 0,
    tiltY: 0,
  }));
}

describe("chordDeviation", () => {
  it("measures the worst wander off the line between the endpoints", () => {
    const { deviationPt, chordPt } = chordDeviation(
      stroke([
        [0, 0],
        [50, 10],
        [100, 0],
      ]),
    );
    expect(chordPt).toBe(100);
    expect(deviationPt).toBe(10);
  });

  it("reports nothing for a stroke that never left its starting point", () => {
    expect(chordDeviation(stroke([[5, 5]]))).toEqual({ deviationPt: 0, chordPt: 0 });
  });
});

describe("straightenStroke", () => {
  it("collapses an almost-straight sweep to two points", () => {
    const result = straightenStroke(
      stroke([
        [0, 0],
        [50, 1.5],
        [100, 2],
        [150, 1],
        [200, 0],
      ]),
    );
    expect(result).toHaveLength(2);
    expect(result[0]).toMatchObject({ x: 0, y: 0 });
    expect(result[1].x).toBeCloseTo(200);
    expect(result[1].y).toBeCloseTo(0);
  });

  it("snaps a nearly horizontal sweep flat", () => {
    const drawn = stroke([
      [0, 0],
      [60, 2],
      [120, 4],
    ]);
    const result = straightenStroke(drawn);
    // Drawn at ~1.9°, which is inside the 15° step, so it lands exactly on the horizontal.
    expect(result[1].y).toBeCloseTo(0);
    expect(result[1].x).toBeCloseTo(Math.hypot(120, 4));
  });

  it("keeps the drawn angle when snapping is switched off", () => {
    const result = straightenStroke(
      stroke([
        [0, 0],
        [60, 2],
        [120, 4],
      ]),
      { ...DEFAULT_STRAIGHTEN, snapDegrees: 0 },
    );
    expect(result[1].x).toBeCloseTo(120);
    expect(result[1].y).toBeCloseTo(4);
  });

  it("leaves a deliberate curve alone", () => {
    const curve = stroke([
      [0, 0],
      [50, 40],
      [100, 55],
      [150, 40],
      [200, 0],
    ]);
    expect(straightenStroke(curve)).toBe(curve);
  });

  // Handwriting is short and rarely straight; a flick between two letters must not become a rule.
  it("leaves a short stroke alone", () => {
    const flick = stroke([
      [0, 0],
      [5, 0],
      [10, 0],
    ]);
    expect(straightenStroke(flick)).toBe(flick);
  });

  it("leaves a stroke that doubles back alone", () => {
    // Hugs its own chord the whole way, so only the travelled distance tells it apart from a line.
    const scrubbed = stroke([
      [0, 0],
      [100, 0],
      [20, 0],
      [100, 0],
    ]);
    expect(straightenStroke(scrubbed)).toBe(scrubbed);
  });

  it("carries the endpoint samples through, so pressure still varies", () => {
    const drawn = stroke([
      [0, 0],
      [100, 1],
      [200, 0],
    ]);
    drawn[0].pressure = 0.2;
    drawn[2].pressure = 0.9;
    const result = straightenStroke(drawn);
    expect([result[0].pressure, result[1].pressure]).toEqual([0.2, 0.9]);
  });
});
