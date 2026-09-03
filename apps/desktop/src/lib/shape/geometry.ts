import type { Point } from "../geometry/coordinates";
import type {
  BezierNode,
  ShapeGeometry,
  ShapePoint,
  ShapeStyle,
  StrokePoint,
} from "../model";

export type ShapeKind = ShapeGeometry["kind"];

export type ShapeDraft = {
  x: number;
  y: number;
  rotation: number;
  geometry: ShapeGeometry;
  style: ShapeStyle;
};

export type ShapeBounds = { left: number; top: number; right: number; bottom: number };

export const MIN_SHAPE_SIZE_PT = 4;
const MAX_SPLINE_NODES = 256;

export function shapePath(geometry: ShapeGeometry): string {
  switch (geometry.kind) {
    case "line":
      return `M ${number(geometry.start.x)} ${number(geometry.start.y)} L ${number(geometry.end.x)} ${number(geometry.end.y)}`;
    case "rectangle": {
      const radius = Math.min(
        Math.max(geometry.cornerRadiusPt, 0),
        geometry.widthPt / 2,
        geometry.heightPt / 2,
      );
      if (radius === 0) return `M 0 0 H ${number(geometry.widthPt)} V ${number(geometry.heightPt)} H 0 Z`;
      return [
        `M ${number(radius)} 0`,
        `H ${number(geometry.widthPt - radius)}`,
        `Q ${number(geometry.widthPt)} 0 ${number(geometry.widthPt)} ${number(radius)}`,
        `V ${number(geometry.heightPt - radius)}`,
        `Q ${number(geometry.widthPt)} ${number(geometry.heightPt)} ${number(geometry.widthPt - radius)} ${number(geometry.heightPt)}`,
        `H ${number(radius)}`,
        `Q 0 ${number(geometry.heightPt)} 0 ${number(geometry.heightPt - radius)}`,
        `V ${number(radius)}`,
        `Q 0 0 ${number(radius)} 0 Z`,
      ].join(" ");
    }
    case "ellipse": {
      const rx = geometry.widthPt / 2;
      const ry = geometry.heightPt / 2;
      return [
        `M ${number(geometry.widthPt)} ${number(ry)}`,
        `A ${number(rx)} ${number(ry)} 0 1 0 0 ${number(ry)}`,
        `A ${number(rx)} ${number(ry)} 0 1 0 ${number(geometry.widthPt)} ${number(ry)} Z`,
      ].join(" ");
    }
    case "spline":
      return splinePath(geometry.nodes, geometry.closed);
  }
}

export function shapeBounds(geometry: ShapeGeometry): ShapeBounds {
  if (geometry.kind === "rectangle" || geometry.kind === "ellipse") {
    return { left: 0, top: 0, right: geometry.widthPt, bottom: geometry.heightPt };
  }
  if (geometry.kind === "line") return pointsBounds([geometry.start, geometry.end]);
  const points = geometry.nodes.flatMap((node) => [
    node.point,
    ...(node.handleIn ? [add(node.point, node.handleIn)] : []),
    ...(node.handleOut ? [add(node.point, node.handleOut)] : []),
  ]);
  return pointsBounds(points);
}

export function shapeFromDrag(
  kind: Exclude<ShapeKind, "spline">,
  start: Point,
  pointer: Point,
  style: ShapeStyle,
  constrain = false,
): ShapeDraft | null {
  if (kind === "line") {
    const end = constrain ? constrainedLineEnd(start, pointer) : pointer;
    if (distance(start, end) < MIN_SHAPE_SIZE_PT) return null;
    return {
      x: start.x,
      y: start.y,
      rotation: 0,
      geometry: {
        kind: "line",
        start: { x: 0, y: 0 },
        end: { x: end.x - start.x, y: end.y - start.y },
      },
      style,
    };
  }
  const end = constrain ? constrainedCorner(start, pointer) : pointer;
  const widthPt = Math.abs(end.x - start.x);
  const heightPt = Math.abs(end.y - start.y);
  if (widthPt < MIN_SHAPE_SIZE_PT || heightPt < MIN_SHAPE_SIZE_PT) return null;
  const box = {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    widthPt,
    heightPt,
  };
  return {
    x: box.x,
    y: box.y,
    rotation: 0,
    geometry:
      kind === "rectangle"
        ? { kind, widthPt, heightPt, cornerRadiusPt: Math.min(4, widthPt / 2, heightPt / 2) }
        : { kind, widthPt, heightPt },
    style,
  };
}

/**
 * Reduce a sampled curve, then derive smooth cubic handles with the Catmull-Rom tangent rule.
 * The fitted geometry is deterministic and bounded; raw input samples remain ink, never hidden
 * inside a shape object.
 */
export function splineFromPoints(
  input: readonly Point[],
  style: ShapeStyle,
  tolerancePt = 1.5,
): ShapeDraft | null {
  const sampled = deduplicate(input);
  if (sampled.length < 2 || travelledDistance(sampled) < MIN_SHAPE_SIZE_PT) return null;
  const simplified = evenlyCap(simplify(sampled, Math.max(tolerancePt, 0.25)), MAX_SPLINE_NODES);
  const bounds = pointsBounds(simplified);
  const local = simplified.map((point) => ({ x: point.x - bounds.left, y: point.y - bounds.top }));
  const nodes = local.map((point, index): BezierNode => {
    const previous = local[Math.max(index - 1, 0)];
    const next = local[Math.min(index + 1, local.length - 1)];
    const tangent = { x: (next.x - previous.x) / 6, y: (next.y - previous.y) / 6 };
    return {
      point,
      handleIn: index === 0 ? null : { x: -tangent.x, y: -tangent.y },
      handleOut: index === local.length - 1 ? null : tangent,
    };
  });
  return {
    x: bounds.left,
    y: bounds.top,
    rotation: 0,
    geometry: { kind: "spline", nodes, closed: false },
    style,
  };
}

export function strokePoints(points: readonly StrokePoint[]): Point[] {
  return points.map(({ x, y }) => ({ x, y }));
}

export function moveShapeGeometry(geometry: ShapeGeometry, delta: Point): ShapeGeometry {
  if (geometry.kind === "line") {
    return {
      ...geometry,
      start: add(geometry.start, delta),
      end: add(geometry.end, delta),
    };
  }
  if (geometry.kind === "spline") {
    return {
      ...geometry,
      nodes: geometry.nodes.map((node) => ({ ...node, point: add(node.point, delta) })),
    };
  }
  return geometry;
}

export function describeShape(geometry: ShapeGeometry): string {
  switch (geometry.kind) {
    case "line":
      return `Line, ${Math.round(distance(geometry.start, geometry.end))} points long`;
    case "rectangle":
      return `Rectangle, ${Math.round(geometry.widthPt)} by ${Math.round(geometry.heightPt)} points`;
    case "ellipse":
      return `${Math.abs(geometry.widthPt - geometry.heightPt) < 0.5 ? "Circle" : "Ellipse"}, ${Math.round(geometry.widthPt)} by ${Math.round(geometry.heightPt)} points`;
    case "spline":
      return `Spline with ${geometry.nodes.length} anchor points`;
  }
}

function splinePath(nodes: BezierNode[], closed: boolean): string {
  if (nodes.length === 0) return "";
  const commands = [`M ${number(nodes[0].point.x)} ${number(nodes[0].point.y)}`];
  for (let index = 1; index < nodes.length; index += 1) {
    appendSegment(commands, nodes[index - 1], nodes[index]);
  }
  if (closed && nodes.length > 2) {
    appendSegment(commands, nodes[nodes.length - 1], nodes[0]);
    commands.push("Z");
  }
  return commands.join(" ");
}

function appendSegment(commands: string[], from: BezierNode, to: BezierNode): void {
  const control1 = add(from.point, from.handleOut ?? { x: 0, y: 0 });
  const control2 = add(to.point, to.handleIn ?? { x: 0, y: 0 });
  commands.push(
    `C ${number(control1.x)} ${number(control1.y)} ${number(control2.x)} ${number(control2.y)} ${number(to.point.x)} ${number(to.point.y)}`,
  );
}

/**
 * Ramer–Douglas–Peucker, walked with an explicit stack over index ranges.
 *
 * The recursive form copies a slice at every level, so a long freehand drag — the one input that
 * reliably produces thousands of samples — costs O(n²) in copying and puts the recursion depth at
 * the mercy of how wiggly the stroke was. This form allocates one flag array and never slices.
 */
function simplify(points: Point[], tolerance: number): Point[] {
  if (points.length <= 2) return points;
  const keep = new Uint8Array(points.length);
  keep[0] = 1;
  keep[points.length - 1] = 1;
  const pending: number[] = [0, points.length - 1];
  while (pending.length > 0) {
    const last = pending.pop()!;
    const first = pending.pop()!;
    let farthest = tolerance;
    let index = -1;
    for (let current = first + 1; current < last; current += 1) {
      const gap = distanceToSegment(points[current], points[first], points[last]);
      if (gap > farthest) {
        farthest = gap;
        index = current;
      }
    }
    if (index === -1) continue;
    keep[index] = 1;
    pending.push(first, index, index, last);
  }
  return points.filter((_, index) => keep[index] === 1);
}

function evenlyCap(points: Point[], maximum: number): Point[] {
  if (points.length <= maximum) return points;
  return Array.from({ length: maximum }, (_, index) =>
    points[Math.round((index * (points.length - 1)) / (maximum - 1))],
  );
}

function deduplicate(points: readonly Point[]): Point[] {
  return points.filter(
    (point, index) => index === 0 || point.x !== points[index - 1].x || point.y !== points[index - 1].y,
  );
}

function pointsBounds(points: readonly ShapePoint[]): ShapeBounds {
  return {
    left: Math.min(...points.map((point) => point.x)),
    top: Math.min(...points.map((point) => point.y)),
    right: Math.max(...points.map((point) => point.x)),
    bottom: Math.max(...points.map((point) => point.y)),
  };
}

function constrainedLineEnd(start: Point, end: Point): Point {
  const length = distance(start, end);
  const step = Math.PI / 12;
  const angle = Math.round(Math.atan2(end.y - start.y, end.x - start.x) / step) * step;
  return { x: start.x + Math.cos(angle) * length, y: start.y + Math.sin(angle) * length };
}

function constrainedCorner(start: Point, end: Point): Point {
  const size = Math.max(Math.abs(end.x - start.x), Math.abs(end.y - start.y));
  return {
    x: start.x + Math.sign(end.x - start.x || 1) * size,
    y: start.y + Math.sign(end.y - start.y || 1) * size,
  };
}

function travelledDistance(points: readonly Point[]): number {
  return points.slice(1).reduce((sum, point, index) => sum + distance(points[index], point), 0);
}

function distanceToSegment(point: Point, start: Point, end: Point): number {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  if (dx === 0 && dy === 0) return distance(point, start);
  const amount = Math.min(
    Math.max(((point.x - start.x) * dx + (point.y - start.y) * dy) / (dx * dx + dy * dy), 0),
    1,
  );
  return distance(point, { x: start.x + amount * dx, y: start.y + amount * dy });
}

function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function add(a: ShapePoint, b: ShapePoint): ShapePoint {
  return { x: a.x + b.x, y: a.y + b.y };
}

function number(value: number): string {
  return String(Math.round(value * 1000) / 1000);
}
