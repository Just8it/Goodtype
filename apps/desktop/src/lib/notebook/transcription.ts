// Turning a confirmed handwriting transcription into an ordinary Typst block (ADR 0040).
//
// The recognizer only proposes. The writer corrects the proposal beside the original ink, and
// what they confirm is inserted as a new block next to it; the ink itself is never touched.

import type { Stroke } from "../model";
import type { InkPolylines } from "../ipc/recognition";
import { transformedPoint, type Bounds } from "../ink/selection";
import { shapeOutline } from "../shape/geometry";
import { toPagePoint } from "../shape/hit";
import type { ShapeView } from "./pageView";

export type TranscriptionKind = "text" | "math";

/** The selected strokes in page points, through their own transforms, in drawing order. */
export function inkPolylines(strokes: Stroke[], ids: string[]): InkPolylines {
  const selected = new Set(ids);
  return strokes
    .filter((stroke) => selected.has(stroke.id) && stroke.points.length > 0)
    .map((stroke) => ({
      widthPt: stroke.widthPt * Math.max(Math.abs(stroke.transform.scaleX), Math.abs(stroke.transform.scaleY)),
      points: stroke.points.map((point) => {
        const { x, y } = transformedPoint(stroke, point);
        return [x, y] as [number, number];
      }),
    }));
}

/**
 * Selected shapes as outline polylines in page points. A fraction bar or root sign is often a
 * held line the shape tool straightened, and leaving it out would leave the formula without it.
 */
export function shapePolylines(shapes: ShapeView[], ids: string[]): InkPolylines {
  const selected = new Set(ids);
  return shapes
    .filter((shape) => selected.has(shape.id))
    .map((shape) => ({
      widthPt: shape.style.strokeWidthPt * Math.abs(shape.scale),
      points: shapeOutline(shape.geometry).map((point) => {
        const { x, y } = toPagePoint(shape, point);
        return [x, y] as [number, number];
      }),
    }))
    .filter((line) => line.points.length > 0);
}

export function polylineBounds(lines: InkPolylines): Bounds | null {
  let bounds: Bounds | null = null;
  for (const line of lines) {
    for (const [x, y] of line.points) {
      if (!bounds) bounds = { left: x, top: y, right: x, bottom: y };
      else {
        bounds.left = Math.min(bounds.left, x);
        bounds.top = Math.min(bounds.top, y);
        bounds.right = Math.max(bounds.right, x);
        bounds.bottom = Math.max(bounds.bottom, y);
      }
    }
  }
  return bounds;
}

/**
 * Plain prose as Typst markup that reads back literally. Characters with markup meaning are
 * escaped, line-start markers only where they would start a heading or list, and line breaks
 * become spaces: handwriting wraps where the page ended, not where a paragraph did.
 */
export function proseToTypst(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const escaped = flat.replace(/[\\#*_`$<>@[\]~/]/g, (symbol) => `\\${symbol}`).replace(/-(?=[-?])/g, "\\-");
  return escaped.replace(/^([=+-])/, "\\$1").replace(/^(\d+)\./, "$1\\.");
}

/** Display math. The writer edits the content; the delimiters are added here. */
export function mathToTypst(content: string): string {
  return `$ ${content.trim()} $`;
}

export function transcriptionSource(kind: TranscriptionKind, value: string): string {
  return kind === "math" ? mathToTypst(value) : proseToTypst(value);
}

/** Characters inserted, removed or replaced between the proposal and what was confirmed. */
export function editDistance(from: string, to: string): number {
  const a = [...from];
  const b = [...to];
  let previous = Array.from({ length: b.length + 1 }, (_, index) => index);
  for (let i = 1; i <= a.length; i += 1) {
    const current = [i];
    for (let j = 1; j <= b.length; j += 1) {
      current[j] = Math.min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    previous = current;
  }
  return previous[b.length];
}

const GAP_PT = 6;
const MIN_WIDTH_PT = 120;
const MARGIN_PT = 12;

/**
 * Directly below the handwriting, as wide as it, so the transcription lands where the writer is
 * looking. Kept on the page; a block that would start below the bottom edge goes above instead.
 */
export function transcriptionPlacement(
  bounds: Bounds,
  page: { widthPt: number; heightPt: number },
): { x: number; y: number; layoutWidthPt: number } {
  const x = Math.min(Math.max(bounds.left, MARGIN_PT), Math.max(MARGIN_PT, page.widthPt - MARGIN_PT - MIN_WIDTH_PT));
  const layoutWidthPt = Math.max(MIN_WIDTH_PT, Math.min(bounds.right - bounds.left, page.widthPt - MARGIN_PT - x));
  const below = bounds.bottom + GAP_PT;
  const y = below < page.heightPt - MARGIN_PT * 2 ? below : Math.max(MARGIN_PT, bounds.top - GAP_PT - 2 * MARGIN_PT);
  return { x, y, layoutWidthPt };
}
