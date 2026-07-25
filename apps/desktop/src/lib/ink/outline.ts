// Variable-width stroke geometry.
//
// A stroke is not a stroked centreline — `stroke-width` is constant along a path, so a stroked
// polyline can never vary with pressure. Instead the ink's actual silhouette is computed: offset
// the centreline by half the local width on each side and close the two sides into one polygon.
//
// That one change fixes three things at once:
//   * pressure survives into the PDF, because export fills the same polygon the canvas does;
//   * no beading, because the stroke is one shape rather than many round-capped segments;
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
};

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
  return Math.round(value * 1000) / 1000;
}

function widthAt(point: OutlinePoint, options: OutlineOptions): number {
  if (!options.pressure) return options.widthPt;
  const clamped = Math.min(1, Math.max(0, point.pressure));
  return options.widthPt * (MIN_PRESSURE_SCALE + (1 - MIN_PRESSURE_SCALE) * clamped);
}

/**
 * The closed silhouette of a stroke, as a polygon. Returns an empty array when there is nothing
 * to draw; a single point becomes a small square dot so a tap still leaves a mark.
 */
export function outlinePoints(
  points: OutlinePoint[],
  options: OutlineOptions,
): { x: number; y: number }[] {
  if (points.length === 0) return [];

  // Samples too close together carry no direction and would produce a noisy normal. Exact
  // duplicates were already dropped here; near-duplicates were not, and they are what a slowly
  // drawn stroke is made of.
  const path: OutlinePoint[] = [points[0]];
  for (const point of points.slice(1)) {
    const previous = path[path.length - 1];
    if (span(point, previous) > MIN_SAMPLE_SPACING_PT) path.push(point);
  }

  if (path.length === 1) {
    const half = widthAt(path[0], options) / 2;
    const { x, y } = path[0];
    return [
      { x: quantize(x - half), y: quantize(y - half) },
      { x: quantize(x + half), y: quantize(y - half) },
      { x: quantize(x + half), y: quantize(y + half) },
      { x: quantize(x - half), y: quantize(y + half) },
    ];
  }

  // Arc length drives the taper, so a slow stroke and a fast one taper over the same distance.
  const cumulative: number[] = [0];
  for (let index = 1; index < path.length; index += 1) {
    const dx = path[index].x - path[index - 1].x;
    const dy = path[index].y - path[index - 1].y;
    cumulative.push(cumulative[index - 1] + Math.hypot(dx, dy));
  }
  const total = cumulative[cumulative.length - 1];
  const taperLength =
    options.taper > 0
      ? Math.min(total * options.taper, options.widthPt * TAPER_MAX_NIB_WIDTHS)
      : 0;

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

    let half = widthAt(path[index], options) / 2;
    if (taperLength > 0) {
      const fromStart = cumulative[index];
      const fromEnd = total - cumulative[index];
      const ramp = Math.min(1, fromStart / taperLength, fromEnd / taperLength);
      half *= ramp;
    }

    // Normal is the direction rotated a quarter turn.
    const nx = -dy * half;
    const ny = dx * half;
    left.push({ x: quantize(path[index].x + nx), y: quantize(path[index].y + ny) });
    right.push({ x: quantize(path[index].x - nx), y: quantize(path[index].y - ny) });
  }

  return [...left, ...right.reverse()];
}

/** The polygon as an SVG/canvas path string. */
export function outlinePath(polygon: { x: number; y: number }[]): string {
  if (polygon.length === 0) return "";
  const [first, ...rest] = polygon;
  return `M ${first.x} ${first.y}${rest.map((point) => ` L ${point.x} ${point.y}`).join("")} Z`;
}
