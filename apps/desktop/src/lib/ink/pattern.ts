// Filled geometry for solid, dashed, and dotted ink.
//
// Pattern spacing follows the centreline in page points, then every visible piece becomes a
// filled silhouette. This keeps pressure-sensitive ink, zoom, and export consistent. The Rust
// mirror lives in `crates/goodtype-core/src/pattern.rs`.

import type { StrokePattern } from "../model";
import {
  outlinePoints,
  outlineDot,
  roundedOutlinePoints,
  outlineWidthAt,
  type OutlineOptions,
  type OutlinePoint,
} from "./outline";

type MeasuredPoint = { point: OutlinePoint; distance: number };
export type InkPolygon = { x: number; y: number }[];

function measured(points: OutlinePoint[]): MeasuredPoint[] {
  if (points.length === 0) return [];
  const path: MeasuredPoint[] = [{ point: points[0], distance: 0 }];
  for (const point of points.slice(1)) {
    const previous = path[path.length - 1];
    const distance = Math.hypot(point.x - previous.point.x, point.y - previous.point.y);
    if (distance > 0) path.push({ point, distance: previous.distance + distance });
  }
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

function dashedRuns(path: MeasuredPoint[], widthPt: number, spacing: number, offset: number): OutlinePoint[][] {
  const total = path[path.length - 1].distance;
  const dash = Math.max(widthPt * 4, 6);
  const gap = Math.max(widthPt * 2.5, 3.5) * spacing;
  const cursor = { segment: 0 };
  const runs: OutlinePoint[][] = [];
  // A fragment continues its original's dashes, so the first one may already be under way.
  for (let start = offset > 0 ? -(offset % (dash + gap)) : 0; start < total; start += dash + gap) {
    const end = Math.min(start + dash, total);
    if (end <= 0) continue;
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
    runs.push(run);
  }
  return runs;
}

function dottedRuns(path: MeasuredPoint[], widthPt: number, factor: number, offset: number): OutlinePoint[][] {
  const total = path[path.length - 1].distance;
  const cursor = { segment: 0 };
  // Centre to centre. Even at the smallest factor this stays above one width, so dots never merge.
  const spacing = Math.max(widthPt * 2.75, 4) * factor;
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
    return runs.map(([point]) => outlineDot(point, outlineWidthAt(point, options) / 2));
  }

  const segmentOptions = { ...options, taper: 0, trim: undefined };
  return runs.flatMap((run) => {
    const first = run[0];
    const last = run[run.length - 1];
    if (run.length === 1) {
      return [outlineDot(first, outlineWidthAt(first, options) / 2)];
    }
    return [
      ...(round ? roundedOutlinePoints(run, segmentOptions) : [outlinePoints(run, segmentOptions)]),
      outlineDot(first, outlineWidthAt(first, options) / 2),
      outlineDot(last, outlineWidthAt(last, options) / 2),
    ];
  });
}
