import { describe, expect, it } from "vitest";
import {
  atScrollEnd,
  PULL_FULL_PX,
  PULL_FULL_WHEEL_PX,
  PULL_MAX_SHIFT_PX,
  pullProgress,
  rubberBand,
  wheelPixels,
} from "./pullToAdd";

describe("pull past the last page", () => {
  it("resists more the further it is pulled and never passes its cap", () => {
    expect(rubberBand(0)).toBe(0);
    expect(rubberBand(-20)).toBe(0);
    const first = rubberBand(50);
    const second = rubberBand(100) - rubberBand(50);
    expect(second).toBeLessThan(first);
    expect(rubberBand(10_000)).toBeLessThanOrEqual(PULL_MAX_SHIFT_PX);
  });

  it("fills the ring by raw distance, and asks the wheel for a longer pull", () => {
    expect(pullProgress(PULL_FULL_PX / 2, "touch")).toBeCloseTo(0.5);
    expect(pullProgress(PULL_FULL_PX * 3, "touch")).toBe(1);
    // One ordinary wheel notch past the end is nowhere near enough.
    expect(pullProgress(100, "wheel")).toBeLessThan(0.3);
    expect(pullProgress(PULL_FULL_WHEEL_PX, "wheel")).toBe(1);
  });

  it("knows the end of the scroll despite rounding, and normalises wheel units", () => {
    expect(atScrollEnd(899.5, 100, 1000)).toBe(true);
    expect(atScrollEnd(850, 100, 1000)).toBe(false);
    expect(wheelPixels(3, 1, 800)).toBe(48);
    expect(wheelPixels(1, 2, 800)).toBe(800);
    expect(wheelPixels(120, 0, 800)).toBe(120);
  });
});
