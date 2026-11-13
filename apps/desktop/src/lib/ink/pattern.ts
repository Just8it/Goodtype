// Filled geometry for solid, dashed, and dotted ink.
//
// Pattern spacing follows the centreline in page points, then every visible piece becomes a
// filled silhouette. This keeps pressure-sensitive ink, zoom, and export consistent. The Rust
// mirror lives in `crates/goodtype-core/src/pattern.rs`.

import type { StrokePattern } from "../model";
import {
  keepSample,
  maximumTaperLength,
  outlinePoints,
  outlineDot,
  roundedCaps,
  roundedJoin,
  roundedOutlinePoints,
  roundedStrip,
  outlineWidthAt,
  sampleRadius,
  segmentAngle,
  span,
  taperForLength,
  taperLengthFor,
  type OutlineOptions,
  type OutlinePoint,
  type OutlineSample,
} from "./outline";

type MeasuredPoint = { point: OutlinePoint; distance: number };
export type InkPolygon = { x: number; y: number }[];

/** Append `point` to a measured centreline; a sample that does not move adds nothing. */
function measure(path: MeasuredPoint[], point: OutlinePoint): void {
  if (path.length === 0) {
    path.push({ point, distance: 0 });
    return;
  }
  const previous = path[path.length - 1];
  const distance = Math.hypot(point.x - previous.point.x, point.y - previous.point.y);
  if (distance > 0) path.push({ point, distance: previous.distance + distance });
}

function measured(points: OutlinePoint[]): MeasuredPoint[] {
  const path: MeasuredPoint[] = [];
  for (const point of points) measure(path, point);
  return path;
}

function interpolate(from: MeasuredPoint, to: MeasuredPoint, distance: number): OutlinePoint {
  const span = to.distance - from.distance;
  const amount = span > 0 ? Math.min(1, Math.max(0, (distance - from.distance) / span)) : 0;
  return {
    x: from.point.x + (to.point.x - from.point.x) * amount,
    y: from.point.y + (to.point.y - from.point.y) * amount,
    pressure: from.point.pressure + (to.point.pressure - from.point.pressure) * amount,
  };
}

function pointAt(path: MeasuredPoint[], distance: number, cursor: { segment: number }): OutlinePoint {
  while (
    cursor.segment + 1 < path.length &&
    path[cursor.segment + 1].distance < distance
  ) {
    cursor.segment += 1;
  }
  const next = path[cursor.segment + 1];
  return next
    ? interpolate(path[cursor.segment], next, distance)
    : path[path.length - 1].point;
}

/** Factor applied to the gap between marks; the marks themselves keep their size. */
export const MIN_PATTERN_SPACING = 0.5;
export const MAX_PATTERN_SPACING = 3;

function dashLengths(widthPt: number, spacing: number): { dash: number; gap: number } {
  return { dash: Math.max(widthPt * 4, 6), gap: Math.max(widthPt * 2.5, 3.5) * spacing };
}

/** Centre-to-centre dot spacing. Even at the smallest factor this stays above one width, so dots never merge. */
function dotSpacing(widthPt: number, factor: number): number {
  return Math.max(widthPt * 2.75, 4) * factor;
}

/** The centreline of the dash that begins `start` along the path. */
function dashRun(path: MeasuredPoint[], start: number, end: number, cursor: { segment: number }): OutlinePoint[] {
  const run = [pointAt(path, Math.max(start, 0), cursor)];
  let inside = cursor.segment + 1;
  while (inside < path.length && path[inside].distance < end) {
    run.push(path[inside].point);
    inside += 1;
  }
  const last = pointAt(path, end, cursor);
  const previous = run.at(-1);
  if (
    !previous ||
    previous.x !== last.x ||
    previous.y !== last.y ||
    previous.pressure !== last.pressure
  ) {
    run.push(last);
  }
  return run;
}

function dashedRuns(path: MeasuredPoint[], widthPt: number, spacing: number, offset: number): OutlinePoint[][] {
  const total = path[path.length - 1].distance;
  const { dash, gap } = dashLengths(widthPt, spacing);
  const cursor = { segment: 0 };
  const runs: OutlinePoint[][] = [];
  // A fragment continues its original's dashes, so the first one may already be under way.
  for (let start = offset > 0 ? -(offset % (dash + gap)) : 0; start < total; start += dash + gap) {
    const end = Math.min(start + dash, total);
    if (end <= 0) continue;
    runs.push(dashRun(path, start, end, cursor));
  }
  return runs;
}

function dottedRuns(path: MeasuredPoint[], widthPt: number, factor: number, offset: number): OutlinePoint[][] {
  const total = path[path.length - 1].distance;
  const cursor = { segment: 0 };
  const spacing = dotSpacing(widthPt, factor);
  const runs: OutlinePoint[][] = [];

  // Keep the phase fixed at the stroke origin. Redistributing dots across the
  // current length makes every existing dot drift whenever a sample arrives. A fragment's first
  // dot is the next one its original would have drawn.
  const first = offset > 0 ? (spacing - (offset % spacing)) % spacing : 0;
  for (let distance = first; distance <= total; distance += spacing) {
    runs.push([pointAt(path, distance, cursor)]);
  }

  return runs;
}

/**
 * Centreline runs for the chosen pattern. `offset` is how far along its original stroke a fragment
 * begins; the phase continues from it.
 */
export function patternRuns(
  points: OutlinePoint[],
  widthPt: number,
  pattern: StrokePattern,
  spacing = 1,
  offset = 0,
): OutlinePoint[][] {
  if (points.length === 0) return [];
  if (pattern === "solid") return [points];
  const path = measured(points);
  if (path.length === 1) return [[path[0].point]];
  return pattern === "dashed"
    ? dashedRuns(path, widthPt, spacing, offset)
    : dottedRuns(path, widthPt, spacing, offset);
}

function dotPolygon(point: OutlinePoint, options: OutlineOptions): InkPolygon {
  return outlineDot(point, outlineWidthAt(point, options) / 2);
}

/** One dash: its body without taper, and a round mark on each end. */
function dashPolygons(run: OutlinePoint[], options: OutlineOptions, round: boolean): InkPolygon[] {
  const first = run[0];
  const last = run[run.length - 1];
  if (run.length === 1) return [dotPolygon(first, options)];
  const segmentOptions = { ...options, taper: 0, trim: undefined };
  return [
    ...(round ? roundedOutlinePoints(run, segmentOptions) : [outlinePoints(run, segmentOptions)]),
    dotPolygon(first, options),
    dotPolygon(last, options),
  ];
}

/** Fillable polygons for the complete patterned stroke. */
export function patternedOutlinePoints(
  points: OutlinePoint[],
  options: OutlineOptions,
  pattern: StrokePattern,
  spacing = 1,
  round = false,
): InkPolygon[] {
  const runs = patternRuns(points, options.widthPt, pattern, spacing, options.trim?.start ?? 0);
  if (pattern === "solid") {
    return runs.flatMap((run) => round ? roundedOutlinePoints(run, options) : [outlinePoints(run, options)])
      .filter((polygon) => polygon.length > 0);
  }
  if (pattern === "dotted") {
    return runs.map(([point]) => dotPolygon(point, options));
  }
  return runs.flatMap((run) => dashPolygons(run, options, round));
}

/** How the stroke under the pen is drawn; fixed for the life of one `LiveInkOutline`. */
export type LiveInkStyle = {
  widthPt: number;
  pressure: boolean;
  /** Taper fraction of the stroke's length. */
  taper: number;
  /** The most length a taper may cover (`limitedTaper`), or null to apply the fraction as is. */
  taperMaximumPt: number | null;
  pattern: StrokePattern;
  spacing: number;
  round: boolean;
};

/**
 * A run of polygons in paint order. A stable run is the same array on every frame and only ever
 * grows at its end, so a painter may keep what it built from it and add only the new polygons.
 */
export type LiveInkRun = { polygons: InkPolygon[]; stable: boolean };

export type LiveInkFrame = {
  /** Concatenated, exactly `patternedOutlinePoints` of the samples so far. */
  runs: LiveInkRun[];
  /** Polygons computed during this update: the frame's geometry work. */
  built: number;
};

/**
 * Geometry for the stroke still under the pen, kept between frames.
 *
 * Rebuilding every polygon each frame makes a frame's cost grow with the stroke. Most of a stroke
 * stops changing once the pen has moved on, so finished pieces are kept and only the pieces that
 * can still change are rebuilt:
 *
 * - round solid ink: a strip or join is final once its samples' radii are; a taper can reach at
 *   most `maximumTaperLength` from either end, so only samples that far from both ends are frozen,
 *   and the head and tail inside that reach are rebuilt every frame as the length moves;
 * - dashes: every dash that ends before the pen is final (the phase is fixed at the origin);
 *   only the dash being drawn is rebuilt;
 * - dots: a dot is final once the pen has passed it.
 *
 * Flat (highlighter) solid ink is one polygon whose two sides meet at the moving end, so it
 * cannot be split without changing the silhouette; it is rebuilt whole every frame.
 */
export class LiveInkOutline {
  private seen = 0;
  private rawLength = 0;
  // Solid: the filtered centreline, its arc length, and the frozen middle.
  private path: OutlinePoint[] = [];
  private cumulative: number[] = [];
  private spread = false;
  private headEnd: number | null = null;
  private tailStart = 0;
  private frozenStrips: InkPolygon[] = [];
  private joinsDone = 0;
  private frozenJoins: InkPolygon[] = [];
  // Patterned: the measured centreline and the marks already final.
  private measuredPath: MeasuredPoint[] = [];
  private cursor = { segment: 0 };
  private nextMark = 0;
  private frozenMarks: InkPolygon[] = [];

  constructor(readonly style: LiveInkStyle) {}

  /** Whether this outline can continue a stroke drawn in `style`; otherwise start a new one. */
  matches(style: LiveInkStyle): boolean {
    const own = this.style;
    return own.widthPt === style.widthPt && own.pressure === style.pressure && own.taper === style.taper &&
      own.taperMaximumPt === style.taperMaximumPt && own.pattern === style.pattern &&
      own.spacing === style.spacing && own.round === style.round;
  }

  /** `points` is the gesture's sample array: it only grows, so samples already seen are skipped. */
  update(points: OutlinePoint[]): LiveInkFrame {
    if (points.length < this.seen) throw new Error("A live stroke's samples only grow.");
    for (let index = this.seen; index < points.length; index += 1) this.add(points, index);
    this.seen = points.length;
    const options = this.options();
    const { pattern, round, spacing } = this.style;
    if (pattern === "solid") {
      if (!round || this.path.length <= 2 || !this.spread) return this.whole(points, options);
      return this.solid(options);
    }
    if (this.measuredPath.length < 2) return this.whole(points, options);
    return pattern === "dashed" ? this.dashed(options, spacing, round) : this.dotted(options, spacing);
  }

  private add(points: OutlinePoint[], index: number): void {
    const point = points[index];
    if (index > 0) this.rawLength += span(points[index - 1], point);
    if (this.style.pattern !== "solid") {
      measure(this.measuredPath, point);
      return;
    }
    const path = this.path;
    if (path.length === 0) {
      path.push(point);
      this.cumulative.push(0);
      return;
    }
    const before = path.length;
    keepSample(path, point);
    if (path.length === before) return;
    const kept = path[before], previous = path[before - 1];
    this.cumulative.push(this.cumulative[before - 1] + Math.hypot(kept.x - previous.x, kept.y - previous.y));
    // Until a sample leaves half a width of the first, the stroke is drawn as a single dot.
    if (span(kept, path[0]) > this.style.widthPt / 2) this.spread = true;
  }

  private options(): OutlineOptions {
    const { widthPt, pressure, taper, taperMaximumPt } = this.style;
    return {
      widthPt,
      pressure,
      taper: taperMaximumPt === null ? taper : taperForLength(this.rawLength, taper, taperMaximumPt),
    };
  }

  private whole(points: OutlinePoint[], options: OutlineOptions): LiveInkFrame {
    const polygons = patternedOutlinePoints(points, options, this.style.pattern, this.style.spacing, this.style.round);
    return { runs: [{ polygons, stable: false }], built: polygons.length };
  }

  private solid(options: OutlineOptions): LiveInkFrame {
    const path = this.path, cumulative = this.cumulative, count = path.length;
    const total = cumulative[count - 1];
    const taperLength = taperLengthFor(total, options);
    // No taper reaches further than this from either end, so a sample beyond it on both sides
    // keeps its radius however the stroke goes on.
    const reach = maximumTaperLength(options);
    const sample = (index: number): OutlineSample => ({
      point: path[index],
      radius: sampleRadius(path[index], cumulative[index], total, taperLength, options),
    });
    const angle = (index: number) => segmentAngle(sample(index), sample(index + 1));
    let built = 0;

    if (this.headEnd === null) {
      const first = cumulative.findIndex((distance) => distance >= reach);
      if (first >= 0) {
        this.headEnd = first;
        this.tailStart = first;
        this.joinsDone = Math.max(first, 1);
      }
    }
    while (this.tailStart < count && total - cumulative[this.tailStart] >= reach) this.tailStart += 1;
    const head = this.headEnd ?? count - 1;
    // Samples in [head, tailStart) are final. A strip needs both its samples final; a join needs
    // its own sample final and a segment after it.
    if (this.headEnd !== null) {
      for (let index = head + this.frozenStrips.length; index + 1 < this.tailStart; index += 1) {
        this.frozenStrips.push(roundedStrip(sample(index), sample(index + 1), angle(index)));
        built += 1;
      }
      for (; this.joinsDone < this.tailStart && this.joinsDone + 1 < count; this.joinsDone += 1) {
        const join = roundedJoin(sample(this.joinsDone), angle(this.joinsDone - 1), angle(this.joinsDone));
        if (join) this.frozenJoins.push(join);
        built += 1;
      }
    }
    const stripsFrozenEnd = head + this.frozenStrips.length;
    const joinStart = this.headEnd === null ? count - 1 : Math.max(this.headEnd, 1);
    const joinsFrozenEnd = this.headEnd === null ? joinStart : this.joinsDone;

    const strips = (from: number, to: number) => {
      const polygons: InkPolygon[] = [];
      for (let index = from; index < to; index += 1) polygons.push(roundedStrip(sample(index), sample(index + 1), angle(index)));
      return polygons;
    };
    const joins = (from: number, to: number) => {
      const polygons: InkPolygon[] = [];
      for (let index = Math.max(from, 1); index < to; index += 1) {
        const join = roundedJoin(sample(index), angle(index - 1), angle(index));
        if (join) polygons.push(join);
      }
      return polygons;
    };
    const headStrips = strips(0, head);
    const tailStrips = strips(stripsFrozenEnd, count - 1);
    const caps = roundedCaps(sample(0), sample(count - 1), angle(0), angle(count - 2));
    const headJoins = joins(1, joinStart);
    const tailJoins = joins(joinsFrozenEnd, count - 1);
    built += headStrips.length + tailStrips.length + caps.length + headJoins.length + tailJoins.length;
    return {
      runs: [
        { polygons: headStrips, stable: false },
        { polygons: this.frozenStrips, stable: true },
        { polygons: [...tailStrips, ...caps, ...headJoins], stable: false },
        { polygons: this.frozenJoins, stable: true },
        { polygons: tailJoins, stable: false },
      ],
      built,
    };
  }

  private dashed(options: OutlineOptions, spacing: number, round: boolean): LiveInkFrame {
    const path = this.measuredPath;
    const total = path[path.length - 1].distance;
    const { dash, gap } = dashLengths(options.widthPt, spacing);
    let built = 0;
    // A dash that ends before the pen is final; the phase is fixed at the stroke's origin.
    while (this.nextMark < total && this.nextMark + dash <= total) {
      const polygons = dashPolygons(dashRun(path, this.nextMark, this.nextMark + dash, this.cursor), options, round);
      this.frozenMarks.push(...polygons);
      built += polygons.length;
      this.nextMark += dash + gap;
    }
    const drawing = this.nextMark < total
      ? dashPolygons(dashRun(path, this.nextMark, total, { ...this.cursor }), options, round)
      : [];
    built += drawing.length;
    return { runs: [{ polygons: this.frozenMarks, stable: true }, { polygons: drawing, stable: false }], built };
  }

  private dotted(options: OutlineOptions, factor: number): LiveInkFrame {
    const path = this.measuredPath;
    const total = path[path.length - 1].distance;
    const spacing = dotSpacing(options.widthPt, factor);
    let built = 0;
    for (; this.nextMark <= total; this.nextMark += spacing) {
      this.frozenMarks.push(dotPolygon(pointAt(path, this.nextMark, this.cursor), options));
      built += 1;
    }
    return { runs: [{ polygons: this.frozenMarks, stable: true }], built };
  }
}
