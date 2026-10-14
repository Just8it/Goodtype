import { describe, expect, it } from "vitest";
import {
  addWidth,
  canRemoveWidth,
  editWidth,
  movePattern,
  moveSpacing,
  pruneSpacings,
  spacingForWidth,
  withWidthSpacing,
  normaliseWidths,
  patternForWidth,
  prunePatterns,
  withWidthPattern,
  removeWidth,
  WIDTH_BOUNDS_MM,
} from "./widths";

const MAX = 4;

describe("line patterns on width chips", () => {
  it("changes only the chip it was set on", () => {
    const patterns = withWidthPattern({}, 1.6, "dotted");
    expect(patternForWidth(patterns, 1.6)).toBe("dotted");
    expect(patternForWidth(patterns, 2.8)).toBe("solid");
    expect(withWidthPattern(patterns, 1.6, "solid")).toEqual({});
  });

  it("follows a chip whose width is edited and leaves with a removed one", () => {
    const patterns = withWidthPattern({}, 1.6, "dashed");
    const moved = movePattern(patterns, 1.6, 2);
    expect(patternForWidth(moved, 2)).toBe("dashed");
    expect(patternForWidth(moved, 1.6)).toBe("solid");
    expect(prunePatterns(moved, [1.6, 2.8])).toEqual({});
  });
});

describe("pattern spacing on width chips", () => {
  it("keeps a spacing per chip and stores nothing at the default", () => {
    const spacings = withWidthSpacing({}, 1.6, 2);
    expect(spacingForWidth(spacings, 1.6)).toBe(2);
    expect(spacingForWidth(spacings, 2.8)).toBe(1);
    expect(withWidthSpacing(spacings, 1.6, 1)).toEqual({});
    expect(spacingForWidth(moveSpacing(spacings, 1.6, 2), 2)).toBe(2);
    expect(pruneSpacings(spacings, [2.8])).toEqual({});
  });
});

describe("the stroke width row", () => {
  it("stays inside the persisted limits", () => {
    expect(WIDTH_BOUNDS_MM.pen.minimum * 2.835).toBeGreaterThanOrEqual(0.2);
    expect(WIDTH_BOUNDS_MM.pen.maximum * 2.835).toBeLessThanOrEqual(12);
    expect(WIDTH_BOUNDS_MM.highlighter.minimum * 2.835).toBeGreaterThanOrEqual(1);
    expect(WIDTH_BOUNDS_MM.highlighter.maximum * 2.835).toBeLessThanOrEqual(20);
  });

  it("keeps one width but removes a chosen width from a larger row", () => {
    expect(canRemoveWidth([1.6])).toBe(false);
    expect(removeWidth([1.6], 0)).toEqual([1.6]);
    expect(removeWidth([], 0)).toEqual([]);
    expect(canRemoveWidth([1.6, 2.8])).toBe(true);
    expect(removeWidth([1, 1.6, 2.8], 1)).toEqual([1, 2.8]);
  });

  it("normalizes additions and edits into one bounded width ladder", () => {
    expect(normaliseWidths([2.8, 1, 1.6], MAX)).toEqual([1, 1.6, 2.8]);
    expect(normaliseWidths([1.6, 1.6, 2.8], MAX)).toEqual([1.6, 2.8]);
    expect(normaliseWidths([1.60001, 1.6], MAX)).toEqual([1.6]);
    expect(editWidth([1, 1.6, 2.8], 0, 1.6, MAX)).toEqual([1.6, 2.8]);
    const full = [0.5, 1, 1.6, 2.8];
    expect(addWidth(full, 4, MAX)).toEqual(full);
    expect(addWidth(full, 1.6, MAX)).toEqual(full);
    expect(addWidth([1.6, 2.8], 0.8, MAX)).toEqual([0.8, 1.6, 2.8]);
  });
});
