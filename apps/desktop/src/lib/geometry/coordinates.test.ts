import { describe, expect, it } from "vitest";
import {
  clampToPage,
  clampZoom,
  pageToScreen,
  screenToPage,
  type PageViewport,
  type ScreenViewport,
} from "./coordinates";

describe("coordinate authority", () => {
  it("round-trips screen and page points at different zoom and origins", () => {
    const viewport: ScreenViewport = { left: 100, top: 60 };
    const view: PageViewport = { pageOriginX: 24, pageOriginY: 12, zoom: 1.5 };
    const page = { x: 72, y: 144 };

    const screen = pageToScreen(page, viewport, view);

    expect(screen).toEqual({ x: 232, y: 288 });
    expect(screenToPage(screen, viewport, view)).toEqual(page);
    expect(clampToPage({ x: -2, y: 900 }, { width: 612, height: 792 })).toEqual({
      x: 0,
      y: 792,
    });
    expect(clampZoom(1.5 * (200 / 100))).toBe(2);
  });

  it("rejects an invalid zoom instead of corrupting canonical geometry", () => {
    expect(() =>
      screenToPage(
        { x: 1, y: 1 },
        { left: 0, top: 0 },
        { pageOriginX: 0, pageOriginY: 0, zoom: 0 },
      ),
    ).toThrow(RangeError);
  });
});
