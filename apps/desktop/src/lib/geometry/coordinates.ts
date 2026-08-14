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

/** Dragging the viewport moves its content with the finger, so scroll travels the other way. */
export function pannedScroll(startScroll: Point, startPointer: Point, pointer: Point): Point {
  return {
    x: startScroll.x - (pointer.x - startPointer.x),
    y: startScroll.y - (pointer.y - startPointer.y),
  };
}

/** Retain the same fraction of velocity per 60 Hz frame, independent of the actual frame rate. */
export function dampedVelocity(velocity: Point, elapsedMs: number, retention: number): Point {
  const factor = retention ** (Math.max(elapsedMs, 0) / (1000 / 60));
  return { x: velocity.x * factor, y: velocity.y * factor };
}

/** Keep a page-sized raster under its memory budget without exceeding device resolution. */
export function boundedRasterScale(size: Size, density: number, maxPixels: number): number {
  return Math.min(density, Math.sqrt(maxPixels / (Math.max(size.width, 1) * Math.max(size.height, 1))));
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

/**
 * How far out the page may be pushed. A quarter scale puts an A4 sheet at about 150pt tall,
 * which is a thumbnail — far enough to see a whole page on a small screen and no further.
 */
export const MIN_ZOOM = 0.25;

/**
 * How far in.
 *
 * The ceiling used to be 2. A point is 1/72 inch and a CSS pixel 1/96, so an unzoomed page is
 * already drawn at 0.75 of its physical size; on a 4K panel at 150% system scaling, 200% zoom is
 * roughly life size. That made the old maximum "you may look at the page, but never closer than
 * holding it" — which is the wrong limit for correcting a subscript or placing a stroke inside a
 * fraction. Eight is far enough that a 0.35mm nib is a comfortable stroke on screen.
 */
export const MAX_ZOOM = 8;

export function clampZoom(zoom: number, minimum = MIN_ZOOM, maximum = MAX_ZOOM): number {
  return Math.min(Math.max(validZoom(zoom), minimum), maximum);
}

function validZoom(zoom: number): number {
  if (!Number.isFinite(zoom) || zoom <= 0) {
    throw new RangeError("zoom must be a positive finite number");
  }
  return zoom;
}
