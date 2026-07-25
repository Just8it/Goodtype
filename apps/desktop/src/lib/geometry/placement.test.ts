import { describe, expect, it } from "vitest";
import { MIN_VISIBLE_PT, keepOnPage } from "./placement";

const page = { widthPt: 595, heightPt: 842 };
const block = { widthPt: 240, heightPt: 100 };

describe("keepOnPage", () => {
  it("leaves an object alone while it is on the page", () => {
    expect(keepOnPage({ x: 72, y: 96 }, block, page)).toEqual({ x: 72, y: 96 });
  });

  it("allows overhang but never a full escape", () => {
    // Dragged far past the left and top edges: a grabbable strip stays on the sheet.
    expect(keepOnPage({ x: -9999, y: -9999 }, block, page)).toEqual({
      x: MIN_VISIBLE_PT - block.widthPt,
      y: MIN_VISIBLE_PT - block.heightPt,
    });
    // And past the right and bottom.
    expect(keepOnPage({ x: 9999, y: 9999 }, block, page)).toEqual({
      x: page.widthPt - MIN_VISIBLE_PT,
      y: page.heightPt - MIN_VISIBLE_PT,
    });
  });

  it("holds an object smaller than the minimum entirely on the page", () => {
    // The naive bound would let a 10pt object hang off by its own size and vanish.
    const tiny = { widthPt: 10, heightPt: 10 };
    expect(keepOnPage({ x: -50, y: -50 }, tiny, page)).toEqual({ x: 0, y: 0 });
    expect(keepOnPage({ x: 9999, y: 9999 }, tiny, page)).toEqual({
      x: page.widthPt - 10,
      y: page.heightPt - 10,
    });
  });

  it("lets an object wider than the page sit anywhere that still shows a grip", () => {
    const wide = { widthPt: 900, heightPt: 60 };
    // Spans 300..1200 with 295pt of it over the sheet — legal, and left alone.
    expect(keepOnPage({ x: 300, y: 10 }, wide, page).x).toBe(300);
    expect(keepOnPage({ x: -9999, y: 10 }, wide, page).x).toBe(MIN_VISIBLE_PT - 900);
    expect(keepOnPage({ x: 9999, y: 10 }, wide, page).x).toBe(page.widthPt - MIN_VISIBLE_PT);
  });

  it("pins to the origin when the page is smaller than the grip", () => {
    // Bounds cross: a 20pt object on a 10pt page can never show 20pt of itself over the sheet,
    // so there is no satisfying answer and the object is held at the corner instead.
    const small = { widthPt: 20, heightPt: 20 };
    const tinyPage = { widthPt: 10, heightPt: 10 };
    expect(keepOnPage({ x: 500, y: -500 }, small, tinyPage)).toEqual({ x: 0, y: 0 });
  });

  it("treats a broken coordinate as the origin rather than propagating it", () => {
    expect(keepOnPage({ x: Number.NaN, y: 40 }, block, page)).toEqual({ x: 0, y: 40 });
  });
});
