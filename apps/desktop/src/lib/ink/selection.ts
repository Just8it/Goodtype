import type { Point } from "../geometry/coordinates";
import type { Stroke, StrokePoint } from "../model";
import type { InkTool } from "./pipeline";

export type Bounds = {
  left: number;
  top: number;
  right: number;
  bottom: number;
};

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
  if (stroke.points.length === 0) return null;
  return pointsBounds(stroke.points.map((point) => transformedPoint(stroke, point)));
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

export function selectStrokesInLasso(strokes: Stroke[], polygon: Point[]): string[] {
  if (polygon.length < 3) return [];
  return strokes
    .filter((stroke) =>
      stroke.points.some((point) => pointInPolygon(transformedPoint(stroke, point), polygon)),
    )
    .map((stroke) => stroke.id);
}

export function hitStroke(strokes: Stroke[], point: Point, radius: number): Stroke | null {
  for (let strokeIndex = strokes.length - 1; strokeIndex >= 0; strokeIndex -= 1) {
    const stroke = strokes[strokeIndex];
    const points = stroke.points.map((sample) => transformedPoint(stroke, sample));
    const hitRadius =
      Math.max(radius, (stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY)) / 2);
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
): Stroke[] {
  const hit = hitStroke(strokes, point, radius);
  return hit ? strokes.filter((stroke) => stroke.id !== hit.id) : strokes;
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
  return {
    left: Math.min(...points.map((point) => point.x)),
    top: Math.min(...points.map((point) => point.y)),
    right: Math.max(...points.map((point) => point.x)),
    bottom: Math.max(...points.map((point) => point.y)),
  };
}

function pointInPolygon(point: Point, polygon: Point[]): boolean {
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

function distanceToSegment(point: Point, start: Point, end: Point): number {
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
export type LassoHandover = { tool: InkTool; handedOver: boolean };

export function toolAfterSelection(
  tool: InkTool,
  selectedCount: number,
  handedOver: boolean,
): LassoHandover {
  if (selectedCount > 0 && tool === "lasso") return { tool: "select", handedOver: true };
  if (selectedCount === 0 && handedOver && tool === "select") {
    return { tool: "lasso", handedOver: false };
  }
  return { tool, handedOver };
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
