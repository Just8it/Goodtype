// The precise eraser: remove only the ink the eraser touched, and leave the rest exactly as it was.
//
// A stroke is a centreline with a width, so removing part of it means cutting the centreline. The
// cut is placed where the drawn ink stops touching the eraser, which differs by nib:
//
//   * A round pen is drawn as the union of disks of its local half-width, so a stretch is removed
//     where that disk meets the eraser. The round cap left behind then just touches the eraser's
//     edge at any crossing angle, and nothing of what remains lies under it.
//   * A highlighter is a flat band with square ends, so a stretch is removed where its cross-section
//     meets the eraser. The disk rule would widen every gap by half a band width.
//
// Both reduce to a convex distance along each straight segment, so each segment loses at most one
// interval per eraser stretch, found exactly by minimising and then bisecting. Fragments carry
// `trim` so their taper and pattern phase continue from the original (ADR 0037).
//
// The eraser path between pointer events is covered by capsules, not points, so a fast sweep does
// not leave beads of ink behind between samples.

import type { Point } from "../geometry/coordinates";
import type { Stroke, StrokePoint } from "../model";
import { inkRadii } from "./outline";
import { placedTrim } from "./paint";
import { quantizePoints } from "./pipeline";
import { distanceToSegment, strokeBounds, transformedPoint, type StrokeFilter } from "./selection";

/** One stretch of the eraser's path: everything within `radius` of the segment is under it. */
export type EraserSweep = { from: Point; to: Point; radius: number };

/** Most a layer may hold; Rust refuses more, so a cut past either would only fail to save. */
export type EraseLimits = { maxStrokes: number; maxPoints: number };

/** `MAX_INK_STROKES_PER_LAYER` and `MAX_INK_POINTS_PER_LAYER` in `goodtype-core/src/storage`. */
export const INK_LAYER_LIMITS: EraseLimits = { maxStrokes: 20_000, maxPoints: 750_000 };

export type PreciseErase = {
  strokes: Stroke[];
  /** Every stroke this pass changed, mapped to what replaced it: its fragments in order, or none. */
  replaced: Map<string, string[]>;
  /** A cut was skipped because the layer could not hold another stroke or sample. */
  limited: boolean;
};

/** Golden-section steps: 0.618⁴⁸ of a segment is far below any distance that can be drawn. */
const MINIMISE_STEPS = 48;
const BISECT_STEPS = 40;
/** Committed samples are stored at this precision (`quantizePoints`). */
const SAMPLE_QUANTUM_PT = 0.01;
const GOLDEN = (Math.sqrt(5) - 1) / 2;

const placedCache = new WeakMap<Stroke, Point[]>();

function placed(stroke: Stroke): Point[] {
  let points = placedCache.get(stroke);
  if (!points) {
    points = stroke.points.map((point) => transformedPoint(stroke, point));
    placedCache.set(stroke, points);
  }
  return points;
}

/**
 * Cut every reachable stroke the sweeps touch. Untouched strokes keep their identity, so every
 * cache keyed on them still hits; a pass that touched nothing returns the same array.
 */
export function eraseThrough(
  strokes: Stroke[],
  sweeps: EraserSweep[],
  options: { reaches?: StrokeFilter; newId: () => string; limits?: EraseLimits },
): PreciseErase {
  const replaced = new Map<string, string[]>();
  if (sweeps.length === 0) return { strokes, replaced, limited: false };
  const area = sweepBounds(sweeps);
  const reaches = options.reaches ?? (() => true);
  let strokeCount = strokes.length;
  let pointCount = options.limits ? strokes.reduce((count, stroke) => count + stroke.points.length, 0) : 0;
  let limited = false;
  const next: Stroke[] = [];
  for (const stroke of strokes) {
    const fragments = reaches(stroke) && near(stroke, area) ? cutStroke(stroke, sweeps, options.newId) : null;
    if (!fragments) {
      next.push(stroke);
      continue;
    }
    const addedStrokes = fragments.length - 1;
    const addedPoints = fragments.reduce((count, fragment) => count + fragment.points.length, 0) -
      stroke.points.length;
    // Removing ink always fits; only a cut that leaves more behind can overflow the layer.
    if (options.limits && fragments.length > 0 &&
      (strokeCount + addedStrokes > options.limits.maxStrokes ||
        pointCount + addedPoints > options.limits.maxPoints)) {
      limited = true;
      next.push(stroke);
      continue;
    }
    strokeCount += addedStrokes;
    pointCount += addedPoints;
    next.push(...fragments);
    replaced.set(stroke.id, fragments.map((fragment) => fragment.id));
  }
  return { strokes: replaced.size > 0 ? next : strokes, replaced, limited };
}

/**
 * Fold one pass's replacements into a gesture's, keyed by the strokes the gesture began with, so
 * a fragment cut again on a later frame still traces back to the stroke it came from.
 */
export function foldReplacements(total: Map<string, string[]>, step: Map<string, string[]>): void {
  for (const [id, fragments] of step) {
    let root: string | undefined;
    for (const [original, current] of total) {
      if (current.includes(id)) {
        root = original;
        break;
      }
    }
    if (root === undefined) total.set(id, fragments);
    else total.set(root, total.get(root)!.flatMap((current) => (current === id ? fragments : [current])));
  }
}

type Bounds = { left: number; top: number; right: number; bottom: number };

function sweepBounds(sweeps: EraserSweep[]): Bounds {
  const bounds = { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity };
  for (const { from, to, radius } of sweeps) {
    bounds.left = Math.min(bounds.left, from.x - radius, to.x - radius);
    bounds.top = Math.min(bounds.top, from.y - radius, to.y - radius);
    bounds.right = Math.max(bounds.right, from.x + radius, to.x + radius);
    bounds.bottom = Math.max(bounds.bottom, from.y + radius, to.y + radius);
  }
  return bounds;
}

function near(stroke: Stroke, area: Bounds): boolean {
  const bounds = strokeBounds(stroke);
  if (!bounds) return false;
  const reach = drawnWidth(stroke) / 2;
  return bounds.left - reach <= area.right && bounds.right + reach >= area.left &&
    bounds.top - reach <= area.bottom && bounds.bottom + reach >= area.top;
}

/** The width the silhouette is built at: the export's width, without the display hairline floor. */
function drawnWidth(stroke: Stroke): number {
  return stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY);
}

/** The cut geometry of one stroke, in parallel arrays. Usually the stroke's own samples. */
type Working = { local: StrokePoint[]; page: Point[]; radii: number[]; flat: boolean };

function working(stroke: Stroke): Working {
  let local = stroke.points;
  let page = placed(stroke);
  const solid = (stroke.pattern ?? "solid") === "solid";
  // Two tapered ends of a whole two-sample stroke are both points; the renderer then draws a
  // midpoint at width so the mark does not vanish. Cut against that same midpoint.
  if (local.length === 2 && solid && stroke.taper > 0 && !stroke.trim) {
    local = [local[0], lerpSample(local[0], local[1], 0.5), local[1]];
    page = [page[0], lerpPoint(page[0], page[1], 0.5), page[1]];
  }
  return { local, page, radii: radiiOf(stroke, local, page), flat: stroke.tool === "highlighter" };
}

function radiiOf(stroke: Stroke, local: StrokePoint[], page: Point[]): number[] {
  // Dashes and dots are drawn untapered (`pattern.ts`), so only solid ink follows the ramp.
  const solid = (stroke.pattern ?? "solid") === "solid";
  return inkRadii(
    page.map((point, index) => ({ ...point, pressure: local[index].pressure })),
    { widthPt: drawnWidth(stroke), pressure: stroke.pressure, taper: solid ? stroke.taper : 0,
      trim: solid ? placedTrim(stroke) : undefined },
  );
}

/** The stroke's fragments after the sweeps, `[]` when nothing is left, `null` when untouched. */
function cutStroke(stroke: Stroke, sweeps: EraserSweep[], newId: () => string): Stroke[] | null {
  const page = placed(stroke);
  const widthPt = drawnWidth(stroke);
  // A whole stroke whose samples all sit inside half a width is drawn as one full-width dot.
  if (page.length === 1 || (!stroke.trim && page.every((point) =>
    Math.hypot(point.x - page[0].x, point.y - page[0].y) <= widthPt / 2))) {
    const radius = stroke.trim ? radiiOf(stroke, stroke.points, page)[0] : widthPt / 2;
    return sweeps.some((sweep) => distanceToSegment(page[0], sweep.from, sweep.to) <= sweep.radius + radius)
      ? []
      : null;
  }

  const shape = working(stroke);
  const removed = contactIntervals(shape.page, shape.radii, shape.flat, sweeps);
  if (removed.length === 0) return null;

  const last = shape.page.length - 1;
  const kept = complement(removed, last);
  const lengths = cumulativeLengths(shape.local);
  const scale = Math.max(stroke.transform.scaleX, stroke.transform.scaleY);
  const fragments: Stroke[] = [];
  for (const [from, to] of kept) {
    const fragment = buildFragment(stroke, shape, lengths, from, to, sweeps, fragments.length === 0 ? stroke.id : newId());
    if (!fragment) continue;
    // A sliver of highlighter shorter than half its width reads as dirt, not as a mark.
    if (shape.flat && (lengthAt(lengths, shape.local, fragment.span[1]) -
      lengthAt(lengths, shape.local, fragment.span[0])) * scale < widthPt / 2) continue;
    fragments.push(fragment.stroke);
  }
  return fragments;
}

/**
 * Where a polyline's drawn ink meets the sweeps, as `[from, to]` positions (segment index plus
 * fraction). `radii` are the drawn half-widths at each point; `flat` measures square-ended bands by
 * their cross-section instead of by round caps. Shapes are cut with the same solver.
 */
export function contactIntervals(
  page: Point[],
  radii: number[],
  flat: boolean,
  sweeps: EraserSweep[],
): [number, number][] {
  const removed: [number, number][] = [];
  for (let index = 0; index + 1 < page.length; index += 1) {
    for (const sweep of sweeps) {
      const span = removedSpan(page, radii, flat, index, sweep);
      if (span) removed.push([index + span[0], index + span[1]]);
    }
  }
  return removed;
}

/** Merge removed intervals and return what is left of `[0, last]`, without zero-length gaps. */
export function complement(removed: [number, number][], last: number): [number, number][] {
  removed.sort((left, right) => left[0] - right[0]);
  const kept: [number, number][] = [];
  let cursor = 0;
  for (const [from, to] of removed) {
    if (from > cursor) kept.push([cursor, from]);
    cursor = Math.max(cursor, to);
  }
  if (cursor < last) kept.push([cursor, last]);
  return kept;
}

/**
 * The part of segment `index` the sweep removes, as `[t0, t1]` within the segment, or null.
 *
 * Contact is a convex function of the position along a straight segment — the distance from a
 * moving disk, or a moving cross-section, to a fixed capsule — so it is at most one interval.
 * Its ends are bisected onto the side the eraser did not reach.
 */
function removedSpan(
  page: Point[],
  radii: number[],
  flat: boolean,
  index: number,
  sweep: EraserSweep,
): [number, number] | null {
  const a = page[index];
  const b = page[index + 1];
  const ra = radii[index];
  const rb = radii[index + 1];
  if (segmentDistance(a, b, sweep.from, sweep.to) > sweep.radius + Math.max(ra, rb)) return null;
  const length = Math.hypot(b.x - a.x, b.y - a.y);
  const contact = flat && length > 0
    ? (t: number) => crossSectionDistance(a, b, length, ra + (rb - ra) * t, t, sweep)
    : (t: number) => distanceToSegment(lerpPoint(a, b, t), sweep.from, sweep.to) - (ra + (rb - ra) * t);
  const reach = sweep.radius;

  let low = 0;
  let high = 1;
  let left = high - GOLDEN * (high - low);
  let right = low + GOLDEN * (high - low);
  let leftValue = contact(left);
  let rightValue = contact(right);
  for (let step = 0; step < MINIMISE_STEPS; step += 1) {
    if (leftValue <= rightValue) {
      high = right;
      right = left;
      rightValue = leftValue;
      left = high - GOLDEN * (high - low);
      leftValue = contact(left);
    } else {
      low = left;
      left = right;
      leftValue = rightValue;
      right = low + GOLDEN * (high - low);
      rightValue = contact(right);
    }
  }
  let deepest = (low + high) / 2;
  let deepestValue = contact(deepest);
  for (const end of [0, 1]) {
    const value = contact(end);
    if (value < deepestValue) {
      deepest = end;
      deepestValue = value;
    }
  }
  if (deepestValue > reach) return null;

  const start = contact(0) <= reach ? 0 : bisect(contact, 0, deepest, reach, "outside-low");
  const end = contact(1) <= reach ? 1 : bisect(contact, deepest, 1, reach, "outside-high");
  return [start, end];
}

/** Find where `contact` crosses `reach`, returning the end of the bracket the eraser missed. */
function bisect(
  contact: (t: number) => number,
  low: number,
  high: number,
  reach: number,
  side: "outside-low" | "outside-high",
): number {
  for (let step = 0; step < BISECT_STEPS; step += 1) {
    const middle = (low + high) / 2;
    const inside = contact(middle) <= reach;
    if (side === "outside-low") {
      if (inside) high = middle;
      else low = middle;
    } else if (inside) low = middle;
    else high = middle;
  }
  return side === "outside-low" ? low : high;
}

/** Distance from the flat cross-section at `t` to the eraser's own segment. */
function crossSectionDistance(a: Point, b: Point, length: number, radius: number, t: number, sweep: EraserSweep): number {
  const centre = lerpPoint(a, b, t);
  const normal = { x: -(b.y - a.y) / length * radius, y: (b.x - a.x) / length * radius };
  return segmentDistance(
    { x: centre.x - normal.x, y: centre.y - normal.y },
    { x: centre.x + normal.x, y: centre.y + normal.y },
    sweep.from, sweep.to,
  );
}

function segmentDistance(a: Point, b: Point, c: Point, d: Point): number {
  const cross = (p: Point, q: Point, r: Point) => (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
  const abc = cross(a, b, c), abd = cross(a, b, d), cda = cross(c, d, a), cdb = cross(c, d, b);
  if (((abc > 0 && abd < 0) || (abc < 0 && abd > 0)) && ((cda > 0 && cdb < 0) || (cda < 0 && cdb > 0))) return 0;
  return Math.min(
    distanceToSegment(a, c, d), distanceToSegment(b, c, d),
    distanceToSegment(c, a, b), distanceToSegment(d, a, b),
  );
}

function cumulativeLengths(points: StrokePoint[]): number[] {
  const lengths = [0];
  for (let index = 1; index < points.length; index += 1) {
    lengths.push(lengths[index - 1] +
      Math.hypot(points[index].x - points[index - 1].x, points[index].y - points[index - 1].y));
  }
  return lengths;
}

/** Arc length, in the stroke's own points, at position `u` (segment index plus fraction). */
function lengthAt(lengths: number[], points: StrokePoint[], u: number): number {
  const index = Math.min(Math.floor(u), points.length - 2);
  const t = u - index;
  const segment = lengths[index + 1] - lengths[index];
  return lengths[index] + segment * t;
}

/** Arc length to the sample stored for position `u`: the segment's start plus the way to it. */
function storedLength(lengths: number[], points: StrokePoint[], u: number, stored: StrokePoint): number {
  if (Number.isInteger(u)) return lengths[u];
  const index = Math.min(Math.floor(u), points.length - 2);
  return lengths[index] + Math.hypot(stored.x - points[index].x, stored.y - points[index].y);
}

function sampleAt(points: StrokePoint[], u: number): StrokePoint {
  const index = Math.min(Math.floor(u), points.length - 2);
  return lerpSample(points[index], points[index + 1], u - index);
}

/**
 * One fragment between `from` and `to`. Its ends are new samples on the original centreline,
 * stored at the usual precision; rounding can move a cut end a hair back under the eraser, so each
 * new end is checked against the drawn geometry again and moved outward until it is clear.
 */
function buildFragment(
  stroke: Stroke,
  shape: Working,
  lengths: number[],
  from: number,
  to: number,
  sweeps: EraserSweep[],
  id: string,
): { stroke: Stroke; span: [number, number] } | null {
  const last = shape.local.length - 1;
  const scale = Math.max(stroke.transform.scaleX, stroke.transform.scaleY);
  const total = lengths[last];
  for (let attempt = 0; attempt < 6 && from < to; attempt += 1) {
    const points = fragmentPoints(shape.local, from, to);
    // Measured to the stored, rounded end samples, so the lineage describes the geometry on disk.
    const startPt = (stroke.trim?.startPt ?? 0) + storedLength(lengths, shape.local, from, points[0]);
    const endPt = (stroke.trim?.endPt ?? 0) + total -
      storedLength(lengths, shape.local, to, points[points.length - 1]);
    const fragment: Stroke = { ...stroke, id, trim: { startPt, endPt }, points };
    const clear = endClear(fragment, shape.flat, sweeps, from > 0, to < last);
    if (clear.start && clear.end) return { stroke: fragment, span: [from, to] };
    // Step one stored quantum outward along the centreline, in this segment's parameter.
    const step = (u: number) => {
      const index = Math.min(Math.floor(u), last - 1);
      const segment = (lengths[index + 1] - lengths[index]) * scale;
      return segment > 0 ? (SAMPLE_QUANTUM_PT * 2 ** attempt) / segment : 1;
    };
    if (!clear.start) from = Math.min(to, from + step(from));
    if (!clear.end) to = Math.max(from, to - step(to));
  }
  return null;
}

function fragmentPoints(points: StrokePoint[], from: number, to: number): StrokePoint[] {
  const first = Math.ceil(from);
  const final = Math.floor(to);
  const result: StrokePoint[] = [];
  const push = (point: StrokePoint) => {
    const previous = result.at(-1);
    if (!previous || previous.x !== point.x || previous.y !== point.y) result.push(point);
  };
  if (from < first) push(quantizePoints([sampleAt(points, from)])[0]);
  for (let index = first; index <= final; index += 1) push(points[index]);
  if (to > final) push(quantizePoints([sampleAt(points, to)])[0]);
  return result;
}

/** Whether each new end of a fragment, as it will be drawn, stays off every sweep. */
function endClear(
  fragment: Stroke,
  flat: boolean,
  sweeps: EraserSweep[],
  checkStart: boolean,
  checkEnd: boolean,
): { start: boolean; end: boolean } {
  const page = fragment.points.map((point) => transformedPoint(fragment, point));
  const radii = radiiOf(fragment, fragment.points, page);
  const clearAt = (index: number, neighbour: number) => sweeps.every((sweep) => {
    const point = page[index];
    const other = page[neighbour] ?? point;
    const length = Math.hypot(other.x - point.x, other.y - point.y);
    const distance = flat && length > 0
      ? crossSectionDistance(point, other, length, radii[index], 0, sweep)
      : distanceToSegment(point, sweep.from, sweep.to) - radii[index];
    return distance > sweep.radius;
  });
  const last = page.length - 1;
  return {
    start: !checkStart || clearAt(0, 1),
    end: !checkEnd || clearAt(last, last - 1),
  };
}

function lerpPoint(a: Point, b: Point, t: number): Point {
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
}

function lerpSample(a: StrokePoint, b: StrokePoint, t: number): StrokePoint {
  return {
    x: a.x + (b.x - a.x) * t,
    y: a.y + (b.y - a.y) * t,
    pressure: a.pressure + (b.pressure - a.pressure) * t,
    timeMs: a.timeMs + (b.timeMs - a.timeMs) * t,
    tiltX: a.tiltX + (b.tiltX - a.tiltX) * t,
    tiltY: a.tiltY + (b.tiltY - a.tiltY) * t,
  };
}
