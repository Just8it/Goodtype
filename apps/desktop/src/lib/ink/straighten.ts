// Straightening a stroke that was meant to be straight.
//
// This runs once, on release, before the stroke is committed — so what gets stored is already the
// straightened geometry. Nothing downstream has to know: export, reopen, and undo all see an
// ordinary two-point stroke. That is deliberate. A "straighten" flag interpreted at render time
// would be a second place where the screen and the PDF could disagree, which is exactly the class
// of bug the shared outline geometry exists to rule out.
//
// It is also why this has no Rust mirror: unlike `outline.ts`, it never runs at export time.

import type { StrokePoint } from "../model";

export type StraightenOptions = {
  /** Largest deviation from the chord that still counts as straight, as a fraction of its length. */
  tolerance: number;
  /** Snap the result to multiples of this angle, in degrees; 0 keeps the drawn angle. */
  snapDegrees: number;
  /** Strokes shorter than this are taps and flicks, not lines, and are left alone. */
  minimumLengthPt: number;
};

export const DEFAULT_STRAIGHTEN: StraightenOptions = {
  // Loose enough that an unsteady hand still reads as a line, tight enough that a deliberate
  // curve survives. A 200pt underline may wander 8pt and still snap.
  tolerance: 0.04,
  snapDegrees: 15,
  minimumLengthPt: 24,
};

/**
 * How far a stroke wanders off the straight line between its own endpoints, in points, together
 * with the length of that line. Exported for the same reason it is tested: the threshold is a
 * judgement call, and it should be possible to see what it is judging.
 */
export function chordDeviation(points: StrokePoint[]): { deviationPt: number; chordPt: number } {
  if (points.length < 2) return { deviationPt: 0, chordPt: 0 };
  const first = points[0];
  const last = points[points.length - 1];
  const dx = last.x - first.x;
  const dy = last.y - first.y;
  const chordPt = Math.hypot(dx, dy);
  if (chordPt === 0) return { deviationPt: 0, chordPt: 0 };

  let deviationPt = 0;
  for (const point of points) {
    // Perpendicular distance to the infinite line through the endpoints, via the cross product.
    const cross = Math.abs(dx * (point.y - first.y) - dy * (point.x - first.x));
    deviationPt = Math.max(deviationPt, cross / chordPt);
  }
  return { deviationPt, chordPt };
}

/**
 * Replaces a stroke that is already almost straight with the line it was trying to be, optionally
 * snapped to a fixed angle. Anything that is genuinely curved — or too short to have an intended
 * direction — is returned untouched, so this can be left on without eating handwriting.
 */
export function straightenStroke(
  points: StrokePoint[],
  options: StraightenOptions = DEFAULT_STRAIGHTEN,
): StrokePoint[] {
  if (points.length < 3) return points;

  const { deviationPt, chordPt } = chordDeviation(points);
  if (chordPt < options.minimumLengthPt) return points;
  if (deviationPt > chordPt * options.tolerance) return points;

  // A stroke that doubles back sits close to the chord while being nothing like a line, so the
  // perpendicular test alone would happily collapse a scribble. Travelled distance catches it.
  let travelled = 0;
  for (let index = 1; index < points.length; index += 1) {
    travelled += Math.hypot(
      points[index].x - points[index - 1].x,
      points[index].y - points[index - 1].y,
    );
  }
  if (travelled > chordPt * 1.2) return points;

  const first = points[0];
  const last = points[points.length - 1];
  let angle = Math.atan2(last.y - first.y, last.x - first.x);
  if (options.snapDegrees > 0) {
    const step = (options.snapDegrees * Math.PI) / 180;
    angle = Math.round(angle / step) * step;
  }

  return [
    first,
    {
      ...last,
      x: first.x + Math.cos(angle) * chordPt,
      y: first.y + Math.sin(angle) * chordPt,
    },
  ];
}
