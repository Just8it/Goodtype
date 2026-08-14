import { describe, expect, it } from "vitest";
import {
  boundedRasterScale,
  clampToPage,
  clampZoom,
  containToPage,
  dampedVelocity,
  MAX_ZOOM,
  MIN_ZOOM,
  pageToScreen,
  pannedScroll,
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
    expect(clampZoom(1.5 * (200 / 100))).toBe(3);
  });

  it("moves viewport scroll opposite a one-finger drag", () => {
    expect(pannedScroll({ x: 80, y: 300 }, { x: 100, y: 200 }, { x: 70, y: 260 })).toEqual({
      x: 110,
      y: 240,
    });
  });

  it("damps touch-scroll velocity consistently across frame lengths", () => {
    const velocity = dampedVelocity({ x: 1, y: -2 }, 1000 / 30, 0.8);
    expect(velocity.x).toBeCloseTo(0.64);
    expect(velocity.y).toBeCloseTo(-1.28);
  });

  it("bounds page-sized raster memory without reducing normal pages", () => {
    expect(boundedRasterScale({ width: 595, height: 842 }, 2, 6_000_000)).toBe(2);
    const a2Scale = boundedRasterScale({ width: 1191, height: 1684 }, 2, 6_000_000);
    expect(1191 * 1684 * a2Scale ** 2).toBeCloseTo(6_000_000);
  });

  // Pen samples must not be pinned to the edge: a hand running off the sheet mid-stroke would
  // produce a run of identical edge coordinates, which draws as a line along the boundary and is
  // stored and exported that way.
  it("lets a pen sample leave the page instead of pinning it to the edge", () => {
    const page = { width: 612, height: 792 };
    expect(containToPage({ x: -40, y: 830 }, page)).toEqual({ x: -40, y: 830 });
    // Still bounded, so a broken viewport cannot produce absurd geometry.
    expect(containToPage({ x: -1e6, y: 1e6 }, page)).toEqual({ x: -612, y: 1584 });
  });

  // The old ceiling of 2 was roughly life size on a 4K panel, so the closest you could get to
  // the page was holding it — no use for correcting a subscript.
  it("zooms far enough in to work inside a fraction, and far enough out to see the page", () => {
    expect(clampZoom(12)).toBe(MAX_ZOOM);
    expect(clampZoom(0.01)).toBe(MIN_ZOOM);
    expect(MAX_ZOOM).toBeGreaterThan(2);
    // Anything within the range is its own value, not a step from a fixed ladder.
    expect(clampZoom(3.7)).toBe(3.7);
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
