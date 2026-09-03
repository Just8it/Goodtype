import type { Point } from "../geometry/coordinates";
import type { ShapeGeometry, ShapePoint } from "../model";
import type { ShapeView } from "../prototype/pageView";
import { MIN_CLOSED_SPLINE_NODES, MIN_SHAPE_SIZE_PT, shapeBounds } from "./geometry";

/// Shift-rotation lands on the same twelfth-turn grid a constrained line snaps to.
const ROTATION_SNAP_DEGREES = 15;
const MIN_OPEN_SPLINE_NODES = 2;

export type ShapeAnchor =
  | "line-start"
  | "line-end"
  | "north-west"
  | "north-east"
  | "south-east"
  | "south-west"
  | "north"
  | "east"
  | "south"
  | "west"
  | { node: number }
  | { node: number; handle: "in" | "out" };

export function moveShape(shape: ShapeView, delta: Point): ShapeView {
  return { ...shape, x: shape.x + delta.x, y: shape.y + delta.y };
}

export function resizeShape(
  shape: ShapeView,
  delta: Point,
  constrain = false,
): ShapeView {
  const bounds = shapeBounds(shape.geometry);
  const width = Math.max(bounds.right - bounds.left, 1);
  const height = Math.max(bounds.bottom - bounds.top, 1);
  let scaleX = Math.max((width + delta.x) / width, 0.05);
  let scaleY = Math.max((height + delta.y) / height, 0.05);
  if (constrain) {
    // One factor for both axes, taken from whichever the pointer moved further along. Picking
    // the larger of the two ratios instead would shrink by the axis that shrank least, so the
    // same drag grew and shrank at different rates.
    const uniform = Math.abs(delta.x) >= Math.abs(delta.y) ? scaleX : scaleY;
    scaleX = scaleY = uniform;
  }
  return { ...shape, geometry: scaleGeometry(shape.geometry, { x: bounds.left, y: bounds.top }, scaleX, scaleY) };
}

export function moveShapeAnchor(shape: ShapeView, anchor: ShapeAnchor, delta: Point): ShapeView {
  const geometry = shape.geometry;
  if (geometry.kind === "line" && typeof anchor === "string") {
    // The store refuses a line whose ends coincide, and rightly — it is not a line. Catching it
    // here keeps a dragged handle from turning into a commit failure the user cannot act on.
    if (anchor === "line-start") {
      return {
        ...shape,
        geometry: { ...geometry, start: separated(add(geometry.start, delta), geometry.end) },
      };
    }
    if (anchor === "line-end") {
      return {
        ...shape,
        geometry: { ...geometry, end: separated(add(geometry.end, delta), geometry.start) },
      };
    }
  }
  if (geometry.kind === "rectangle" && typeof anchor === "string") {
    return resizeParametric(shape, anchor, delta, geometry.cornerRadiusPt);
  }
  if (geometry.kind === "ellipse" && typeof anchor === "string") {
    return resizeParametric(shape, anchor, delta);
  }
  if (geometry.kind !== "spline" || typeof anchor === "string") return shape;
  const node = geometry.nodes[anchor.node];
  if (!node) return shape;
  const nodes = geometry.nodes.map((candidate, index) => {
    if (index !== anchor.node) return candidate;
    if (!("handle" in anchor)) return { ...candidate, point: add(candidate.point, delta) };
    const key = anchor.handle === "in" ? "handleIn" : "handleOut";
    return { ...candidate, [key]: add(candidate[key] ?? { x: 0, y: 0 }, delta) };
  });
  return { ...shape, geometry: { ...geometry, nodes } };
}

/**
 * Turn a shape about the middle of its own bounds.
 *
 * Stored rotation is about the shape's local origin, because that is what the renderer and the
 * PDF exporter both apply. Turning about the origin is not what a grabbed handle means, though,
 * so the origin is moved to hold the centre still — the shape spins in place, and what is stored
 * stays the one transform every renderer already agrees on.
 */
export function rotateShape(
  shape: ShapeView,
  grabbed: ShapePoint,
  delta: Point,
  snap = false,
): ShapeView {
  const centre = boundsCentre(shape.geometry);
  const pivot = worldPoint(shape, centre);
  const from = worldPoint(shape, grabbed);
  const to = { x: from.x + delta.x, y: from.y + delta.y };
  const turned =
    Math.atan2(to.y - pivot.y, to.x - pivot.x) - Math.atan2(from.y - pivot.y, from.x - pivot.x);
  const degrees = shape.rotation + (turned * 180) / Math.PI;
  const rotation = snap ? Math.round(degrees / ROTATION_SNAP_DEGREES) * ROTATION_SNAP_DEGREES : degrees;
  const turnedShape = { ...shape, rotation };
  const moved = worldPoint(turnedShape, centre);
  return {
    ...turnedShape,
    x: turnedShape.x + pivot.x - moved.x,
    y: turnedShape.y + pivot.y - moved.y,
  };
}

/// Where a rotation handle sits: straight above the bounds, in the shape's own coordinates.
export function rotationHandlePoint(geometry: ShapeGeometry, offset: number): ShapePoint {
  const bounds = shapeBounds(geometry);
  return { x: (bounds.left + bounds.right) / 2, y: bounds.top - offset };
}

/// A spline may close only when it has enough knots to enclose anything; the store agrees.
export function canCloseSpline(geometry: ShapeGeometry): boolean {
  return geometry.kind === "spline" && geometry.nodes.length >= MIN_CLOSED_SPLINE_NODES;
}

export function toggleSplineClosed(shape: ShapeView): ShapeView {
  const geometry = shape.geometry;
  if (geometry.kind !== "spline") return shape;
  if (!geometry.closed && !canCloseSpline(geometry)) return shape;
  return { ...shape, geometry: { ...geometry, closed: !geometry.closed } };
}

/**
 * Drop one knot from a spline, keeping enough behind to remain a curve the store will accept:
 * two for an open path, three for a closed one.
 */
export function removeSplineNode(shape: ShapeView, index: number): ShapeView {
  const geometry = shape.geometry;
  if (geometry.kind !== "spline") return shape;
  const floor = geometry.closed ? MIN_CLOSED_SPLINE_NODES : MIN_OPEN_SPLINE_NODES;
  if (geometry.nodes.length <= floor || !geometry.nodes[index]) return shape;
  return {
    ...shape,
    geometry: { ...geometry, nodes: geometry.nodes.filter((_, at) => at !== index) },
  };
}

export function anchorPoints(geometry: ShapeGeometry): { anchor: ShapeAnchor; point: ShapePoint }[] {
  switch (geometry.kind) {
    case "line":
      return [
        { anchor: "line-start", point: geometry.start },
        { anchor: "line-end", point: geometry.end },
      ];
    case "rectangle":
    case "ellipse":
      return boxAnchors(geometry.widthPt, geometry.heightPt);
    case "spline":
      return geometry.nodes.flatMap((node, index) => [
        { anchor: { node: index }, point: node.point },
        ...(node.handleIn
          ? [{ anchor: { node: index, handle: "in" as const }, point: add(node.point, node.handleIn) }]
          : []),
        ...(node.handleOut
          ? [{ anchor: { node: index, handle: "out" as const }, point: add(node.point, node.handleOut) }]
          : []),
      ]);
  }
}

function boundsCentre(geometry: ShapeGeometry): ShapePoint {
  const bounds = shapeBounds(geometry);
  return { x: (bounds.left + bounds.right) / 2, y: (bounds.top + bounds.bottom) / 2 };
}

/// A point in the shape's own coordinates, placed on the page by the transform every renderer
/// applies: `translate(x y) rotate(rotation) scale(scale)`.
function worldPoint(shape: ShapeView, point: ShapePoint): Point {
  const angle = (shape.rotation * Math.PI) / 180;
  const x = point.x * shape.scale;
  const y = point.y * shape.scale;
  return {
    x: shape.x + x * Math.cos(angle) - y * Math.sin(angle),
    y: shape.y + x * Math.sin(angle) + y * Math.cos(angle),
  };
}

/// Push `point` off `anchor` along the line they already make, so the two never coincide.
function separated(point: ShapePoint, anchor: ShapePoint): ShapePoint {
  const dx = point.x - anchor.x;
  const dy = point.y - anchor.y;
  const length = Math.hypot(dx, dy);
  if (length >= MIN_SHAPE_SIZE_PT) return point;
  if (length === 0) return { x: anchor.x + MIN_SHAPE_SIZE_PT, y: anchor.y };
  return {
    x: anchor.x + (dx / length) * MIN_SHAPE_SIZE_PT,
    y: anchor.y + (dy / length) * MIN_SHAPE_SIZE_PT,
  };
}

export function localDelta(delta: Point, rotationDegrees: number, scale: number): Point {
  const angle = (-rotationDegrees * Math.PI) / 180;
  const safeScale = Math.max(Math.abs(scale), 0.001);
  return {
    x: (delta.x * Math.cos(angle) - delta.y * Math.sin(angle)) / safeScale,
    y: (delta.x * Math.sin(angle) + delta.y * Math.cos(angle)) / safeScale,
  };
}

function resizeParametric(
  shape: ShapeView,
  anchor: string,
  delta: Point,
  cornerRadius?: number,
): ShapeView {
  const geometry = shape.geometry;
  if (geometry.kind !== "rectangle" && geometry.kind !== "ellipse") return shape;
  const movesWest = anchor === "north-west" || anchor === "south-west" || anchor === "west";
  const movesEast = anchor === "north-east" || anchor === "south-east" || anchor === "east";
  const movesNorth = anchor === "north-west" || anchor === "north-east" || anchor === "north";
  const movesSouth = anchor === "south-west" || anchor === "south-east" || anchor === "south";
  const left = movesWest ? Math.min(delta.x, geometry.widthPt - 4) : 0;
  const top = movesNorth ? Math.min(delta.y, geometry.heightPt - 4) : 0;
  const right = geometry.widthPt + (movesEast ? delta.x : 0);
  const bottom = geometry.heightPt + (movesSouth ? delta.y : 0);
  const widthPt = Math.max(4, right - left);
  const heightPt = Math.max(4, bottom - top);
  const origin = rotateAndScale({ x: left, y: top }, shape.rotation, shape.scale);
  const resized = geometry.kind === "rectangle"
    ? {
        kind: "rectangle" as const,
        widthPt,
        heightPt,
        cornerRadiusPt: Math.min(cornerRadius ?? geometry.cornerRadiusPt, widthPt / 2, heightPt / 2),
      }
    : { kind: "ellipse" as const, widthPt, heightPt };
  return { ...shape, x: shape.x + origin.x, y: shape.y + origin.y, geometry: resized };
}

function scaleGeometry(
  geometry: ShapeGeometry,
  origin: Point,
  scaleX: number,
  scaleY: number,
): ShapeGeometry {
  const point = (value: ShapePoint) => ({
    x: origin.x + (value.x - origin.x) * scaleX,
    y: origin.y + (value.y - origin.y) * scaleY,
  });
  switch (geometry.kind) {
    case "line":
      return { ...geometry, start: point(geometry.start), end: point(geometry.end) };
    case "rectangle":
      return {
        ...geometry,
        widthPt: geometry.widthPt * scaleX,
        heightPt: geometry.heightPt * scaleY,
        cornerRadiusPt: Math.min(geometry.cornerRadiusPt * Math.min(scaleX, scaleY), geometry.widthPt * scaleX / 2, geometry.heightPt * scaleY / 2),
      };
    case "ellipse":
      return { ...geometry, widthPt: geometry.widthPt * scaleX, heightPt: geometry.heightPt * scaleY };
    case "spline":
      return {
        ...geometry,
        nodes: geometry.nodes.map((node) => ({
          ...node,
          point: point(node.point),
          handleIn: node.handleIn ? { x: node.handleIn.x * scaleX, y: node.handleIn.y * scaleY } : null,
          handleOut: node.handleOut ? { x: node.handleOut.x * scaleX, y: node.handleOut.y * scaleY } : null,
        })),
      };
  }
}

/// Corners resize both axes, edges resize one. A rectangle and an ellipse are the same box to
/// drag, so they get the same handles rather than one of each set for no reason anyone could name.
function boxAnchors(width: number, height: number) {
  return [
    { anchor: "north-west" as const, point: { x: 0, y: 0 } },
    { anchor: "north" as const, point: { x: width / 2, y: 0 } },
    { anchor: "north-east" as const, point: { x: width, y: 0 } },
    { anchor: "east" as const, point: { x: width, y: height / 2 } },
    { anchor: "south-east" as const, point: { x: width, y: height } },
    { anchor: "south" as const, point: { x: width / 2, y: height } },
    { anchor: "south-west" as const, point: { x: 0, y: height } },
    { anchor: "west" as const, point: { x: 0, y: height / 2 } },
  ];
}

function rotateAndScale(point: Point, rotationDegrees: number, scale: number): Point {
  const angle = (rotationDegrees * Math.PI) / 180;
  return {
    x: (point.x * Math.cos(angle) - point.y * Math.sin(angle)) * scale,
    y: (point.x * Math.sin(angle) + point.y * Math.cos(angle)) * scale,
  };
}

function add(a: ShapePoint, b: ShapePoint): ShapePoint {
  return { x: a.x + b.x, y: a.y + b.y };
}
