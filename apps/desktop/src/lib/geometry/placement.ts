// Keeping objects reachable on the page.
//
// An object dragged fully past an edge has nothing left to hit, so it cannot be selected, moved
// back, or deleted — it is lost while still being in the file and still being exported. Partial
// overhang is useful and stays allowed; total escape does not.

/** How much of an object must stay on the page. Roughly a fingertip, so it is always grabbable. */
export const MIN_VISIBLE_PT = 24;

export type PageBox = { widthPt: number; heightPt: number };
export type ViewRect = {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
};

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

/**
 * Place a floating control above its selected object, or below when the top edge is too close.
 * Coordinates are relative to `boundary`, ready for an absolutely positioned child.
 */
export function placeFloatingToolbar(
  anchor: ViewRect,
  toolbar: Pick<ViewRect, "width" | "height">,
  boundary: ViewRect,
  gap = 10,
  margin = 12,
): { left: number; top: number; side: "above" | "below" } {
  const above = anchor.top - toolbar.height - gap;
  const side = above >= boundary.top + margin ? "above" : "below";
  const wantedTop = side === "above" ? above : anchor.bottom + gap;
  const left = Math.min(
    Math.max(anchor.left + (anchor.width - toolbar.width) / 2, boundary.left + margin),
    boundary.right - toolbar.width - margin,
  );
  const top = Math.min(
    Math.max(wantedTop, boundary.top + margin),
    boundary.bottom - toolbar.height - margin,
  );
  return {
    left: left - boundary.left,
    top: top - boundary.top,
    side,
  };
}
