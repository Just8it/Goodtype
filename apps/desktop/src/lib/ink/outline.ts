// Variable-width stroke geometry.
//
// A stroke is not a stroked centreline — `stroke-width` is constant along a path, so a stroked
// polyline can never vary with pressure. Highlighters use a flat silhouette; writing pens use
// consistently wound strips, round end caps and outside join arcs, filled as one compound path.
//
// That one change fixes three things at once:
//   * pressure survives into the PDF, because export fills the same polygon the canvas does;
//   * overlapping pen joins add rather than cut holes through the stroke;
//   * highlighter alpha is even, because overlapping segments no longer double-darken at joints.
//
// This file is mirrored by `outline_points` in `crates/goodtype-core/src/outline.rs`. The two
// are pinned together by `fixtures/ink/outline.json`; if they drift, verification fails.

export type OutlinePoint = { x: number; y: number; pressure: number };

export type OutlineOptions = {
  /** Nominal stroke width in points. */
  widthPt: number;
  /** When false, pressure is ignored and the stroke keeps a constant width. */
  pressure: boolean;
  /** Fraction of the stroke length over which the ends taper to a point, 0 disables. */
  taper: number;
  /**
   * For a fragment left by the precise eraser: how much of the original stroke lies before and
   * after it, in the same units as the points. Absent for a stroke drawn whole.
   */
  trim?: OutlineTrim;
};

/** Arc length of the original stroke cut away before and after a fragment (ADR 0037). */
export type OutlineTrim = { start: number; end: number };

/** Narrowest a pressure-varying stroke gets, as a fraction of its nominal width. */
const MIN_PRESSURE_SCALE = 0.25;

/**
 * Samples closer together than this carry no usable direction — the vector between them is
 * dominated by pointer jitter, so the normal computed from it flips and bites a notch out of the
 * silhouette. Drawing slowly is what produces them, which is why a slow highlighter sweep came
 * out ragged while a quick one did not.
 */
const MIN_SAMPLE_SPACING_PT = 0.05;

/**
 * How far apart the two samples behind a central difference must be before the direction they
 * give is trusted. Larger than the dedup spacing, so a dense but legitimate curve is still
 * differentiated over a span long enough to mean something. Well-spaced samples reach this on
 * the immediate neighbours and behave exactly as a plain central difference.
 */
const MIN_DIRECTION_SPAN_PT = 0.5;

/**
 * A taper runs over at most this many nib widths.
 *
 * It used to be a pure fraction of arc length, which made a full-page sweep taper over
 * centimetres while a flick tapered over millimetres. A real nib tapers over a distance set by
 * the nib, not by how far the hand happened to travel.
 */
const TAPER_MAX_NIB_WIDTHS = 6;

/** Distance between two samples, used to decide whether they are far enough apart to mean anything. */
function span(a: OutlinePoint, b: OutlinePoint): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/**
 * Rounding is part of the contract: both implementations must produce identical numbers, so
 * this uses round-half-toward-positive-infinity (JavaScript's `Math.round`) and the Rust side
 * mimics it deliberately rather than using `f64::round`.
 */
function quantize(value: number): number {
  const rounded = Math.round(value * 1000) / 1000;
  return rounded === 0 ? 0 : rounded;
}

export function outlineWidthAt(point: OutlinePoint, options: OutlineOptions): number {
  if (!options.pressure) return options.widthPt;
  const clamped = Math.min(1, Math.max(0, point.pressure));
  return options.widthPt * (MIN_PRESSURE_SCALE + (1 - MIN_PRESSURE_SCALE) * clamped);
}

/** Round mark, with the same winding as a stroke silhouette. */
export function outlineDot(center: OutlinePoint, radius: number): { x: number; y: number }[] {
  return Array.from({ length: 12 }, (_, index) => {
    const angle = (-Math.PI * 2 * index) / 12;
    return {
      x: quantize(center.x + radius * Math.cos(angle)),
      y: quantize(center.y + radius * Math.sin(angle)),
    };
  });
}

/**
 * Resolve filtered samples and their pressure/taper radii for both nib projections.
 */
function outlineSamples(
  points: OutlinePoint[],
  options: OutlineOptions,
): { point: OutlinePoint; radius: number }[] {
  if (points.length === 0) return [];

  // Samples too close together carry no direction and would produce a noisy normal. Exact
  // duplicates were already dropped here; near-duplicates were not, and they are what a slowly
  // drawn stroke is made of.
  const path: OutlinePoint[] = [points[0]];
  for (const point of points.slice(1)) {
    const previous = path[path.length - 1];
    if (span(point, previous) > MIN_SAMPLE_SPACING_PT) path.push(point);
  }

  // A fragment keeps its real geometry, however short: a full-width dot would be fatter than the
  // ink it stands for and could reach back into the area the eraser just cleared.
  if (!options.trim && path.every((point) => span(point, path[0]) <= options.widthPt / 2)) {
    return [{ point: path[0], radius: options.widthPt / 2 }];
  }

  const before = options.trim?.start ?? 0;
  const after = options.trim?.end ?? 0;

  // Two tapered endpoints have zero width; retain a middle section even without a move sample. A
  // cut end keeps its width, so a fragment only needs this when both of its ends are original tips.
  if (path.length === 2 && options.taper > 0 && before === 0 && after === 0) {
    const [first, last] = path;
    path.splice(1, 0, {
      x: (first.x + last.x) / 2,
      y: (first.y + last.y) / 2,
      pressure: (first.pressure + last.pressure) / 2,
    });
  }

  // Arc length drives the taper, so a slow stroke and a fast one taper over the same distance.
  const cumulative: number[] = [0];
  for (let index = 1; index < path.length; index += 1) {
    const dx = path[index].x - path[index - 1].x;
    const dy = path[index].y - path[index - 1].y;
    cumulative.push(cumulative[index - 1] + Math.hypot(dx, dy));
  }
  const total = cumulative[cumulative.length - 1];
  // The taper belongs to the original stroke: a fragment measures it from the original ends.
  const taperLength =
    options.taper > 0
      ? Math.min((before + total + after) * options.taper, options.widthPt * TAPER_MAX_NIB_WIDTHS)
      : 0;

  return path.map((point, index) => {
    const ramp = taperLength > 0
      ? Math.min(1, (before + cumulative[index]) / taperLength,
        (after + total - cumulative[index]) / taperLength)
      : 1;
    return { point, radius: outlineWidthAt(point, options) / 2 * ramp };
  });
}

/**
 * The drawn half-width at every sample, resolved exactly as `outlineSamples` resolves it, but
 * aligned with the input instead of filtered. The precise eraser measures contact with this, so
 * what it removes and what is drawn cannot disagree. Arc length runs over every sample; the
 * renderer's dedup of near-coincident samples shortens it by less than 0.05 pt per dropped one.
 */
export function inkRadii(points: OutlinePoint[], options: OutlineOptions): number[] {
  const before = options.trim?.start ?? 0;
  const after = options.trim?.end ?? 0;
  const cumulative = [0];
  for (let index = 1; index < points.length; index += 1) {
    cumulative.push(cumulative[index - 1] + span(points[index], points[index - 1]));
  }
  const total = cumulative[cumulative.length - 1];
  const taperLength =
    options.taper > 0
      ? Math.min((before + total + after) * options.taper, options.widthPt * TAPER_MAX_NIB_WIDTHS)
      : 0;
  return points.map((point, index) => {
    const ramp = taperLength > 0
      ? Math.min(1, (before + cumulative[index]) / taperLength,
        (after + total - cumulative[index]) / taperLength)
      : 1;
    return outlineWidthAt(point, options) / 2 * ramp;
  });
}

export function outlinePoints(points: OutlinePoint[], options: OutlineOptions): { x: number; y: number }[] {
  const samples = outlineSamples(points, options);
  if (samples.length === 0) return [];
  if (samples.length === 1) return outlineDot(samples[0].point, samples[0].radius);
  const path = samples.map(({ point }) => point);
  const left: { x: number; y: number }[] = [];
  const right: { x: number; y: number }[] = [];

  // Carried so a stretch too short to differentiate reuses the last direction that meant
  // something, rather than snapping to a fixed axis and folding the outline over itself.
  let heading = { x: 1, y: 0 };

  for (let index = 0; index < path.length; index += 1) {
    // Central difference, but taken over a span rather than over the immediate neighbours: two
    // samples a hundredth of a point apart describe jitter, not direction. Well-spaced samples
    // reach the span on their neighbours, so this is the plain central difference wherever the
    // stroke is not crawling.
    let back = index;
    while (back > 0 && span(path[back], path[index]) < MIN_DIRECTION_SPAN_PT) back -= 1;
    let forward = index;
    while (forward + 1 < path.length && span(path[forward], path[index]) < MIN_DIRECTION_SPAN_PT)
      forward += 1;

    let dx = path[forward].x - path[back].x;
    let dy = path[forward].y - path[back].y;
    const length = Math.hypot(dx, dy);
    if (length > 0) {
      dx /= length;
      dy /= length;
      heading = { x: dx, y: dy };
    } else {
      dx = heading.x;
      dy = heading.y;
    }

    const half = samples[index].radius;

    // Normal is the direction rotated a quarter turn.
    const nx = -dy * half;
    const ny = dx * half;
    left.push({ x: quantize(path[index].x + nx), y: quantize(path[index].y + ny) });
    right.push({ x: quantize(path[index].x - nx), y: quantize(path[index].y - ny) });
  }

  return [...left, ...right.reverse()];
}

/** Round pen geometry: consistently wound strips and outside arcs, filled once as one path. */
export function roundedOutlinePoints(points: OutlinePoint[], options: OutlineOptions): { x: number; y: number }[][] {
  const samples = outlineSamples(points, options);
  if (samples.length === 0) return [];
  if (samples.length === 1) return [outlineDot(samples[0].point, samples[0].radius)];
  const polygons: { x: number; y: number }[][] = [];
  const angles: number[] = [];
  const at = (index: number, angle: number) => ({
    x: quantize(samples[index].point.x + samples[index].radius * Math.cos(angle)),
    y: quantize(samples[index].point.y + samples[index].radius * Math.sin(angle)),
  });
  const arc = (index: number, start: number, sweep: number) => {
    if (samples[index].radius <= 0 || sweep <= 0) return;
    const steps = Math.max(1, Math.ceil(sweep / (Math.PI / 6)));
    polygons.push([
      { x: quantize(samples[index].point.x), y: quantize(samples[index].point.y) },
      ...Array.from({ length: steps + 1 }, (_, step) => at(index, start - sweep * step / steps)),
    ]);
  };
  for (let index = 0; index + 1 < samples.length; index += 1) {
    const from = samples[index].point, to = samples[index + 1].point;
    const angle = Math.atan2(to.y - from.y, to.x - from.x);
    angles.push(angle);
    polygons.push([at(index, angle + Math.PI / 2), at(index + 1, angle + Math.PI / 2),
      at(index + 1, angle - Math.PI / 2), at(index, angle - Math.PI / 2)]);
  }
  arc(0, angles[0] - Math.PI / 2, Math.PI);
  arc(samples.length - 1, angles.at(-1)! + Math.PI / 2, Math.PI);
  for (let index = 1; index + 1 < samples.length; index += 1) {
    const before = angles[index - 1], after = angles[index];
    const turn = Math.atan2(Math.sin(after - before), Math.cos(after - before));
    arc(index, turn > 0 ? after - Math.PI / 2 : before + Math.PI / 2, Math.abs(turn));
  }
  return polygons;
}

/** Resolve a pen's maximum taper into the existing per-stroke length fraction. */
export function limitedTaper(points: OutlinePoint[], taper: number, maximumLengthPt: number): number {
  let length = 0;
  for (let index = 1; index < points.length; index += 1) length += span(points[index - 1], points[index]);
  return length > 0 ? Math.min(taper, Math.max(0, maximumLengthPt) / length) : taper;
}

/** The polygon as an SVG/canvas path string. */
export function outlinePath(polygon: { x: number; y: number }[]): string {
  if (polygon.length === 0) return "";
  const [first, ...rest] = polygon;
  return `M ${first.x} ${first.y}${rest.map((point) => ` L ${point.x} ${point.y}`).join("")} Z`;
}
