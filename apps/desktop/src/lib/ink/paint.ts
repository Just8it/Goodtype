// Turning committed strokes into the handful of SVG paths that display them.
//
// Committed ink is SVG rather than canvas so that zoom is a transform the browser composites,
// instead of JavaScript re-rasterising every stroke. That only pays off if a page does not cost
// one DOM node per stroke, which is what the merging here is for — and merging is only possible
// at all because a stroke is a filled silhouette rather than a stroked centreline.

import type { Stroke } from "../model";
import { outlinePath, outlinePoints } from "./outline";
import { transformedPoint } from "./selection";

export type PaintedPath = {
  /** The id of the first stroke in the run, so the keyed each block stays stable. */
  key: string;
  d: string;
  color: string;
  opacity: number;
};

/**
 * Silhouettes are cached per stroke object, because the whole set is rebuilt on any change — a new
 * stroke, a selection drag, a zoom step — and recomputing five thousand outlines each time costs
 * tens of milliseconds that land directly in the drag. A committed stroke is immutable, so the
 * object's identity is a sound key: editing one produces a new object and misses the cache, while
 * the thousands left alone hit it. Weak, so nothing here keeps a deleted stroke alive.
 */
const silhouettes = new WeakMap<Stroke, { widthPt: number; d: string }>();

function silhouette(stroke: Stroke, minimumWidthPt: number): string {
  const widthPt = Math.max(
    stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY),
    minimumWidthPt,
  );
  const cached = silhouettes.get(stroke);
  // The floor moves with zoom, so a cached path is only reusable at the width it was built for.
  if (cached && cached.widthPt === widthPt) return cached.d;

  const d = outlinePath(
    outlinePoints(
      stroke.points.map((point) => ({ ...point, ...transformedPoint(stroke, point) })),
      { widthPt, pressure: stroke.pressure, taper: stroke.taper },
    ),
  );
  silhouettes.set(stroke, { widthPt, d });
  return d;
}

/**
 * The committed strokes as painted paths, in paint order.
 *
 * Consecutive opaque strokes of one colour collapse into subpaths of a single path. Only
 * *consecutive* ones, and never translucent ones: merging changes paint order, and paint order is
 * visible. Reordering across an intervening colour would change what covers what, and collapsing
 * two overlapping highlighter sweeps into one fill would stop them darkening where they cross —
 * which is wrong on screen and disagrees with the PDF, where every stroke is its own shape.
 *
 * `minimumWidthPt` keeps hairline ink from vanishing when the page is zoomed far out.
 */
export function paintOrder(strokes: Stroke[], minimumWidthPt = 0): PaintedPath[] {
  const painted: PaintedPath[] = [];
  for (const stroke of strokes) {
    const d = silhouette(stroke, minimumWidthPt);
    if (!d) continue;

    const previous = painted[painted.length - 1];
    if (
      previous &&
      previous.opacity === 1 &&
      stroke.opacity === 1 &&
      previous.color === stroke.color
    ) {
      previous.d += ` ${d}`;
      continue;
    }
    painted.push({ key: stroke.id, d, color: stroke.color, opacity: stroke.opacity });
  }
  return painted;
}
