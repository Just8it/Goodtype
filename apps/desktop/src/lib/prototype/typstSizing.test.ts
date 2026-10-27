import { describe, expect, it } from "vitest";
import { rotatedVector } from "../geometry/coordinates";
import { scaleTypstCorner, typstScale, typstWidthFromMm, POINTS_PER_MM } from "./typstSizing";

describe("Typst dimensions", () => {
  const start = { x: 10, y: 20, layoutWidthPt: 200, scale: 1.5, rotation: 0 };
  it("scales uniformly from a diagonal drag, including rotated blocks", () => {
    expect(scaleTypstCorner(start, 100, { x: 100, y: 50 })).toBe(2);
    expect(scaleTypstCorner({ ...start, rotation: 70 }, 100, rotatedVector({ x: 100, y: 50 }, 70))).toBeCloseTo(2);
    expect(scaleTypstCorner(start, 100, { x: -50, y: 100 })).toBe(1.5);
    expect(start.layoutWidthPt).toBe(200);
  });
  it("keeps sizes positive and bounded when dragging past the anchor", () => {
    expect(scaleTypstCorner(start, 100, { x: -1000, y: -1000 })).toBe(0.1);
    expect(typstScale(100)).toBe(8);
  });
  it("converts the visible millimetre width without changing scale", () => {
    expect(typstWidthFromMm(100, 2) * 2 / POINTS_PER_MM).toBeCloseTo(100);
    expect(typstWidthFromMm(1, 2)).toBe(72);
  });
});
