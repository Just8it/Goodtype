import { describe, expect, it } from "vitest";
import { MIN_VISIBLE_PT, keepOnPage, placeFloatingToolbar, type ViewRect } from "./placement";

const page = { widthPt: 595, heightPt: 842 };
const block = { widthPt: 240, heightPt: 100 };

describe("keepOnPage", () => {
  it("keeps every object reachable without moving valid positions", () => {
    expect(keepOnPage({ x: 72, y: 96 }, block, page)).toEqual({ x: 72, y: 96 });
    expect(keepOnPage({ x: -9999, y: -9999 }, block, page)).toEqual({
      x: MIN_VISIBLE_PT - block.widthPt,
      y: MIN_VISIBLE_PT - block.heightPt,
    });
    expect(keepOnPage({ x: 9999, y: 9999 }, block, page)).toEqual({
      x: page.widthPt - MIN_VISIBLE_PT,
      y: page.heightPt - MIN_VISIBLE_PT,
    });
    const tiny = { widthPt: 10, heightPt: 10 };
    expect(keepOnPage({ x: -50, y: -50 }, tiny, page)).toEqual({ x: 0, y: 0 });
    expect(keepOnPage({ x: 9999, y: 9999 }, tiny, page)).toEqual({
      x: page.widthPt - 10,
      y: page.heightPt - 10,
    });
    const wide = { widthPt: 900, heightPt: 60 };
    expect(keepOnPage({ x: 300, y: 10 }, wide, page).x).toBe(300);
    expect(keepOnPage({ x: -9999, y: 10 }, wide, page).x).toBe(MIN_VISIBLE_PT - 900);
    expect(keepOnPage({ x: 9999, y: 10 }, wide, page).x).toBe(page.widthPt - MIN_VISIBLE_PT);
    const small = { widthPt: 20, heightPt: 20 };
    const tinyPage = { widthPt: 10, heightPt: 10 };
    expect(keepOnPage({ x: 500, y: -500 }, small, tinyPage)).toEqual({ x: 0, y: 0 });
    expect(keepOnPage({ x: Number.NaN, y: 40 }, block, page)).toEqual({ x: 0, y: 40 });
  });
});

describe("placeFloatingToolbar", () => {
  const boundary: ViewRect = {
    left: 100,
    top: 50,
    right: 900,
    bottom: 650,
    width: 800,
    height: 600,
  };
  const toolbar = { width: 132, height: 44 };

  it("prefers above, falls below near the top, and stays inside the sides", () => {
    expect(
      placeFloatingToolbar(
        { left: 400, top: 300, right: 600, bottom: 400, width: 200, height: 100 },
        toolbar,
        boundary,
      ),
    ).toEqual({ left: 334, top: 196, side: "above" });

    expect(
      placeFloatingToolbar(
        { left: 105, top: 60, right: 205, bottom: 140, width: 100, height: 80 },
        toolbar,
        boundary,
      ),
    ).toEqual({ left: 12, top: 100, side: "below" });
  });
});
