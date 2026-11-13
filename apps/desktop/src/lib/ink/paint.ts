// Turning committed strokes into the handful of SVG paths that display them.
//
// Committed ink is SVG rather than canvas so that zoom is a transform the browser composites,
// instead of JavaScript re-rasterising every stroke. That only pays off if a page does not cost
// one DOM node per stroke, which is what the merging here is for — and merging is only possible
// at all because a stroke is a filled silhouette rather than a stroked centreline.

import { DEFAULT_INK_Z_INDEX, type Stroke } from "../model";
import { outlinePath, type OutlineTrim } from "./outline";
import { patternedOutlinePoints } from "./pattern";
import { strokeBounds, transformedPoint } from "./selection";

export type PaintedPath = {
  /** The id of the first stroke in the bucket, so the keyed each block stays stable. */
  key: string;
  d: string;
  color: string;
  opacity: number;
  selected?: boolean;
};

export type PaintedStratum = {
  key: string;
  zIndex: number;
  paths: PaintedPath[];
};

/**
 * Silhouettes are cached per stroke object, because the whole set is rebuilt on any change — a new
 * stroke, a selection drag, a zoom step — and recomputing five thousand outlines each time costs
 * tens of milliseconds that land directly in the drag. A committed stroke is immutable, so the
 * object's identity is a sound key: editing one produces a new object and misses the cache, while
 * the thousands left alone hit it. Weak, so nothing here keeps a deleted stroke alive.
 */
const silhouettes = new WeakMap<Stroke, { widthPt: number; d: string }>();
const strataCache = new WeakMap<Stroke[], {
  thinnest: number;
  boundaries: string;
  floor: number;
  selection: string;
  selectionScale: number;
  live: string;
  strata: PaintedStratum[];
}>();

// A4 is 595 x 842 pt; 128 pt cells keep merged bounds near raster tiles of a few hundred px.
const INK_CELL_SIZE_PT = 128;

/**
 * The last merged path built for a bucket, keyed by the bucket's first stroke. A bucket made of the very
 * same silhouette strings reuses the very same path string, so the DOM sees an unchanged `d` and
 * re-parses nothing. That is what keeps an erase gesture from re-parsing the page's ink each frame.
 */
const runCache = new WeakMap<Stroke, { parts: string[]; d: string }>();

function joinedRun(first: Stroke, parts: string[]): string {
  const cached = runCache.get(first);
  if (cached && cached.parts.length === parts.length && cached.parts.every((part, index) => part === parts[index])) {
    return cached.d;
  }
  const d = parts.join(" ");
  runCache.set(first, { parts, d });
  return d;
}

export function silhouetteWidth(stroke: Stroke, minimumWidthPt: number): number {
  return Math.max(
    stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY),
    minimumWidthPt,
  );
}

export function hasSilhouette(stroke: Stroke, minimumWidthPt: number): boolean {
  return silhouettes.get(stroke)?.widthPt === silhouetteWidth(stroke, minimumWidthPt);
}

export function installSilhouette(stroke: Stroke, minimumWidthPt: number, d: string): void {
  silhouettes.set(stroke, { widthPt: silhouetteWidth(stroke, minimumWidthPt), d });
}

export function silhouette(stroke: Stroke, minimumWidthPt: number): string {
  const widthPt = silhouetteWidth(stroke, minimumWidthPt);
  const cached = silhouettes.get(stroke);
  // The floor moves with zoom, so a cached path is only reusable at the width it was built for.
  if (cached && cached.widthPt === widthPt) return cached.d;

  const d = patternedOutlinePoints(
    stroke.points.map((point) => ({ ...transformedPoint(stroke, point), pressure: point.pressure })),
    { widthPt, pressure: stroke.pressure, taper: stroke.taper, trim: placedTrim(stroke) },
    stroke.pattern ?? "solid",
    stroke.patternSpacing ?? 1,
    stroke.tool === "pen",
  )
    .map(outlinePath)
    .join(" ");
  silhouettes.set(stroke, { widthPt, d });
  return d;
}

/**
 * A fragment's lineage in page points. It is stored in the stroke's own points and the silhouette
 * is built after the transform, so it scales exactly as the width does; the export does the same.
 */
export function placedTrim(stroke: Stroke): OutlineTrim | undefined {
  if (!stroke.trim) return undefined;
  const scale = Math.max(stroke.transform.scaleX, stroke.transform.scaleY);
  return { start: stroke.trim.startPt * scale, end: stroke.trim.endPt * scale };
}

/**
 * The committed strokes as painted paths, in paint order.
 *
 * Consecutive opaque strokes of one colour merge within spatial buckets of a run. Only
 * *consecutive* ones, and never translucent ones: merging changes paint order, and paint order is
 * visible. Reordering across an intervening colour would change what covers what, and collapsing
 * two overlapping highlighter sweeps into one fill would stop them darkening where they cross —
 * which is wrong on screen and disagrees with the PDF, where every stroke is its own shape.
 *
 * `minimumWidthPt` keeps hairline ink from vanishing when the page is zoomed far out.
 *
 * `liveIds` are strokes an active gesture is still changing, such as fragments under the precise
 * eraser. They paint as runs of their own, so each frame replaces only their paths and every other
 * run keeps its exact string.
 */
export function paintOrder(
  strokes: Stroke[],
  minimumWidthPt = 0,
  selectedIds?: ReadonlySet<string>,
  selectionScale = 1,
  liveIds?: ReadonlySet<string>,
): PaintedPath[] {
  type Bucket = { first: Stroke; parts: string[] };
  const paths: PaintedPath[] = [];
  let run: { color: string; opacity: number; selected?: boolean; live?: boolean; buckets: Map<string, Bucket> } | undefined;
  const flush = () => {
    if (!run) return;
    for (const { first, parts } of run.buckets.values()) {
      paths.push({ key: first.id, d: joinedRun(first, parts), color: run.color, opacity: run.opacity,
        ...(selectedIds ? { selected: run.selected } : {}) });
    }
  };
  for (const stroke of strokes) {
    const selected = selectedIds?.has(stroke.id);
    const live = liveIds?.has(stroke.id);
    const floor = selected ? minimumWidthPt / selectionScale : minimumWidthPt;
    const d = silhouette(stroke, floor);
    if (!d) continue;

    if (!run || run.opacity !== 1 || stroke.opacity !== 1 || run.color !== stroke.color ||
      run.selected !== selected || run.live !== live) {
      flush();
      run = { color: stroke.color, opacity: stroke.opacity, selected, live, buckets: new Map() };
    }
    // Bounds already cache transformed page-space points by immutable stroke identity.
    // Include the rendered width when deciding whether a stroke would inflate a cell.
    const bounds = strokeBounds(stroke)!;
    const width = silhouetteWidth(stroke, floor);
    const cell = bounds.right - bounds.left + width > 2 * INK_CELL_SIZE_PT ||
      bounds.bottom - bounds.top + width > 2 * INK_CELL_SIZE_PT
      ? "large"
      : `${Math.floor((bounds.left + bounds.right) / (2 * INK_CELL_SIZE_PT))},${Math.floor((bounds.top + bounds.bottom) / (2 * INK_CELL_SIZE_PT))}`;
    const bucket = run.buckets.get(cell);
    if (bucket) bucket.parts.push(d);
    else run.buckets.set(cell, { first: stroke, parts: [d] });
  }
  flush();
  return paths;
}

/** Collapse ink into at most one SVG stratum between each pair of movable objects. */
export function paintStrata(
  strokes: Stroke[],
  objectZIndices: number[],
  minimumWidthPt = 0,
  selectedIds?: string[],
  selectionScale = 1,
  liveIds?: ReadonlySet<string>,
): PaintedStratum[] {
  const boundaries = [...objectZIndices].sort((left, right) => left - right);
  const cached = strataCache.get(strokes);
  const thinnest = cached?.thinnest ?? strokes.reduce((minimum, stroke) =>
    Math.min(minimum, stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY)), Infinity);
  // Ordinary-width ink has identical geometry across zoom tiers; keep its SVG strings intact.
  const floor = minimumWidthPt <= thinnest * Math.min(1, selectionScale) ? 0 : minimumWidthPt;
  const cachedScale = floor && selectedIds?.length ? selectionScale : 1;
  const boundaryKey = boundaries.join(",");
  const selection = JSON.stringify(selectedIds ?? []);
  const live = liveIds?.size ? [...liveIds].join(",") : "";
  if (cached && cached.floor === floor && cached.boundaries === boundaryKey && cached.selection === selection &&
    cached.selectionScale === cachedScale && cached.live === live) return cached.strata;
  const selected = selectedIds?.length ? new Set(selectedIds) : undefined;
  const groups = new Map<number, Stroke[]>();
  let band = 0;
  for (const stroke of orderedStrokes(strokes)) {
    const zIndex = stroke.zIndex ?? DEFAULT_INK_Z_INDEX;
    while (band < boundaries.length && boundaries[band] <= zIndex) band += 1;
    const group = groups.get(band);
    if (group) group.push(stroke);
    else groups.set(band, [stroke]);
  }
  const strata = [...groups.values()].map((group) => ({
    key: group[0].id,
    zIndex: group[0].zIndex ?? DEFAULT_INK_Z_INDEX,
    paths: paintOrder(group, floor, selected, selectionScale, liveIds?.size ? liveIds : undefined),
  }));
  strataCache.set(strokes, { thinnest, boundaries: boundaryKey, floor, selection, selectionScale: cachedScale, live, strata });
  return strata;
}

function orderedStrokes(strokes: Stroke[]): Stroke[] {
  for (let index = 1; index < strokes.length; index += 1) {
    if (
      (strokes[index - 1].zIndex ?? DEFAULT_INK_Z_INDEX) >
      (strokes[index].zIndex ?? DEFAULT_INK_Z_INDEX)
    ) {
      return strokes
        .map((stroke, order) => ({ stroke, order }))
        .sort(
          (left, right) =>
            (left.stroke.zIndex ?? DEFAULT_INK_Z_INDEX) -
              (right.stroke.zIndex ?? DEFAULT_INK_Z_INDEX) ||
            left.order - right.order,
        )
        .map(({ stroke }) => stroke);
    }
  }
  return strokes;
}
