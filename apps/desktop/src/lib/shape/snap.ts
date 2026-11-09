import type { Point } from "../geometry/coordinates";
import type { ShapeView } from "../notebook/pageView";
import { toPagePoint } from "./hit";

/** How close, in screen points, a dragged end has to come to another end before it meets it. */
export const SNAP_RADIUS_SCREEN_PT = 10;

/**
 * Every point a dragged line end or rectangle corner may meet, on the page: both ends of each
 * line, the four corners of each rectangle, and the loose ends of each open spline. Ellipses and
 * closed splines have no ends to meet.
 */
export function snapTargets(shapes: readonly ShapeView[], excludeId?: string): Point[] {
  return shapes.flatMap((shape) => {
    if (shape.id === excludeId) return [];
    const geometry = shape.geometry;
    switch (geometry.kind) {
      case "line":
        return [toPagePoint(shape, geometry.start), toPagePoint(shape, geometry.end)];
      case "rectangle": {
        const { widthPt: w, heightPt: h } = geometry;
        return [{ x: 0, y: 0 }, { x: w, y: 0 }, { x: w, y: h }, { x: 0, y: h }]
          .map((corner) => toPagePoint(shape, corner));
      }
      case "spline": {
        const first = geometry.nodes[0];
        const last = geometry.nodes.at(-1);
        if (geometry.closed || !first || !last) return [];
        return [toPagePoint(shape, first.point), toPagePoint(shape, last.point)];
      }
      case "ellipse":
        return [];
    }
  });
}

/** The target nearest to `point` within `radiusPt`, or null when none is that close. */
export function snapPoint(point: Point, targets: readonly Point[], radiusPt: number): Point | null {
  let found: Point | null = null;
  let best = radiusPt;
  for (const target of targets) {
    const distance = Math.hypot(target.x - point.x, target.y - point.y);
    if (distance <= best) {
      found = target;
      best = distance;
    }
  }
  return found;
}
