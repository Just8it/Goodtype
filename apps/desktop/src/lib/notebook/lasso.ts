import { translate, type Translator } from "../i18n";
import type { Point } from "../geometry/coordinates";
import { rotatedVector } from "../geometry/coordinates";
import type { Bounds } from "../ink/selection";
import { polylineInLasso } from "../ink/selection";
import { shapeBounds, splinePolyline } from "../shape/geometry";
import type { ShapeView } from "./pageView";

export type LassoTargets = { handwriting: boolean; highlighter: boolean; shapes: boolean; images: boolean; text: boolean };
export const DEFAULT_LASSO_TARGETS: LassoTargets = { handwriting: true, highlighter: true, shapes: true, images: true, text: true };
export function lassoOptions(t: Translator = translate) { return [
  { key: "handwriting", label: t("workspace.handwriting") },
  { key: "highlighter", label: t("workspace.highlighter") },
  { key: "shapes", label: t("workspace.shapes") },
  { key: "images", label: t("workspace.images") },
  { key: "text", label: t("workspace.textBlocks") },
] as const; }
export const LASSO_OPTIONS = lassoOptions();
export type LassoItem = { id: string; kind: "shapes" | "images" | "text"; outline: Point[]; closed: boolean };

export function boxOutline(x: number, y: number, width: number, height: number, rotation = 0): Point[] {
  return [{ x: 0, y: 0 }, { x: width, y: 0 }, { x: width, y: height }, { x: 0, y: height }]
    .map(point => { const local = rotatedVector(point, rotation); return { x: x + local.x, y: y + local.y }; });
}

export function shapeLassoItem(shape: ShapeView): LassoItem {
  const g = shape.geometry;
  const bounds = shapeBounds(g);
  const local = g.kind === "line" ? [g.start, g.end]
    : g.kind === "spline" ? splinePolyline(g.nodes, g.closed, 16)
    : g.kind === "ellipse" ? Array.from({ length: 64 }, (_, i) => ({
      x: g.widthPt * (1 + Math.cos(i * Math.PI / 32)) / 2,
      y: g.heightPt * (1 + Math.sin(i * Math.PI / 32)) / 2,
    })) : boxOutline(bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top);
  const angle = shape.rotation * Math.PI / 180;
  return { id: shape.id, kind: "shapes", closed: g.kind !== "line" && (g.kind !== "spline" || g.closed),
    outline: local.map(({ x, y }) => ({
      x: shape.x + shape.scale * (x * Math.cos(angle) - y * Math.sin(angle)),
      y: shape.y + shape.scale * (x * Math.sin(angle) + y * Math.cos(angle)),
    })) };
}

export function selectLassoObjects(items: LassoItem[], polygon: Point[], targets: LassoTargets): string[] {
  return items.filter((item) => targets[item.kind] && polylineInLasso(item.outline, polygon, item.closed)).map((item) => item.id);
}

export function combinedBounds(items: LassoItem[], ink: Bounds | null): Bounds | null {
  const points = items.flatMap((item) => item.outline);
  if (ink) points.push({ x: ink.left, y: ink.top }, { x: ink.right, y: ink.bottom });
  return points.length ? { left: Math.min(...points.map(p => p.x)), top: Math.min(...points.map(p => p.y)),
    right: Math.max(...points.map(p => p.x)), bottom: Math.max(...points.map(p => p.y)) } : null;
}

export function transformSelectionPoint(point: Point, delta: Point, scale: number, anchor: Point): Point {
  return { x: anchor.x + (point.x - anchor.x) * scale + delta.x, y: anchor.y + (point.y - anchor.y) * scale + delta.y };
}
