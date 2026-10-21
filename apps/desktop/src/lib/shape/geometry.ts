import type { Point } from "../geometry/coordinates";
import type {
  BezierNode,
  ShapeGeometry,
  ShapePoint,
  ShapeStyle,
  StrokePattern,
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

/**
 * Dash array for a shape outline, in the shape's own units.
 *
 * Spacing matches patterned ink (`ink/pattern.ts`): dashes of max(4w, 6), gaps of max(2.5w, 3.5)
 * and dots every max(2.75w, 4). Shapes are stroked with round caps that add half a width at each
 * end, so the array shortens each dash by one width to keep the visible lengths equal. The export
 * in `crates/goodtype-typst/src/export.rs` mirrors this.
 */
export function shapeDashArray(style: ShapeStyle): number[] | null {
  return patternDashArray(style.pattern ?? "solid", style.strokeWidthPt, style.patternSpacing ?? 1);
}

/**
 * The same spacing for any round-capped line of `width`, in that line's units. Previews draw at a
 * magnified width, so they pass that width rather than the nib's: a fixed array let fat dots run
 * together into a solid bead.
 */
export function patternDashArray(
  pattern: StrokePattern,
  width: number,
  spacing = 1,
): number[] | null {
  if (pattern === "dashed") {
    return [
      Math.max(Math.max(width * 4, 6) - width, 0.01),
      Math.max(width * 2.5, 3.5) * spacing + width,
    ];
  }
  if (pattern === "dotted") return [0.01, Math.max(width * 2.75, 4) * spacing];
  return null;
}
/// The store will not accept a closed curve with fewer knots than this, and it would not enclose
/// anything anyway.
export const MIN_CLOSED_SPLINE_NODES = 3;
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

/**
 * How far a point in the shape's own coordinates lies from its outline.
 *
 * The outline, not the region: an unfilled shape is only the line that was drawn, and reaching
 * inside a large empty rectangle should no more touch it than reaching outside one does.
 */
export function distanceToOutline(geometry: ShapeGeometry, point: ShapePoint): number {
  switch (geometry.kind) {
    case "line":
      return distanceToSegment(point, geometry.start, geometry.end);
    case "rectangle":
      return Math.abs(signedBoxDistance(geometry.widthPt, geometry.heightPt, point));
    case "ellipse":
      return distanceToEllipse(
        point,
        { x: geometry.widthPt / 2, y: geometry.heightPt / 2 },
        geometry.widthPt / 2,
        geometry.heightPt / 2,
      );
    case "spline": {
      const walk = splinePolyline(geometry.nodes, geometry.closed);
      let nearest = Number.POSITIVE_INFINITY;
      for (let index = 1; index < walk.length; index += 1) {
        nearest = Math.min(nearest, distanceToSegment(point, walk[index - 1], walk[index]));
      }
      return walk.length === 1 ? distance(point, walk[0]) : nearest;
    }
  }
}

/**
 * Whether a point in the shape's own coordinates falls inside it.
 *
 * Only meaningful for a shape that encloses something, and only consulted for one that is filled —
 * a tint is part of what was drawn, so touching it counts as touching the shape.
 */
export function containsPoint(geometry: ShapeGeometry, point: ShapePoint): boolean {
  switch (geometry.kind) {
    case "line":
      return false;
    case "rectangle":
      return signedBoxDistance(geometry.widthPt, geometry.heightPt, point) <= 0;
    case "ellipse": {
      const rx = geometry.widthPt / 2;
      const ry = geometry.heightPt / 2;
      return rx > 0 && ry > 0 && Math.hypot((point.x - rx) / rx, (point.y - ry) / ry) <= 1;
    }
    case "spline":
      return geometry.closed && windsAround(splinePolyline(geometry.nodes, true), point);
  }
}

/**
 * Distance from a point to an ellipse's outline, by one Newton step from the normalised residual.
 *
 * Exact on a circle and close enough on anything a hand draws, which is what lets a residual in
 * these coordinates be compared against a distance measured anywhere else.
 */
export function distanceToEllipse(
  point: ShapePoint,
  centre: ShapePoint,
  rx: number,
  ry: number,
): number {
  if (rx <= 0 || ry <= 0) return distance(point, centre);
  const dx = (point.x - centre.x) / rx;
  const dy = (point.y - centre.y) / ry;
  const radius = Math.hypot(dx, dy);
  const gradient = Math.hypot(dx / rx, dy / ry);
  return gradient > 1e-9 ? Math.abs(radius - 1) / gradient : Math.min(rx, ry);
}

/// A spline flattened into straight segments, for measuring against rather than drawing.
export function splinePolyline(nodes: BezierNode[], closed: boolean, perSegment = 8): ShapePoint[] {
  if (nodes.length === 0) return [];
  if (nodes.length === 1) return [nodes[0].point];
  const walk: ShapePoint[] = [nodes[0].point];
  const pairs = closed && nodes.length > 2 ? nodes.length : nodes.length - 1;
  for (let index = 0; index < pairs; index += 1) {
    const from = nodes[index];
    const to = nodes[(index + 1) % nodes.length];
    const control1 = add(from.point, from.handleOut ?? { x: 0, y: 0 });
    const control2 = add(to.point, to.handleIn ?? { x: 0, y: 0 });
    for (let step = 1; step <= perSegment; step += 1) {
      walk.push(cubicAt(from.point, control1, control2, to.point, step / perSegment));
    }
  }
  return walk;
}

/// Negative inside the box, positive outside, and the magnitude is the distance to its edge.
function signedBoxDistance(width: number, height: number, point: ShapePoint): number {
  const halfWidth = width / 2;
  const halfHeight = height / 2;
  const x = Math.abs(point.x - halfWidth) - halfWidth;
  const y = Math.abs(point.y - halfHeight) - halfHeight;
  return Math.hypot(Math.max(x, 0), Math.max(y, 0)) + Math.min(Math.max(x, y), 0);
}

/// Even-odd crossing test against a closed polyline.
function windsAround(walk: ShapePoint[], point: ShapePoint): boolean {
  let inside = false;
  for (let index = 0, previous = walk.length - 1; index < walk.length; previous = index, index += 1) {
    const from = walk[index];
    const to = walk[previous];
    if (
      from.y > point.y !== to.y > point.y &&
      point.x < ((to.x - from.x) * (point.y - from.y)) / (to.y - from.y) + from.x
    ) {
      inside = !inside;
    }
  }
  return inside;
}

function cubicAt(
  from: ShapePoint,
  control1: ShapePoint,
  control2: ShapePoint,
  to: ShapePoint,
  at: number,
): ShapePoint {
  const inverse = 1 - at;
  const a = inverse * inverse * inverse;
  const b = 3 * inverse * inverse * at;
  const c = 3 * inverse * at * at;
  const d = at * at * at;
  return {
    x: a * from.x + b * control1.x + c * control2.x + d * to.x,
    y: a * from.y + b * control1.y + c * control2.y + d * to.y,
  };
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
  closed = false,
): ShapeDraft | null {
  const sampled = deduplicate(input);
  if (sampled.length < 2 || travelledDistance(sampled) < MIN_SHAPE_SIZE_PT) return null;
  // A ring's last sample is the first one again; keeping both would leave a zero-length segment
  // where the curve closes on itself.
  const open = closed && sampled.length > 2 && distance(sampled[0], sampled[sampled.length - 1]) < tolerancePt
    ? sampled.slice(0, -1)
    : sampled;
  const simplified = evenlyCap(simplify(open, Math.max(tolerancePt, 0.25)), MAX_SPLINE_NODES);
  const bounds = pointsBounds(simplified);
  const local = simplified.map((point) => ({ x: point.x - bounds.left, y: point.y - bounds.top }));
  // A closed curve is a ring with no ends, so every knot takes its tangent from the neighbours
  // it actually has — the last one's next is the first. Leaving the end knots straight, as an
  // open curve does, would put a visible corner where the two ends meet.
  const ring = closed && local.length >= MIN_CLOSED_SPLINE_NODES;
  const tangents = besselTangents(local, ring);
  const nodes = local.map((point, index): BezierNode => {
    const first = !ring && index === 0;
    const last = !ring && index === local.length - 1;
    const back = first ? 0 : distance(local[(index - 1 + local.length) % local.length], point);
    const ahead = last ? 0 : distance(point, local[(index + 1) % local.length]);
    const tangent = tangents[index];
    return {
      point,
      handleIn: first ? null : { x: (-tangent.x * back) / 3, y: (-tangent.y * back) / 3 },
      handleOut: last ? null : { x: (tangent.x * ahead) / 3, y: (tangent.y * ahead) / 3 },
    };
  });
  return {
    x: bounds.left,
    y: bounds.top,
    rotation: 0,
    geometry: { kind: "spline", nodes, closed: ring },
    style,
  };
}

/**
 * Each knot's direction, per unit of chord length, from the parabola through it and its two
 * neighbours (Bessel tangents); a handle is then a third of its own segment along it.
 *
 * Simplification leaves knots unevenly spaced. The uniform rule this replaced — a sixth of the
 * neighbour-to-neighbour vector on both sides — sized both handles for neither segment where a
 * short one met a long one, and pointed an open curve's ends along the chord: a hand-drawn half
 * circle bulged, flattened and hooked at its ends, visibly changing roundness from knot to knot.
 * An open end mirrors its neighbour's direction about the end chord, which is the same parabola's
 * slope at the end rather than the chord's.
 */
function besselTangents(points: readonly Point[], ring: boolean): Point[] {
  const count = points.length;
  const unit = (from: Point, to: Point) => {
    const length = distance(from, to);
    return length === 0 ? { x: 0, y: 0 } : { x: (to.x - from.x) / length, y: (to.y - from.y) / length };
  };
  if (count < 2) return points.map(() => ({ x: 0, y: 0 }));
  if (count === 2 && !ring) {
    const along = unit(points[0], points[1]);
    return [along, along];
  }
  const inner = (index: number): Point => {
    const previous = points[(index - 1 + count) % count];
    const point = points[index];
    const next = points[(index + 1) % count];
    const back = distance(previous, point);
    const ahead = distance(point, next);
    if (back === 0 || ahead === 0) return unit(previous, next);
    const total = back + ahead;
    const inward = unit(previous, point);
    const outward = unit(point, next);
    return {
      x: (ahead / total) * inward.x + (back / total) * outward.x,
      y: (ahead / total) * inward.y + (back / total) * outward.y,
    };
  };
  const tangents = points.map((_, index) => (ring || (index > 0 && index < count - 1) ? inner(index) : { x: 0, y: 0 }));
  if (!ring) {
    const startChord = unit(points[0], points[1]);
    const endChord = unit(points[count - 2], points[count - 1]);
    tangents[0] = { x: 2 * startChord.x - tangents[1].x, y: 2 * startChord.y - tangents[1].y };
    tangents[count - 1] = { x: 2 * endChord.x - tangents[count - 2].x, y: 2 * endChord.y - tangents[count - 2].y };
  }
  return tangents;
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

function distanceToSegment(point: ShapePoint, start: ShapePoint, end: ShapePoint): number {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  if (dx === 0 && dy === 0) return distance(point, start);
  const amount = Math.min(
    Math.max(((point.x - start.x) * dx + (point.y - start.y) * dy) / (dx * dx + dy * dy), 0),
    1,
  );
  return distance(point, { x: start.x + amount * dx, y: start.y + amount * dy });
}

function distance(a: ShapePoint, b: ShapePoint): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

function add(a: ShapePoint, b: ShapePoint): ShapePoint {
  return { x: a.x + b.x, y: a.y + b.y };
}

function number(value: number): string {
  return String(Math.round(value * 1000) / 1000);
}
