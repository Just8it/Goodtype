import type { Point } from "../geometry/coordinates";
import type { ShapeStyle, StrokePoint } from "../model";
import { straightenStroke } from "../ink/straighten";
import { shapeFromDrag, splineFromPoints, type ShapeDraft } from "./geometry";

export type Recognition = { draft: ShapeDraft; confidence: number };

const MIN_RECOGNITION_SIZE_PT = 24;

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

  const travel = travelledDistance(points);
  const closure = distance(points[0], points[points.length - 1]) / diagonal;
  if (closure <= 0.18 && travel / diagonal <= 5) {
    const closed = recognizeClosed(points, style, closure);
    if (closed) return closed;
  }

  // Holding a simple open curve means "clean this curve". A cap on both travel and fitted
  // nodes prevents a word or scrub from being quietly promoted into a vector path.
  if (travel / diagonal <= 2.4) {
    const draft = splineFromPoints(points, style, Math.max(1.25, diagonal * 0.012));
    if (draft?.geometry.kind === "spline" && draft.geometry.nodes.length <= 10) {
      return { draft, confidence: 0.78 };
    }
  }
  return null;
}

function recognizeClosed(
  points: Point[],
  style: ShapeStyle,
  closure: number,
): Recognition | null {
  const rectangle = bestRectangle(points);
  const ellipse = bestEllipse(points);
  const candidates: (Recognition | null)[] = [
    rectangle && rectangle.score < 0.115
      ? {
          confidence: confidence(rectangle.score + closure * 0.12, 0.16),
          draft: {
            x: rectangle.origin.x,
            y: rectangle.origin.y,
            rotation: rectangle.angleDegrees,
            geometry: {
              kind: "rectangle" as const,
              widthPt: rectangle.width,
              heightPt: rectangle.height,
              cornerRadiusPt: Math.min(4, rectangle.width / 2, rectangle.height / 2),
            },
            style,
          },
        }
      : null,
    ellipse && ellipse.score < 0.14
      ? {
          confidence: confidence(ellipse.score + closure * 0.12, 0.19),
          draft: {
            x: ellipse.origin.x,
            y: ellipse.origin.y,
            rotation: ellipse.angleDegrees,
            geometry: {
              kind: "ellipse" as const,
              widthPt: ellipse.width,
              heightPt: ellipse.height,
            },
            style,
          },
        }
      : null,
  ];
  const recognized = candidates.filter((candidate): candidate is Recognition => candidate !== null);
  return recognized.sort((left, right) => right.confidence - left.confidence)[0] ?? null;
}

type OrientedFit = {
  origin: Point;
  width: number;
  height: number;
  angleDegrees: number;
  score: number;
};

function bestRectangle(points: Point[]): OrientedFit | null {
  const sampled = points.filter((_, index) => index % Math.max(1, Math.floor(points.length / 24)) === 0);
  const angles = sampled.slice(1).flatMap((point, index) => {
    const previous = sampled[index];
    return distance(previous, point) > 3 ? [Math.atan2(point.y - previous.y, point.x - previous.x)] : [];
  });
  let best: OrientedFit | null = null;
  for (const angle of angles) {
    const fit = orientedBounds(points, angle);
    const diagonal = Math.hypot(fit.width, fit.height);
    if (fit.width < 8 || fit.height < 8 || diagonal === 0) continue;
    const error = fit.rotated.reduce((sum, point) => {
      const edge = Math.min(
        Math.abs(point.x - fit.left),
        Math.abs(point.x - fit.right),
        Math.abs(point.y - fit.top),
        Math.abs(point.y - fit.bottom),
      );
      return sum + edge / diagonal;
    }, 0) / points.length;
    const candidate = toFit(fit, angle, error);
    if (!best || candidate.score < best.score) best = candidate;
  }
  return best;
}

function bestEllipse(points: Point[]): OrientedFit | null {
  const center = average(points);
  const covariance = points.reduce(
    (sum, point) => {
      const x = point.x - center.x;
      const y = point.y - center.y;
      return { xx: sum.xx + x * x, xy: sum.xy + x * y, yy: sum.yy + y * y };
    },
    { xx: 0, xy: 0, yy: 0 },
  );
  const angle = 0.5 * Math.atan2(2 * covariance.xy, covariance.xx - covariance.yy);
  const fit = orientedBounds(points, angle);
  const rx = fit.width / 2;
  const ry = fit.height / 2;
  if (rx < 4 || ry < 4) return null;
  const cx = (fit.left + fit.right) / 2;
  const cy = (fit.top + fit.bottom) / 2;
  const error = fit.rotated.reduce((sum, point) => {
    const radius = Math.hypot((point.x - cx) / rx, (point.y - cy) / ry);
    return sum + Math.abs(radius - 1);
  }, 0) / points.length;
  return toFit(fit, angle, error);
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
  score: number,
): OrientedFit {
  return {
    origin: rotate({ x: fit.left, y: fit.top }, angle),
    width: fit.width,
    height: fit.height,
    angleDegrees: (angle * 180) / Math.PI,
    score,
  };
}

function confidence(error: number, limit: number): number {
  return Math.max(0.7, Math.min(0.96, 1 - error / limit));
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
