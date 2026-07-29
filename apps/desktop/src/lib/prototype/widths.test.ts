import { describe, expect, it } from "vitest";
import {
  addWidth,
  canRemoveWidth,
  editWidth,
  normaliseWidths,
  removeWidth,
  WIDTH_BOUNDS_MM,
} from "./widths";

const MAX = 4;

describe("the stroke width row", () => {
  it("only offers sizes the Rust settings store accepts", () => {
    expect(WIDTH_BOUNDS_MM.pen.minimum * 2.835).toBeGreaterThanOrEqual(0.2);
    expect(WIDTH_BOUNDS_MM.pen.maximum * 2.835).toBeLessThanOrEqual(12);
    expect(WIDTH_BOUNDS_MM.highlighter.minimum * 2.835).toBeGreaterThanOrEqual(1);
    expect(WIDTH_BOUNDS_MM.highlighter.maximum * 2.835).toBeLessThanOrEqual(20);
  });

  it("never gives up its last width", () => {
    expect(canRemoveWidth([1.6])).toBe(false);
    expect(removeWidth([1.6], 0)).toEqual([1.6]);
    // A tool with no width has nothing to draw with, and an empty row cannot be refilled from
    // itself — the add tile is gone by the time the row is full, and useless when it is empty.
    expect(removeWidth([], 0)).toEqual([]);
  });

  it("gives one up while there is more than one", () => {
    expect(canRemoveWidth([1.6, 2.8])).toBe(true);
    expect(removeWidth([1, 1.6, 2.8], 1)).toEqual([1, 2.8]);
  });

  it("stays a ladder: sorted and without twins", () => {
    expect(normaliseWidths([2.8, 1, 1.6], MAX)).toEqual([1, 1.6, 2.8]);
    expect(normaliseWidths([1.6, 1.6, 2.8], MAX)).toEqual([1.6, 2.8]);
  });

  // Editing one tile onto another's value is a way of asking for fewer, not for a twin.
  it("collapses an edit that lands on a width already there", () => {
    expect(editWidth([1, 1.6, 2.8], 0, 1.6, MAX)).toEqual([1.6, 2.8]);
  });

  it("refuses to grow past the row, but takes a width already in it", () => {
    const full = [0.5, 1, 1.6, 2.8];
    expect(addWidth(full, 4, MAX)).toEqual(full);
    // Not a growth, so not refused — this is the no-op that keeps re-selecting simple.
    expect(addWidth(full, 1.6, MAX)).toEqual(full);
    expect(addWidth([1.6, 2.8], 0.8, MAX)).toEqual([0.8, 1.6, 2.8]);
  });

  it("rounds to the precision a nib is specified at, so near-twins do not both survive", () => {
    expect(normaliseWidths([1.60001, 1.6], MAX)).toEqual([1.6]);
  });
});
