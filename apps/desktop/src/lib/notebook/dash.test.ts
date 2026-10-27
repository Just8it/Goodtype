import { describe, expect, it } from "vitest";
import { centredDashArray } from "./dash";

describe("centred preview dashes", () => {
  it("fits whole marks and shifts them to leave equal room at both ends", () => {
    // 18 long, 5 dashes with 4 gaps: two marks use 14, so 2 is left on each side.
    expect(centredDashArray(18, 5, 4)).toEqual({ dasharray: "5 4 5 18", dashoffset: -2 });
    // Dots: whatever whole number fits, with the remainder split between both ends.
    const dots = centredDashArray(30, 0.01, 5)!;
    const used = dots.dasharray.split(" ").map(Number).slice(0, -1).reduce((a, b) => a + b, 0);
    expect(-dots.dashoffset).toBeCloseTo((30 - used) / 2);
  });

  it("never starts with a zero-length dash, which round caps would draw as a dot", () => {
    for (const [length, dash, gap] of [[18, 5, 4], [30, 0.01, 5], [4, 5, 4]]) {
      expect(centredDashArray(length, dash, gap)!.dasharray.startsWith("0 ")).toBe(false);
    }
  });
});
