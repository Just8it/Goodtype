import { describe, expect, it } from "vitest";
import { focusTarget, moveByOne, moveIntoGap, sameOrder, selectRange, toggleSelection } from "./pageOverview";

const order = ["a", "b", "c", "d", "e"];

describe("page overview selection", () => {
  it("selects a range in either direction", () => {
    expect(selectRange(order, "b", "d")).toEqual(["b", "c", "d"]);
    expect(selectRange(order, "d", "b")).toEqual(["b", "c", "d"]);
    expect(selectRange(order, "gone", "c")).toEqual(["c"]);
  });

  it("toggles one page in and out", () => {
    expect(toggleSelection(["a"], "c")).toEqual(["a", "c"]);
    expect(toggleSelection(["a", "c"], "a")).toEqual(["c"]);
  });
});

describe("page overview reordering", () => {
  it("drops one page into a gap", () => {
    expect(moveIntoGap(order, ["a"], 3)).toEqual(["b", "c", "a", "d", "e"]);
    expect(moveIntoGap(order, ["e"], 0)).toEqual(["e", "a", "b", "c", "d"]);
    expect(moveIntoGap(order, ["c"], 3)).toEqual(order);
  });

  it("moves a scattered selection together, in notebook order", () => {
    expect(moveIntoGap(order, ["d", "b"], 5)).toEqual(["a", "c", "e", "b", "d"]);
    expect(moveIntoGap(order, ["b", "d"], 0)).toEqual(["b", "d", "a", "c", "e"]);
  });

  it("moves by one place with the keyboard and stops at the ends", () => {
    expect(moveByOne(order, ["c"], -1)).toEqual(["a", "c", "b", "d", "e"]);
    expect(moveByOne(order, ["c"], 1)).toEqual(["a", "b", "d", "c", "e"]);
    expect(moveByOne(order, ["b", "c"], 1)).toEqual(["a", "d", "b", "c", "e"]);
    expect(moveByOne(order, ["a"], -1)).toEqual(order);
    expect(moveByOne(order, ["e"], 1)).toEqual(order);
  });

  it("compares orders", () => {
    expect(sameOrder(order, [...order])).toBe(true);
    expect(sameOrder(order, moveByOne(order, ["c"], 1))).toBe(false);
  });
});

describe("page overview focus", () => {
  it("walks a two-column grid and clamps at its ends", () => {
    expect(focusTarget("ArrowRight", 0, 5, 2)).toBe(1);
    expect(focusTarget("ArrowDown", 1, 5, 2)).toBe(3);
    expect(focusTarget("ArrowDown", 3, 5, 2)).toBe(4);
    expect(focusTarget("ArrowUp", 1, 5, 2)).toBe(0);
    expect(focusTarget("End", 0, 5, 2)).toBe(4);
    expect(focusTarget("Enter", 0, 5, 2)).toBeNull();
  });
});
