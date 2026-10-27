// The precise eraser on shapes: cut the outline where the eraser touched it, and keep what is left
// as shapes that describe the same outline exactly (ADR 0037).
//
// Every outline is first written as cubic Bézier pieces: a line or a rectangle side is a cubic
// with its handles on its ends, a rounded corner is its quadratic raised to a cubic (exact), and
// an ellipse is cubic arcs of 45°, whose radial error of 4.2e-6 of the radius stays below the
// stored 0.01 pt precision on any page-sized ellipse. The pieces are flattened finely only to
// find where the eraser touches; the result is split from the curves themselves, so the outline
// that is kept is not an approximation of what was drawn but the drawn outline.
//
// A line stays a line. Everything else becomes open splines that the writer can still edit.

import type { Point } from "../geometry/coordinates";
import { complement, contactIntervals, type EraserSweep } from "../ink/erase";
import { distanceToSegment } from "../ink/selection";
import type { BezierNode, ShapeGeometry, ShapePoint } from "../model";
import type { ShapeView } from "../prototype/pageView";
import { containsPoint } from "./geometry";
import { toLocalPoint } from "./hit";

type Cubic = [ShapePoint, ShapePoint, ShapePoint, ShapePoint];
/** `straight` marks pieces that are straight lines, so they rebuild as corners without handles. */
type Outline = { cubics: Cubic[]; straight: boolean[]; closed: boolean };

/** Flattening tolerance, in page points: well under the 0.01 pt that geometry is stored at. */
const FLATNESS_PT = 0.002;
const MAX_PIECES_PER_CUBIC = 2048;
/** `4/3 · tan(φ/4)` for a 45° arc. */
const ARC_HANDLE = (4 / 3) * Math.tan(Math.PI / 16);

export type ShapeErase = {
  shapes: ShapeView[];
  /** Every shape this pass changed, mapped to the pieces now standing for it, or none. */
  replaced: Map<string, string[]>;
};

/** Cut every shape the sweeps touch. A pass that touched nothing returns the same array. */
export function eraseShapesThrough(
  shapes: ShapeView[],
  sweeps: EraserSweep[],
  newId: () => string,
): ShapeErase {
  const replaced = new Map<string, string[]>();
  if (sweeps.length === 0) return { shapes, replaced };
  const next: ShapeView[] = [];
  for (const shape of shapes) {
    const pieces = cutShape(shape, sweeps, newId);
    if (!pieces) {
      next.push(shape);
      continue;
    }
    next.push(...pieces);
    replaced.set(shape.id, pieces.map((piece) => piece.id));
  }
  return { shapes: replaced.size > 0 ? next : shapes, replaced };
}

/** The shape's pieces after the sweeps, `[]` when nothing is left, `null` when untouched. */
export function cutShape(shape: ShapeView, sweeps: EraserSweep[], newId: () => string): ShapeView[] | null {
  const outline = outlineOf(shape.geometry);
  const walk = flatten(outline, shape);
  if (walk.page.length === 0) return null;
  const radius = (shape.style.strokeWidthPt * Math.abs(shape.scale)) / 2;
  const radii = walk.page.map(() => radius);
  const removed = walk.page.length === 1
    ? (sweeps.some((sweep) => distanceToSegment(walk.page[0], sweep.from, sweep.to) <= sweep.radius + radius) ? [[0, 0] as [number, number]] : [])
    : contactIntervals(walk.page, radii, false, sweeps);

  // A fill is an area, and an outline model cannot cut a hole in it: a touched filled shape goes
  // whole, as it always has. Touching the tint counts, as it does for the stroke eraser.
  if (shape.style.fillColor !== null) {
    const touched = removed.length > 0 || sweeps.some((sweep) =>
      [sweep.from, sweep.to].some((point) => containsPoint(shape.geometry, toLocalPoint(shape, point))));
    return touched ? [] : null;
  }
  if (removed.length === 0) return null;
  const last = walk.page.length - 1;
  let kept = complement(removed, last);
  if (kept.length === 0) return [];

  // A closed outline that kept both sides of its seam is one piece running through it. A
  // patterned outline restarts its dashes at the seam, so it stays two pieces there and every
  // dash keeps its place.
  const solid = (shape.style.pattern ?? "solid") === "solid";
  const wraps = outline.closed && solid && kept.length > 1 && kept[0][0] === 0 && kept[kept.length - 1][1] === last;
  const runs: [number, number][][] = wraps
    ? [[kept[kept.length - 1], kept[0]], ...kept.slice(1, -1).map((span) => [span])]
    : kept.map((span) => [span]);

  const pieces: ShapeView[] = [];
  for (const run of runs) {
    const settled = run.map((span, index) => clearEnds(walk, shape, sweeps, span,
      index === 0 && span[0] > 0, index === run.length - 1 && span[1] < last));
    if (settled.some((span) => !span)) continue;
    const spans = settled as [number, number][];
    const geometry = shape.geometry.kind === "line"
      ? lineBetween(walk, spans[0])
      : splineBetween(walk, outline, spans);
    if (!geometry) continue;
    const offset = (shape.style.patternOffsetPt ?? 0) + lengthAt(walk, spans[0][0]);
    pieces.push({
      ...shape,
      id: pieces.length === 0 ? shape.id : newId(),
      geometry,
      style: solid || offset === 0
        ? withoutOffset(shape.style)
        : { ...shape.style, patternOffsetPt: offset },
    });
  }
  return pieces;
}

function withoutOffset(style: ShapeView["style"]): ShapeView["style"] {
  if (style.patternOffsetPt === undefined) return style;
  const rest = { ...style };
  delete rest.patternOffsetPt;
  return rest;
}

/** The outline as cubic pieces, starting where and running the way `shapePath` draws it. */
function outlineOf(geometry: ShapeGeometry): Outline {
  const outline: Outline = { cubics: [], straight: [], closed: false };
  const line = (from: ShapePoint, to: ShapePoint) => {
    outline.cubics.push(straight(from, to));
    outline.straight.push(true);
  };
  const curve = (cubic: Cubic) => {
    outline.cubics.push(cubic);
    outline.straight.push(false);
  };
  switch (geometry.kind) {
    case "line":
      line(geometry.start, geometry.end);
      return outline;
    case "rectangle": {
      const { widthPt: w, heightPt: h } = geometry;
      const r = Math.min(Math.max(geometry.cornerRadiusPt, 0), w / 2, h / 2);
      outline.closed = true;
      if (r === 0) {
        const corners = [{ x: 0, y: 0 }, { x: w, y: 0 }, { x: w, y: h }, { x: 0, y: h }];
        corners.forEach((corner, index) => line(corner, corners[(index + 1) % 4]));
        return outline;
      }
      const side = (from: ShapePoint, to: ShapePoint) => {
        if (from.x !== to.x || from.y !== to.y) line(from, to);
      };
      side({ x: r, y: 0 }, { x: w - r, y: 0 });
      curve(raised({ x: w - r, y: 0 }, { x: w, y: 0 }, { x: w, y: r }));
      side({ x: w, y: r }, { x: w, y: h - r });
      curve(raised({ x: w, y: h - r }, { x: w, y: h }, { x: w - r, y: h }));
      side({ x: w - r, y: h }, { x: r, y: h });
      curve(raised({ x: r, y: h }, { x: 0, y: h }, { x: 0, y: h - r }));
      side({ x: 0, y: h - r }, { x: 0, y: r });
      curve(raised({ x: 0, y: r }, { x: 0, y: 0 }, { x: r, y: 0 }));
      return outline;
    }
    case "ellipse": {
      // `shapePath` starts at the right and sweeps through the top first.
      const rx = geometry.widthPt / 2;
      const ry = geometry.heightPt / 2;
      const at = (angle: number) => ({ x: rx + rx * Math.cos(angle), y: ry + ry * Math.sin(angle) });
      const tangent = (angle: number) => ({ x: -rx * Math.sin(angle), y: ry * Math.cos(angle) });
      outline.closed = true;
      for (let index = 0; index < 8; index += 1) {
        const from = (-index * Math.PI) / 4;
        const to = (-(index + 1) * Math.PI) / 4;
        const start = at(from), end = at(to), out = tangent(from), into = tangent(to);
        curve([
          start,
          { x: start.x - ARC_HANDLE * out.x, y: start.y - ARC_HANDLE * out.y },
          { x: end.x + ARC_HANDLE * into.x, y: end.y + ARC_HANDLE * into.y },
          end,
        ]);
      }
      return outline;
    }
    case "spline": {
      const { nodes, closed } = geometry;
      const count = closed && nodes.length > 2 ? nodes.length : nodes.length - 1;
      for (let index = 0; index < count; index += 1) {
        const from = nodes[index];
        const to = nodes[(index + 1) % nodes.length];
        // A segment with no handles is drawn straight and stays a corner-to-corner line.
        if (!from.handleOut && !to.handleIn) line(from.point, to.point);
        else curve([from.point, add(from.point, from.handleOut), add(to.point, to.handleIn), to.point]);
      }
      if (count <= 0 && nodes.length === 1) line(nodes[0].point, nodes[0].point);
      outline.closed = closed && nodes.length > 2;
      return outline;
    }
  }
}

type Walk = {
  page: Point[];
  local: ShapePoint[];
  /** For each flattened segment: which cubic, and the parameters at its two ends. */
  pieces: { cubic: number; from: number; to: number }[];
  /** Arc length of the outline, in the shape's own units, at each flattened point. */
  lengths: number[];
  cubics: Cubic[];
};

function flatten(outline: Outline, shape: ShapeView): Walk {
  const scale = Math.abs(shape.scale);
  const walk: Walk = { page: [], local: [], pieces: [], lengths: [], cubics: outline.cubics };
  outline.cubics.forEach((cubic, index) => {
    const [p0, p1, p2, p3] = cubic;
    const bend = outline.straight[index] ? 0 : Math.max(
      Math.hypot(p0.x - 2 * p1.x + p2.x, p0.y - 2 * p1.y + p2.y),
      Math.hypot(p1.x - 2 * p2.x + p3.x, p1.y - 2 * p2.y + p3.y),
    ) * scale;
    // Uniform subdivision deviates from a cubic by at most ¾ · bend / n².
    const count = Math.min(MAX_PIECES_PER_CUBIC, Math.max(1, Math.ceil(Math.sqrt((0.75 * bend) / FLATNESS_PT))));
    for (let step = index === 0 ? 0 : 1; step <= count; step += 1) {
      const point = cubicAt(cubic, step / count);
      const previous = walk.local[walk.local.length - 1];
      walk.lengths.push(previous ? walk.lengths[walk.lengths.length - 1] + Math.hypot(point.x - previous.x, point.y - previous.y) : 0);
      walk.local.push(point);
      walk.page.push(toPage(shape, point));
      if (step > 0) walk.pieces.push({ cubic: index, from: (step - 1) / count, to: step / count });
    }
  });
  return walk;
}

/** The cubic and its parameter at walk position `u`. */
function parameterAt(walk: Walk, u: number): { cubic: number; t: number } {
  const index = Math.min(Math.floor(u), walk.pieces.length - 1);
  const piece = walk.pieces[index];
  return { cubic: piece.cubic, t: piece.from + (piece.to - piece.from) * (u - index) };
}

function lengthAt(walk: Walk, u: number): number {
  const index = Math.min(Math.floor(u), walk.local.length - 2);
  if (index < 0) return 0;
  return walk.lengths[index] + (walk.lengths[index + 1] - walk.lengths[index]) * (u - index);
}

function exactAt(walk: Walk, u: number): ShapePoint {
  const { cubic, t } = parameterAt(walk, u);
  return cubicAt(walk.cubics[cubic], t);
}

/**
 * The span the eraser was bisected off is measured on the flattened outline, which can sit up to
 * the flatness tolerance inside the true curve. Check each new end on the curve itself and step it
 * outward until it is clear. `null` when nothing clear is left.
 */
function clearEnds(
  walk: Walk,
  shape: ShapeView,
  sweeps: EraserSweep[],
  span: [number, number],
  checkStart: boolean,
  checkEnd: boolean,
): [number, number] | null {
  let [from, to] = span;
  const radius = (shape.style.strokeWidthPt * Math.abs(shape.scale)) / 2;
  const clear = (u: number) => {
    const point = toPage(shape, exactAt(walk, u));
    return sweeps.every((sweep) => distanceToSegment(point, sweep.from, sweep.to) - radius > sweep.radius);
  };
  for (let attempt = 0; attempt < 8; attempt += 1) {
    const startClear = !checkStart || clear(from);
    const endClear = !checkEnd || clear(to);
    if (startClear && endClear) return to > from ? [from, to] : null;
    const step = (u: number) => {
      const index = Math.min(Math.floor(u), walk.page.length - 2);
      const length = Math.hypot(walk.page[index + 1].x - walk.page[index].x, walk.page[index + 1].y - walk.page[index].y);
      return length > 0 ? (FLATNESS_PT * 2 ** attempt) / length : 1;
    };
    if (!startClear) from = Math.min(to, from + step(from));
    if (!endClear) to = Math.max(from, to - step(to));
    if (to <= from) return null;
  }
  return null;
}

function lineBetween(walk: Walk, [from, to]: [number, number]): ShapeGeometry | null {
  const start = exactAt(walk, from);
  const end = exactAt(walk, to);
  return start.x === end.x && start.y === end.y ? null : { kind: "line", start, end };
}

/** An open spline through the given spans, which follow one another along the outline. */
function splineBetween(walk: Walk, outline: Outline, spans: [number, number][]): ShapeGeometry | null {
  const segments: Cubic[] = [];
  const straightness: boolean[] = [];
  for (const [from, to] of spans) {
    const start = parameterAt(walk, from);
    const end = parameterAt(walk, to);
    for (let cubic = start.cubic; cubic <= end.cubic; cubic += 1) {
      const t0 = cubic === start.cubic ? start.t : 0;
      const t1 = cubic === end.cubic ? end.t : 1;
      if (t1 > t0) {
        segments.push(subCubic(outline.cubics[cubic], t0, t1));
        straightness.push(outline.straight[cubic]);
      }
    }
  }
  if (segments.length === 0) return null;
  // A straight piece is stored without handles, which is how a corner-to-corner line is drawn.
  const handleOf = (index: number, knot: ShapePoint, control: ShapePoint) =>
    straightness[index] ? null : handle(knot, control);
  const nodes: BezierNode[] = [
    { point: segments[0][0], handleIn: null, handleOut: handleOf(0, segments[0][0], segments[0][1]) },
  ];
  segments.forEach((segment, index) => {
    const next = segments[index + 1];
    nodes.push({
      point: segment[3],
      handleIn: handleOf(index, segment[3], segment[2]),
      handleOut: next ? handleOf(index + 1, segment[3], next[1]) : null,
    });
  });
  const first = nodes[0].point, final = nodes[nodes.length - 1].point;
  if (nodes.length === 2 && first.x === final.x && first.y === final.y) return null;
  return { kind: "spline", nodes, closed: false };
}

/** A handle as the shape model stores it: relative to its knot, absent when it is the knot. */
function handle(knot: ShapePoint, control: ShapePoint): ShapePoint | null {
  const x = control.x - knot.x;
  const y = control.y - knot.y;
  return Math.hypot(x, y) < 1e-9 ? null : { x, y };
}

/** The part of a cubic between two of its parameters, by de Casteljau subdivision. */
function subCubic(cubic: Cubic, from: number, to: number): Cubic {
  const head = to < 1 ? split(cubic, to)[0] : cubic;
  return from > 0 ? split(head, from / to)[1] : head;
}

function split([p0, p1, p2, p3]: Cubic, t: number): [Cubic, Cubic] {
  const a = lerp(p0, p1, t), b = lerp(p1, p2, t), c = lerp(p2, p3, t);
  const d = lerp(a, b, t), e = lerp(b, c, t);
  const f = lerp(d, e, t);
  return [[p0, a, d, f], [f, e, c, p3]];
}

function cubicAt(cubic: Cubic, t: number): ShapePoint {
  if (t <= 0) return cubic[0];
  if (t >= 1) return cubic[3];
  const [p0, p1, p2, p3] = cubic;
  const s = 1 - t;
  const a = s * s * s, b = 3 * s * s * t, c = 3 * s * t * t, d = t * t * t;
  return { x: a * p0.x + b * p1.x + c * p2.x + d * p3.x, y: a * p0.y + b * p1.y + c * p2.y + d * p3.y };
}

/** A straight line as a cubic with evenly spaced handles, so its parameter runs evenly along it. */
function straight(from: ShapePoint, to: ShapePoint): Cubic {
  return [from, lerp(from, to, 1 / 3), lerp(from, to, 2 / 3), to];
}

/** A quadratic Bézier written exactly as a cubic. */
function raised(from: ShapePoint, control: ShapePoint, to: ShapePoint): Cubic {
  return [
    from,
    { x: from.x + (2 / 3) * (control.x - from.x), y: from.y + (2 / 3) * (control.y - from.y) },
    { x: to.x + (2 / 3) * (control.x - to.x), y: to.y + (2 / 3) * (control.y - to.y) },
    to,
  ];
}

function lerp(a: ShapePoint, b: ShapePoint, t: number): ShapePoint {
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
}

function add(point: ShapePoint, offset: ShapePoint | null): ShapePoint {
  return offset ? { x: point.x + offset.x, y: point.y + offset.y } : point;
}

/// The transform every renderer applies: `translate(x y) rotate(rotation) scale(scale)`.
function toPage(shape: ShapeView, local: ShapePoint): Point {
  const angle = (shape.rotation * Math.PI) / 180;
  const x = local.x * shape.scale;
  const y = local.y * shape.scale;
  return {
    x: shape.x + x * Math.cos(angle) - y * Math.sin(angle),
    y: shape.y + x * Math.sin(angle) + y * Math.cos(angle),
  };
}
