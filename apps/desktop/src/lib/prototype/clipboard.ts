/**
 * Copy, cut, paste and duplicate (specs/qol-wave.md section 3).
 *
 * A bundle is self-contained: ink and shapes by value, Typst blocks by their source text, images
 * by their original bytes. Nothing in it points back at the files it came from, so it can land on
 * another page or in another notebook, and a pasted copy can never share a block file or an asset
 * with its original. Every pasted item gets a fresh identity.
 */

import type { Point } from "../geometry/coordinates";
import { selectionBounds } from "../ink/selection";
import type { Stroke } from "../model";
import { boxOutline, shapeLassoItem } from "./lasso";
import type { ShapeView } from "./pageView";

/** How far a paste moves its contents, in page points. */
export type PasteShift = { dx: number; dy: number };

export type ClipboardBlock = {
  source: string;
  x: number;
  y: number;
  layoutWidthPt: number;
  /** Rendered height before `scale`, as measured when copied; it only centres a paste. */
  heightPt: number;
  scale: number;
  rotation?: number;
};

export type ClipboardImage = {
  bytes: Uint8Array;
  mime: string;
  alt: string;
  x: number;
  y: number;
  widthPt: number;
  heightPt: number;
  scale: number;
  rotation?: number;
};

export type ClipboardBundle = {
  /** Identifies this copy, so a paste can tell it apart from something copied elsewhere since. */
  token: string;
  /** The page it was copied on; pasting back onto it offsets so the copy does not hide the original. */
  pageId: string;
  strokes: Stroke[];
  shapes: ShapeView[];
  blocks: ClipboardBlock[];
  images: ClipboardImage[];
};

/** How far each paste onto the same page moves, in points: visibly apart, still close by. */
export const PASTE_STEP_PT = 12;

export function isEmptyBundle(bundle: ClipboardBundle): boolean {
  return (
    bundle.strokes.length === 0 &&
    bundle.shapes.length === 0 &&
    bundle.blocks.length === 0 &&
    bundle.images.length === 0
  );
}

/** The text written to the system clipboard so a later paste can recognise this copy. */
export function clipboardMarker(bundle: ClipboardBundle): string {
  return `goodtype-clipboard:${bundle.token}`;
}

/**
 * Offset for the `nth` paste (1-based) of a bundle onto `pageId`: stepped on its own page so
 * repeated pastes fan out, untouched elsewhere so a copy lands where it sat on the other page.
 */
export function pasteOffset(bundle: ClipboardBundle, pageId: string, nth: number): PasteShift {
  const step = bundle.pageId === pageId ? PASTE_STEP_PT * Math.max(1, nth) : 0;
  return { dx: step, dy: step };
}

/** The middle of the transformed selection in page points, or null for an empty bundle. */
export function bundleCenter(bundle: ClipboardBundle): Point | null {
  const points: Point[] = [];
  const ink = selectionBounds(bundle.strokes, bundle.strokes.map(stroke => stroke.id));
  if (ink) points.push({ x: ink.left, y: ink.top }, { x: ink.right, y: ink.bottom });
  for (const shape of bundle.shapes) points.push(...shapeLassoItem(shape).outline);
  for (const block of bundle.blocks) points.push(...boxOutline(block.x, block.y,
    block.layoutWidthPt * block.scale, block.heightPt * block.scale, block.rotation));
  for (const image of bundle.images) points.push(...boxOutline(image.x, image.y,
    image.widthPt * image.scale, image.heightPt * image.scale, image.rotation));
  if (!points.length) return null;
  return { x: (Math.min(...points.map(p => p.x)) + Math.max(...points.map(p => p.x))) / 2,
    y: (Math.min(...points.map(p => p.y)) + Math.max(...points.map(p => p.y))) / 2 };
}

/** The shift that puts the bundle's middle on `point`. */
export function shiftTo(bundle: ClipboardBundle, point: Point): PasteShift {
  const center = bundleCenter(bundle);
  return center ? { dx: point.x - center.x, dy: point.y - center.y } : { dx: 0, dy: 0 };
}

/** Ink as it will be pasted: fresh ids, ungrouped, moved by `shift`, stacked from `zIndex`. */
export function pastedStrokes(
  strokes: Stroke[],
  shift: PasteShift,
  firstZIndex: number,
  newId: () => string,
): Stroke[] {
  return strokes.map((stroke, index) => ({
    ...stroke,
    id: newId(),
    zIndex: firstZIndex + index,
    groupId: null,
    transform: {
      ...stroke.transform,
      translateX: stroke.transform.translateX + shift.dx,
      translateY: stroke.transform.translateY + shift.dy,
    },
  }));
}

/** Shapes as they will be pasted: fresh ids, moved by `shift`, stacked and read after the rest. */
export function pastedShapes(
  shapes: ShapeView[],
  shift: PasteShift,
  firstZIndex: number,
  firstReadingOrder: number,
  newId: () => string,
): ShapeView[] {
  return shapes.map((shape, index) => ({
    ...shape,
    id: newId(),
    x: shape.x + shift.dx,
    y: shape.y + shift.dy,
    zIndex: firstZIndex + index,
    readingOrder: firstReadingOrder + index,
  }));
}
