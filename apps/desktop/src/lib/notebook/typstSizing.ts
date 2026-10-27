import { rotatedVector, type Point } from "../geometry/coordinates";
import type { TypstTransform } from "./pageView";

export const MIN_TYPST_SCALE = 0.1;
export const MAX_TYPST_SCALE = 8;
export const MIN_TYPST_WIDTH_PT = 72;
export const POINTS_PER_MM = 72 / 25.4;

export function typstScale(value: number): number {
  return Math.min(MAX_TYPST_SCALE, Math.max(MIN_TYPST_SCALE, value));
}

/** Project the page-space drag onto the rotated block diagonal; the opposite corner stays put. */
export function scaleTypstCorner(start: TypstTransform, heightPt: number, delta: Point): number {
  const local = rotatedVector(delta, -(start.rotation ?? 0));
  const width = start.layoutWidthPt;
  return typstScale(start.scale + (local.x * width + local.y * heightPt) / (width * width + heightPt * heightPt));
}

/** The field measures the visible line on paper; scale remains independent of reflow. */
export function typstWidthFromMm(widthMm: number, scale: number): number {
  return Math.max(MIN_TYPST_WIDTH_PT, widthMm * POINTS_PER_MM / scale);
}
