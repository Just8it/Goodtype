import type { Point } from "../geometry/coordinates";
import type { Stroke } from "../model";
import { moveSelected, scaleSelected, selectionBounds, type Bounds } from "./selection";
import { transformSelectionPoint } from "../prototype/lasso";

export type InkPreview = { ids: string[]; delta: Point; scale: number; anchor: Point };
export function previewMatrix(preview: InkPreview): string {
  const { scale, anchor, delta } = preview;
  return `translate(${delta.x} ${delta.y}) translate(${anchor.x} ${anchor.y}) scale(${scale}) translate(${-anchor.x} ${-anchor.y})`;
}
export function previewBounds(bounds: Bounds | null, preview: InkPreview | null): Bounds | null {
  if (!bounds || !preview) return bounds;
  const a = transformSelectionPoint({ x: bounds.left, y: bounds.top }, preview.delta, preview.scale, preview.anchor);
  const b = transformSelectionPoint({ x: bounds.right, y: bounds.bottom }, preview.delta, preview.scale, preview.anchor);
  return { left: a.x, top: a.y, right: b.x, bottom: b.y };
}
export function finishInkPreview(strokes: Stroke[], preview: InkPreview): Stroke[] {
  return moveSelected(scaleSelected(strokes, preview.ids, preview.anchor, preview.scale), preview.ids, preview.delta);
}
export function inkPreviewBounds(strokes: Stroke[], ids: string[], preview: InkPreview | null) {
  return previewBounds(selectionBounds(strokes, ids), preview);
}
