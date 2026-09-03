import type { Point } from "../geometry/coordinates";
import type { ShapeStyle, StrokePoint } from "../model";
import { straightenStroke } from "../ink/straighten";
import { shapeFromDrag, splineFromPoints, type ShapeDraft } from "./geometry";

export type Recognition = { draft: ShapeDraft; confidence: number };

const MIN_RECOGNITION_SIZE_PT = 24;

/**
 * How near the two ends of a mark have to be for it to count as closed.
 *
 * Whichever is more forgiving: a few points of absolute slack, so a small ring closes on the same
 * "just shy of meeting" gesture a large one does, or a share of the mark's own size, so a broad
 * sweep is not held to a pen-width tolerance. One rule, used by every closed shape — a rectangle,
 * an ellipse and a freehand loop all decide this the same way.
 */
const CLOSED_GAP_PT = 12;
const CLOSED_GAP_RATIO = 0.18;

/// How much of each end of a mark may be the run that closes it — and so how long an overshoot
/// can be and still be recognised as one, rather than read as part of the outline.
const CLOSING_RUN = 0.28;
/// How much of a mark has to remain once its closing tail is trimmed for what is left to be the
/// outline it drew.
const MIN_OUTLINE_SHARE = 0.6;

/**
 * The most a fitted outline may sit away from the samples, as a share of the mark's diagonal.
 *
 * Both fits report this in the same unit — mean distance from a sample to the outline it proposes
 * — so one number governs both and the two can be compared directly. They could not be before:
 * the rectangle scored a distance while the ellipse scored a dimensionless radial residual, which
 * grows with pen noise and shrinks with size, so a small shaky circle always lost to its own
 * bounding box.
 */
const MAX_FIT_ERROR = 0.065;

/// How much curve a held mark may be promoted into.
///
/// An open mark gets a tight budget, because a stray word or scrub is an open mark and a generous
/// one is how it would sneak through. A closed mark can afford far more: handwriting does not
/// return to where it began and then sit still, so the closure test has already turned away the
/// thing this budget exists to catch, and a drawn outline needs the knots.
const MAX_OPEN_SPLINE_NODES = 10;
const MAX_CLOSED_SPLINE_NODES = 32;

/**
 * Where a mark stops reading as a ring and starts reading as a box.
 *
 * Measured rings top out near 1.9 turning concentration however shaky the hand, and boxes bottom
 * out near 2.3 with their corners rounded right off; the split sits in the gap between. The range
 * is how far either side of it the reading stays in doubt, so a mark near the line is decided by
 * how well each outline actually fits rather than by which side of a hard edge it fell.
 */
const CORNER_SPLIT = 2.05;
const CORNER_RANGE = 1.2;
/// Weighed against the fit errors, which typically differ by a couple of hundredths — so a clear
/// reading of the turning outranks a slightly closer outline, and a doubtful one does not.
const SHAPE_WEIGHT = 0.08;

/**
 * Conservative deterministic recognition for the deliberate draw-and-hold gesture.
 * A low-confidence mark remains ink; there is no model, network call, or language-dependent
 * heuristic in the writing path.
 */
export function recognizeHeldShape(
  samples: readonly StrokePoint[],
  style: ShapeStyle,
): Recognition | null {
  const points = deduplicate(samples.map(({ x, y }) => ({ x, y })));
  if (points.length < 3) return null;
  const bounds = pointBounds(points);
  const diagonal = Math.hypot(bounds.width, bounds.height);
  if (diagonal < MIN_RECOGNITION_SIZE_PT) return null;

  const snapped = straightenStroke(samples.slice());
  if (snapped.length === 2) {
    const draft = shapeFromDrag("line", snapped[0], snapped[1], style);
    return draft ? { draft, confidence: 0.98 } : null;
  }

  // What gets fitted is the outline, not the stroke: an overshoot past the start is part of the
  // gesture that closed the shape, not part of the shape.
  const { outline, closed } = closedOutline(points);
  const shape = pointBounds(outline);
  const span = Math.hypot(shape.width, shape.height);
  if (span < MIN_RECOGNITION_SIZE_PT) return null;

  const travel = travelledDistance(outline);
  if (closed && travel / span <= 5) {
    const found = recognizeClosed(outline as Point[], style, span);
    if (found) return found;
  }

  // Holding a simple curve means "clean this curve". A cap on both travel and fitted nodes
  // prevents a word or scrub from being quietly promoted into a vector path. A loop that came
  // back to where it started stays a loop: it is fitted closed, so a drawing that met itself
  // does not come out as an outline with a gap in it.
  if (travel / span <= (closed ? 5 : 2.4)) {
    const draft = splineFromPoints(outline, style, Math.max(1.25, span * 0.012), closed);
    // A mark that came back to where it began is a deliberate outline, so it is allowed a richer
    // curve than an open one, where a generous budget is how a word gets mistaken for a path.
    const budget = closed ? MAX_CLOSED_SPLINE_NODES : MAX_OPEN_SPLINE_NODES;
    if (draft?.geometry.kind === "spline" && draft.geometry.nodes.length <= budget) {
      return { draft, confidence: closed ? 0.74 : 0.78 };
    }
  }
  return null;
}

/**
 * Whether a mark's two ends are near enough to call it closed.
 *
 * Exported because a deliberate drag with the shape tool is judged the same way: ending a
 * freehand curve back where it began closes it there too, not only when the mark was recognised.
 */
export function isClosedStroke(points: readonly Point[]): boolean {
  return closedSpan(points) !== null;
}

/**
 * Where a mark closes on itself, as a range of samples to keep.
 *
 * Ends almost never meet exactly. A hand either stops a little short, or — more often, and more
 * confidently — carries past where it started and leaves a tail lying across the outline. Both are
 * the same gesture and both should read as closed, so this looks for the closest approach between
 * the mark's opening run and its closing one rather than measuring the two endpoints, which an
 * overshoot pushes arbitrarily far apart however neatly the loop was drawn.
 *
 * Returning the range rather than a yes or no matters just as much: the leftover tail is not part
 * of the shape, and left in it would drag the bounding box, the enclosed area and every fit that
 * rests on them.
 */
export function closedSpan(points: readonly Point[]): { from: number; to: number } | null {
  const count = points.length;
  if (count < 6) return null;
  const bounds = pointBounds(points);
  const diagonal = Math.hypot(bounds.width, bounds.height);
  if (diagonal <= 0) return null;
  const tolerance = Math.max(CLOSED_GAP_PT, CLOSED_GAP_RATIO * diagonal);

  // Only the two ends can close a mark, so only they are searched — and coarsely, because the
  // samples near a crossing are all within a pen width of each other anyway.
  const reach = Math.max(1, Math.floor(count * CLOSING_RUN));
  const step = Math.max(1, Math.floor(reach / 24));
  let best: { from: number; to: number; gap: number } | null = null;
  for (let from = 0; from <= reach; from += step) {
    for (let to = count - 1; to >= count - 1 - reach; to -= step) {
      // What is trimmed has to be a tail. Requiring most of the mark to survive is what keeps a
      // spiral out: its second lap passes close to its first, so a laxer rule would happily cut
      // one away and fit a tidy ring to what is left, which is not what was drawn.
      if (to - from < count * MIN_OUTLINE_SHARE) continue;
      const gap = distance(points[from], points[to]);
      if (gap <= tolerance && (!best || gap < best.gap)) best = { from, to, gap };
    }
  }
  return best && { from: best.from, to: best.to };
}

/**
 * The part of a mark that is its outline, and whether that outline closes.
 *
 * One place decides this, so a held mark and a deliberate drag with the shape tool agree on both
 * what was drawn and where it ended.
 */
export function closedOutline(points: readonly Point[]): {
  outline: readonly Point[];
  closed: boolean;
} {
  const span = closedSpan(points);
  if (!span) return { outline: points, closed: false };
  const outline = points.slice(span.from, span.to + 1);
  return outline.length >= 3 ? { outline, closed: true } : { outline: points, closed: false };
}

/**
 * Decide between a box and a ring, having already decided the mark is closed.
 *
 * Both fits are scored the same way — mean distance from a sample to the outline being proposed,
 * over the mark's diagonal — so the two numbers mean the same thing and a single tolerance admits
 * or rejects either. What separates them is not that score but where the turning happens: a
 * residual cannot tell a wobbly ring from a ring that is really a box, and neither can area, since
 * a hand rounds the corners off a rectangle until it fills no more of its bounding box than an
 * ellipse would. Turning survives both, because rounding a corner shortens the run it turns
 * through without spreading the turn out along the sides.
 */
/**
 * How concentrated a mark's turning is: 1 when it turns at a steady rate, and up to a quarter of
 * `steps` when it does all of its turning in four places.
 *
 * This is what separates a box from a ring, and area is not. Rounding the corners off a rectangle
 * — which every hand does — takes area out of its bounding box until it fills no more of it than
 * an ellipse would, but it does not spread the turning out: the sides stay straight and the
 * corners keep their ninety degrees, just over a shorter run. Walking equal steps of arc length
 * rather than equal samples keeps the pen's speed out of it, and squaring rewards the same total
 * turn for being bunched up rather than spread.
 */
export function turningConcentration(points: readonly Point[], steps = 48): number {
  const walk = resampleClosed(points, steps);
  if (!walk) return 1;
  let squared = 0;
  let total = 0;
  for (let index = 0; index < walk.length; index += 1) {
    const previous = walk[(index - 1 + walk.length) % walk.length];
    const point = walk[index];
    const next = walk[(index + 1) % walk.length];
    const before = Math.atan2(point.y - previous.y, point.x - previous.x);
    const after = Math.atan2(next.y - point.y, next.x - point.x);
    let turn = after - before;
    while (turn > Math.PI) turn -= 2 * Math.PI;
    while (turn < -Math.PI) turn += 2 * Math.PI;
    squared += turn * turn;
    total += Math.abs(turn);
  }
  return total > 1e-6 ? (squared * walk.length) / (total * total) : 1;
}

/// Equal steps of arc length around a closed outline, so turning is measured per unit travelled.
function resampleClosed(points: readonly Point[], steps: number): Point[] | null {
  const loop = [...points, points[0]];
  const spans: number[] = [];
  let perimeter = 0;
  for (let index = 1; index < loop.length; index += 1) {
    perimeter += distance(loop[index - 1], loop[index]);
    spans.push(perimeter);
  }
  if (perimeter <= 0) return null;
  const walk: Point[] = [];
  let at = 0;
  for (let step = 0; step < steps; step += 1) {
    const target = (step / steps) * perimeter;
    while (at < spans.length - 1 && spans[at] < target) at += 1;
    const before = at === 0 ? 0 : spans[at - 1];
    const along = spans[at] > before ? (target - before) / (spans[at] - before) : 0;
    walk.push({
      x: loop[at].x + (loop[at + 1].x - loop[at].x) * along,
      y: loop[at].y + (loop[at + 1].y - loop[at].y) * along,
    });
  }
  return walk;
}

function recognizeClosed(
  points: Point[],
  style: ShapeStyle,
  diagonal: number,
): Recognition | null {
  // 0 for a mark that turns at a steady rate, 1 for one that turns in four places.
  const boxiness = Math.min(
    Math.max((turningConcentration(points) - CORNER_SPLIT) / CORNER_RANGE + 0.5, 0),
    1,
  );
  const candidates = [
    { kind: "rectangle" as const, fit: bestRectangle(points, diagonal), penalty: 1 - boxiness },
    { kind: "ellipse" as const, fit: bestEllipse(points, diagonal), penalty: boxiness },
  ]
    .flatMap(({ kind, fit, penalty }) =>
      fit && fit.error <= MAX_FIT_ERROR
        ? [{ kind, fit, score: fit.error + SHAPE_WEIGHT * penalty }]
        : [],
    )
    .sort((left, right) => left.score - right.score);

  const best = candidates[0];
  if (!best) return null;
  const { fit } = best;
  return {
    confidence: confidence(best.score),
    draft: {
      x: fit.origin.x,
      y: fit.origin.y,
      rotation: fit.angleDegrees,
      geometry:
        best.kind === "rectangle"
          ? {
              kind: "rectangle" as const,
              widthPt: fit.width,
              heightPt: fit.height,
              cornerRadiusPt: Math.min(4, fit.width / 2, fit.height / 2),
            }
          : { kind: "ellipse" as const, widthPt: fit.width, heightPt: fit.height },
      style,
    },
  };
}

type OrientedFit = {
  origin: Point;
  width: number;
  height: number;
  angleDegrees: number;
  /// Mean distance from a sample to the proposed outline, over the mark's diagonal.
  error: number;
};

function bestRectangle(points: Point[], diagonal: number): OrientedFit | null {
  const step = Math.max(1, Math.floor(points.length / 24));
  const sampled = points.filter((_, index) => index % step === 0);
  // The directions the mark actually travels in are the best guesses at which way its sides run,
  // but a wobbly side points a little differently everywhere along it, so on a large shaky box
  // none of them is quite square to it. The sweep is the floor under that: it costs one more pass
  // and means the fit never depends on having caught a clean edge.
  const angles = sampled.slice(1).flatMap((point, index) => {
    const previous = sampled[index];
    return distance(previous, point) > 3 ? [Math.atan2(point.y - previous.y, point.x - previous.x)] : [];
  });
  angles.push(principalAngle(points));
  for (let index = 0; index < 12; index += 1) angles.push((index * Math.PI) / 24);
  let best: OrientedFit | null = null;
  for (const angle of angles) {
    const fit = orientedBounds(points, angle);
    if (fit.width < 8 || fit.height < 8) continue;
    const error =
      fit.rotated.reduce(
        (sum, point) =>
          sum +
          Math.min(
            Math.abs(point.x - fit.left),
            Math.abs(point.x - fit.right),
            Math.abs(point.y - fit.top),
            Math.abs(point.y - fit.bottom),
          ),
        0,
      ) /
      points.length /
      diagonal;
    const candidate = toFit(fit, angle, error);
    if (!best || candidate.error < best.error) best = candidate;
  }
  return best;
}

/**
 * The principal axis of the samples, which is the orientation a ring is most likely drawn at.
 *
 * Meaningless on its own for anything near circular or square, where the two axes carry the same
 * variance and the angle this returns is whatever the noise happened to favour — which is why it
 * is one candidate among a sweep rather than the answer.
 */
function principalAngle(points: Point[]): number {
  const centre = average(points);
  const covariance = points.reduce(
    (sum, point) => {
      const x = point.x - centre.x;
      const y = point.y - centre.y;
      return { xx: sum.xx + x * x, xy: sum.xy + x * y, yy: sum.yy + y * y };
    },
    { xx: 0, xy: 0, yy: 0 },
  );
  return 0.5 * Math.atan2(2 * covariance.xy, covariance.xx - covariance.yy);
}

function bestEllipse(points: Point[], diagonal: number): OrientedFit | null {
  // A quarter turn covers every distinct orientation of an axis-aligned box, and the principal
  // axis joins the sweep for the well-conditioned case of a genuinely elongated ring. Sweeping
  // matters most for the shape it looks least necessary on: a hand-drawn square has no principal
  // axis to speak of, so trusting one would hand the ellipse an arbitrarily tilted bounding box,
  // far roomier than the square itself, and let it claim a fill only a real ring should have.
  const angles = [principalAngle(points)];
  for (let step = 0; step < 12; step += 1) angles.push((step * Math.PI) / 24);

  let best: OrientedFit | null = null;
  for (const angle of angles) {
    const fit = orientedBounds(points, angle);
    const rx = fit.width / 2;
    const ry = fit.height / 2;
    if (rx < 4 || ry < 4) continue;
    const cx = (fit.left + fit.right) / 2;
    const cy = (fit.top + fit.bottom) / 2;
    const error =
      fit.rotated.reduce((sum, point) => {
        const dx = (point.x - cx) / rx;
        const dy = (point.y - cy) / ry;
        const radius = Math.hypot(dx, dy);
        // One Newton step from the normalised residual back to a distance on the page, so this
        // number is the same kind of thing the rectangle reports and the two can be compared.
        const gradient = Math.hypot(dx / rx, dy / ry);
        return sum + (gradient > 1e-9 ? Math.abs(radius - 1) / gradient : Math.min(rx, ry));
      }, 0) /
      points.length /
      diagonal;
    const candidate = toFit(fit, angle, error);
    if (!best || candidate.error < best.error) best = candidate;
  }
  return best;
}

function orientedBounds(points: Point[], angle: number) {
  const rotated = points.map((point) => rotate(point, -angle));
  const bounds = pointBounds(rotated);
  return {
    rotated,
    left: bounds.left,
    top: bounds.top,
    right: bounds.right,
    bottom: bounds.bottom,
    width: bounds.width,
    height: bounds.height,
  };
}

function toFit(
  fit: ReturnType<typeof orientedBounds>,
  angle: number,
  error: number,
): OrientedFit {
  return {
    origin: rotate({ x: fit.left, y: fit.top }, angle),
    width: fit.width,
    height: fit.height,
    angleDegrees: (angle * 180) / Math.PI,
    error,
  };
}

function confidence(score: number): number {
  return Math.max(0.7, Math.min(0.96, 1 - score / (MAX_FIT_ERROR + SHAPE_WEIGHT)));
}

function pointBounds(points: readonly Point[]) {
  const left = Math.min(...points.map((point) => point.x));
  const right = Math.max(...points.map((point) => point.x));
  const top = Math.min(...points.map((point) => point.y));
  const bottom = Math.max(...points.map((point) => point.y));
  return { left, right, top, bottom, width: right - left, height: bottom - top };
}

function average(points: readonly Point[]): Point {
  const total = points.reduce((sum, point) => ({ x: sum.x + point.x, y: sum.y + point.y }), { x: 0, y: 0 });
  return { x: total.x / points.length, y: total.y / points.length };
}

function rotate(point: Point, angle: number): Point {
  const cosine = Math.cos(angle);
  const sine = Math.sin(angle);
  return { x: point.x * cosine - point.y * sine, y: point.x * sine + point.y * cosine };
}

function travelledDistance(points: readonly Point[]): number {
  return points.slice(1).reduce((sum, point, index) => sum + distance(points[index], point), 0);
}

function deduplicate(points: Point[]): Point[] {
  return points.filter(
    (point, index) => index === 0 || point.x !== points[index - 1].x || point.y !== points[index - 1].y,
  );
}

function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}
