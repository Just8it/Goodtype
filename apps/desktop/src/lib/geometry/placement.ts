// Keeping objects reachable on the page.
//
// An object dragged fully past an edge has nothing left to hit, so it cannot be selected, moved
// back, or deleted — it is lost while still being in the file and still being exported. Partial
// overhang is useful and stays allowed; total escape does not.

/** How much of an object must stay on the page. Roughly a fingertip, so it is always grabbable. */
export const MIN_VISIBLE_PT = 24;

export type PageBox = { widthPt: number; heightPt: number };

/**
 * Clamp a top-left position so at least `MIN_VISIBLE_PT` of the object stays over the page on
 * each axis.
 *
 * Objects smaller than the minimum are held entirely on the page rather than being allowed to
 * hang off by their own size, which is what the naive bound would do.
 */
export function keepOnPage(
  position: { x: number; y: number },
  size: { widthPt: number; heightPt: number },
  page: PageBox,
  minimumVisiblePt = MIN_VISIBLE_PT,
): { x: number; y: number } {
  return {
    x: clampAxis(position.x, size.widthPt, page.widthPt, minimumVisiblePt),
    y: clampAxis(position.y, size.heightPt, page.heightPt, minimumVisiblePt),
  };
}

function clampAxis(value: number, extent: number, pageExtent: number, minimum: number): number {
  if (!Number.isFinite(value)) return 0;
  const visible = Math.min(minimum, Math.max(extent, 0));
  const lowest = visible - Math.max(extent, 0);
  const highest = pageExtent - visible;
  // A page narrower than the object leaves the bounds crossed; pinning to the low end keeps the
  // object's leading edge on the sheet rather than snapping it somewhere arbitrary.
  if (highest < lowest) return lowest;
  return Math.min(Math.max(value, lowest), highest);
}
