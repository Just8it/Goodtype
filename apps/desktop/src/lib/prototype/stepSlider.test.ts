import { describe, expect, it } from "vitest";
import { fromPosition, nudge, toPosition, TRACK_RESOLUTION } from "./stepSlider";

const width = { min: 0.05, max: 20, step: 0.05, log: true };
const spacing = { min: 0.5, max: 3, step: 0.05 };

describe("step slider", () => {
  it("gives everyday nib widths most of a log track", () => {
    // 0.05–1 mm covers half the track; a plain slider gave it under 5 %.
    expect(toPosition(1, width)).toBe(500);
    expect(toPosition(0.05, width)).toBe(0);
    expect(toPosition(20, width)).toBe(TRACK_RESOLUTION);
  });

  it("round-trips track positions and keeps fine values at the thin end", () => {
    expect(fromPosition(toPosition(0.35, width), width)).toBe(0.35);
    // Thick widths get less track per millimetre; the buttons reach the exact value.
    expect(fromPosition(toPosition(7.5, width), width)).toBeCloseTo(7.5, 1);
    expect(fromPosition(130, width)).toBe(0.11);
    expect(fromPosition(toPosition(1.25, spacing), spacing)).toBe(1.25);
  });

  it("nudges one step onto the grid and stops at the ends", () => {
    expect(nudge(0.35, 1, width)).toBe(0.4);
    expect(nudge(0.35, -1, width)).toBe(0.3);
    expect(nudge(0.37, 1, width)).toBe(0.4);
    expect(nudge(0.37, -1, width)).toBe(0.35);
    expect(nudge(0.05, -1, width)).toBe(0.05);
    expect(nudge(3, 1, spacing)).toBe(3);
    expect(nudge(1, 1, { min: 4, max: 96, step: 1 })).toBe(4);
  });
});
