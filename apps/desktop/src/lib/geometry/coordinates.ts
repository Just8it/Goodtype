export type Point = { x: number; y: number };
export type Size = { width: number; height: number };

export type ScreenViewport = {
  left: number;
  top: number;
};

export type PageViewport = {
  pageOriginX: number;
  pageOriginY: number;
  zoom: number;
};

export function screenToViewport(point: Point, viewport: ScreenViewport): Point {
  return { x: point.x - viewport.left, y: point.y - viewport.top };
}

export function viewportToScreen(point: Point, viewport: ScreenViewport): Point {
  return { x: point.x + viewport.left, y: point.y + viewport.top };
}

export function viewportToPage(point: Point, view: PageViewport): Point {
  const zoom = validZoom(view.zoom);
  return {
    x: (point.x - view.pageOriginX) / zoom,
    y: (point.y - view.pageOriginY) / zoom,
  };
}

export function pageToViewport(point: Point, view: PageViewport): Point {
  const zoom = validZoom(view.zoom);
  return {
    x: view.pageOriginX + point.x * zoom,
    y: view.pageOriginY + point.y * zoom,
  };
}

export function screenToPage(
  point: Point,
  viewport: ScreenViewport,
  view: PageViewport,
): Point {
  return viewportToPage(screenToViewport(point, viewport), view);
}

export function pageToScreen(
  point: Point,
  viewport: ScreenViewport,
  view: PageViewport,
): Point {
  return viewportToScreen(pageToViewport(point, view), viewport);
}

export function clampToPage(point: Point, page: Size): Point {
  return {
    x: Math.min(Math.max(point.x, 0), page.width),
    y: Math.min(Math.max(point.y, 0), page.height),
  };
}

/**
 * Keep a sample within reach of the page without pinning it to the edge.
 *
 * Pen samples must *not* go through `clampToPage`. A hand that runs off the sheet mid-stroke
 * produces a run of samples all clamped to the same edge coordinate, which collapses into a
 * straight line drawn along the boundary — and it is stored that way and exported that way.
 * Letting the coordinates stay real means the stroke simply leaves the page, and the page's own
 * clipping hides what is outside. The bound here is only so a broken viewport cannot produce
 * absurd geometry; it is a sanity limit, not a layout rule.
 */
export function containToPage(point: Point, page: Size): Point {
  return {
    x: Math.min(Math.max(point.x, -page.width), page.width * 2),
    y: Math.min(Math.max(point.y, -page.height), page.height * 2),
  };
}

export function clampZoom(zoom: number, minimum = 0.5, maximum = 2): number {
  return Math.min(Math.max(validZoom(zoom), minimum), maximum);
}

function validZoom(zoom: number): number {
  if (!Number.isFinite(zoom) || zoom <= 0) {
    throw new RangeError("zoom must be a positive finite number");
  }
  return zoom;
}
