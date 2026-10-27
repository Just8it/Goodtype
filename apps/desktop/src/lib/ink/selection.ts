import type { Point } from "../geometry/coordinates";
import type { Stroke, StrokePattern, StrokePoint } from "../model";
import type { InkTool } from "./pipeline";

export type Bounds = {
  left: number;
  top: number;
  right: number;
  bottom: number;
};

const boundsCache = new WeakMap<Stroke, Bounds | null>();

export function transformedPoint(stroke: Stroke, point: StrokePoint): Point {
  const scaledX = point.x * stroke.transform.scaleX;
  const scaledY = point.y * stroke.transform.scaleY;
  const angle = (stroke.transform.rotation * Math.PI) / 180;
  return {
    x:
      scaledX * Math.cos(angle) -
      scaledY * Math.sin(angle) +
      stroke.transform.translateX,
    y:
      scaledX * Math.sin(angle) +
      scaledY * Math.cos(angle) +
      stroke.transform.translateY,
  };
}

export function strokeBounds(stroke: Stroke): Bounds | null {
  if (boundsCache.has(stroke)) return boundsCache.get(stroke)!;
  const bounds = stroke.points.length === 0 ? null : pointsBounds(stroke.points.map((point) => transformedPoint(stroke, point)));
  boundsCache.set(stroke, bounds);
  return bounds;
}

export function selectionBounds(strokes: Stroke[], selectedIds: string[]): Bounds | null {
  const selected = new Set(selectedIds);
  const bounds = strokes
    .filter((stroke) => selected.has(stroke.id))
    .map(strokeBounds)
    .filter((value): value is Bounds => value !== null);
  if (bounds.length === 0) return null;
  return {
    left: Math.min(...bounds.map((value) => value.left)),
    top: Math.min(...bounds.map((value) => value.top)),
    right: Math.max(...bounds.map((value) => value.right)),
    bottom: Math.max(...bounds.map((value) => value.bottom)),
  };
}

export function selectStrokesInLasso(strokes: Stroke[], polygon: Point[], reaches: StrokeFilter = () => true): string[] {
  if (polygon.length < 3) return [];
  return strokes
    .filter((stroke) => reaches(stroke) && polylineInLasso(
      stroke.points.map((point) => transformedPoint(stroke, point)), polygon,
    ))
    .map((stroke) => stroke.id);
}

/** Which strokes a hit may land on; the eraser narrows this to highlighter sweeps on request. */
export type StrokeFilter = (stroke: Stroke) => boolean;

export function hitStroke(
  strokes: Stroke[],
  point: Point,
  radius: number,
  reaches: StrokeFilter = () => true,
): Stroke | null {
  for (let strokeIndex = strokes.length - 1; strokeIndex >= 0; strokeIndex -= 1) {
    const stroke = strokes[strokeIndex];
    if (!reaches(stroke)) continue;
    const hitRadius =
      Math.max(radius, (stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY)) / 2);
    const bounds = strokeBounds(stroke);
    if (!bounds || point.x < bounds.left - hitRadius || point.x > bounds.right + hitRadius ||
      point.y < bounds.top - hitRadius || point.y > bounds.bottom + hitRadius) continue;
    const points = stroke.points.map((sample) => transformedPoint(stroke, sample));
    if (
      points.some((sample, index) =>
        index === 0
          ? Math.hypot(point.x - sample.x, point.y - sample.y) <= hitRadius
          : distanceToSegment(point, points[index - 1], sample) <= hitRadius,
      )
    ) {
      return stroke;
    }
  }
  return null;
}

export function eraseStrokeAt(
  strokes: Stroke[],
  point: Point,
  radius: number,
  reaches: StrokeFilter = () => true,
): Stroke[] {
  const hit = hitStroke(strokes, point, radius, reaches);
  return hit ? strokes.filter((stroke) => stroke.id !== hit.id) : strokes;
}

/** What restyling a selection may change. Absent fields are left as they are. */
export type StylePatch = {
  color?: string;
  widthPt?: number;
  pattern?: StrokePattern;
  patternSpacing?: number;
};

/**
 * Apply a style to the selected strokes only, as new stroke objects so every cache keyed on a
 * stroke's identity rebuilds. Highlighter sweeps take colour and width but stay solid, the same
 * rule the highlighter follows when it draws.
 */
export function restyleStrokes(
  strokes: Stroke[],
  selectedIds: Iterable<string>,
  patch: StylePatch,
): Stroke[] {
  const selected = new Set(selectedIds);
  return strokes.map((stroke) => {
    if (!selected.has(stroke.id)) return stroke;
    const next: Stroke = { ...stroke };
    if (patch.color !== undefined) next.color = patch.color;
    if (patch.widthPt !== undefined) next.widthPt = patch.widthPt;
    if (stroke.tool === "pen") {
      if (patch.pattern !== undefined) next.pattern = patch.pattern;
      if (patch.patternSpacing !== undefined) {
        if (patch.patternSpacing === 1) delete next.patternSpacing;
        else next.patternSpacing = patch.patternSpacing;
      }
    }
    return next;
  });
}

/** The value every item shares, or undefined when they differ or there are none. */
export function shared<T>(values: T[]): T | undefined {
  if (values.length === 0) return undefined;
  return values.every((value) => value === values[0]) ? values[0] : undefined;
}

export function moveSelected(
  strokes: Stroke[],
  selectedIds: string[],
  delta: Point,
): Stroke[] {
  const selected = new Set(selectedIds);
  return strokes.map((stroke) =>
    selected.has(stroke.id)
      ? {
          ...stroke,
          transform: {
            ...stroke.transform,
            translateX: stroke.transform.translateX + delta.x,
            translateY: stroke.transform.translateY + delta.y,
          },
        }
      : stroke,
  );
}

export function scaleSelected(
  strokes: Stroke[],
  selectedIds: string[],
  anchor: Point,
  scale: number,
): Stroke[] {
  const selected = new Set(selectedIds);
  const safeScale = Math.max(scale, 0.05);
  return strokes.map((stroke) =>
    selected.has(stroke.id)
      ? {
          ...stroke,
          transform: {
            ...stroke.transform,
            translateX:
              anchor.x + (stroke.transform.translateX - anchor.x) * safeScale,
            translateY:
              anchor.y + (stroke.transform.translateY - anchor.y) * safeScale,
            scaleX: stroke.transform.scaleX * safeScale,
            scaleY: stroke.transform.scaleY * safeScale,
          },
        }
      : stroke,
  );
}

function pointsBounds(points: Point[]): Bounds {
  const bounds = { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity };
  for (const point of points) {
    bounds.left = Math.min(bounds.left, point.x);
    bounds.top = Math.min(bounds.top, point.y);
    bounds.right = Math.max(bounds.right, point.x);
    bounds.bottom = Math.max(bounds.bottom, point.y);
  }
  return bounds;
}

export function pointInPolygon(point: Point, polygon: Point[]): boolean {
  let inside = false;
  for (let current = 0, previous = polygon.length - 1; current < polygon.length; previous = current++) {
    const a = polygon[current];
    const b = polygon[previous];
    if (
      a.y > point.y !== b.y > point.y &&
      point.x < ((b.x - a.x) * (point.y - a.y)) / (b.y - a.y) + a.x
    ) {
      inside = !inside;
    }
  }
  return inside;
}

/** Include crossings between sparse samples, not just samples that fall inside the lasso. */
export function polylineInLasso(points: Point[], polygon: Point[], closed = false): boolean {
  if (polygon.length < 3 || points.length === 0) return false;
  const aabb = (values: Point[]) => values.reduce((b, p) => ({ left: Math.min(b.left, p.x), top: Math.min(b.top, p.y),
    right: Math.max(b.right, p.x), bottom: Math.max(b.bottom, p.y) }), { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity });
  const line = aabb(points), loop = aabb(polygon);
  if (line.right < loop.left || line.left > loop.right || line.bottom < loop.top || line.top > loop.bottom) return false;
  if (points.some((point) => pointInPolygon(point, polygon))) return true;
  if (closed && polygon.some((point) => pointInPolygon(point, points))) return true;
  for (let i = 1; i < points.length + (closed ? 1 : 0); i++) {
    const a = points[i - 1], b = points[i % points.length];
    for (let j = 0; j < polygon.length; j++) {
      const c = polygon[j], d = polygon[(j + 1) % polygon.length];
      const cross = (p: Point, q: Point, r: Point) => (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
      if (Math.max(a.x, b.x) >= Math.min(c.x, d.x) && Math.max(c.x, d.x) >= Math.min(a.x, b.x) &&
        Math.max(a.y, b.y) >= Math.min(c.y, d.y) && Math.max(c.y, d.y) >= Math.min(a.y, b.y) &&
        cross(a, b, c) * cross(a, b, d) <= 0 && cross(c, d, a) * cross(c, d, b) <= 0) return true;
    }
  }
  return false;
}

export function distanceToSegment(point: Point, start: Point, end: Point): number {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  if (dx === 0 && dy === 0) return Math.hypot(point.x - start.x, point.y - start.y);
  const amount = Math.min(
    Math.max(((point.x - start.x) * dx + (point.y - start.y) * dy) / (dx * dx + dy * dy), 0),
    1,
  );
  return Math.hypot(point.x - (start.x + amount * dx), point.y - (start.y + amount * dy));
}

/**
 * The lasso hands over to the selection tool the moment it catches something, so the writer can
 * move or scale what they just caught without a second trip to the palette.
 *
 * The hand-over has to be undone when the selection empties. Deleting what you lassoed left the
 * tool sitting on "select" — dragging did nothing, and the only way out was to pick the lasso
 * again. Coming back is not a nicety: nothing on screen says the tool changed under you.
 *
 * Only an automatic hand-over is reversed. Picking a tool by hand is a decision, and clearing a
 * selection must not undo it.
 */
export type LassoHandover = { tool: InkTool; returnTo: InkTool | null };

/**
 * A selection made while a brush is in hand came from the pen's side button acting as a lasso.
 * It hands over to the selection tool the same way, and gives the brush back when it empties.
 */
export function toolAfterSelection(
  tool: InkTool,
  selectedCount: number,
  returnTo: InkTool | null,
): LassoHandover {
  if (selectedCount > 0 && !keepsSelection(tool)) return { tool: "select", returnTo: tool };
  if (selectedCount > 0 && tool === "lasso") return { tool: "select", returnTo: "lasso" };
  if (selectedCount === 0 && returnTo && tool === "select") {
    return { tool: returnTo, returnTo: null };
  }
  return { tool, returnTo };
}

/**
 * Whether a selection means anything while this tool is active.
 *
 * A selection is only live while a selection tool is. A brush owns the whole page — an object
 * under the pointer is something to draw on, not something to grab — so a selection carried into
 * one would sit there looking live and doing nothing, which is the state people reported as
 * broken. Picking a brush drops it; picking the lasso or the selection tool keeps it.
 *
 * The same answer decides who receives a press. Both halves have to use this one function, or the
 * page shows handles it will not act on.
 */
export function keepsSelection(tool: InkTool): boolean {
  return tool === "lasso" || tool === "select";
}
