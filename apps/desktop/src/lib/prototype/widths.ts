/**
 * The stroke-width row on the palette.
 *
 * Pure, because the rules are worth pinning: the row must never empty, it must stay a ladder
 * rather than a list, and it must not grow past what the tiles can be told apart at. Those were
 * enforced in three places inside the component and true only by agreement between them.
 */

/** A width the row will keep, rounded to the precision a nib is specified at. */
function tidy(widthPt: number): number {
  return Math.round(widthPt * 1000) / 1000;
}

/**
 * Sorted, de-duplicated, and capped.
 *
 * Sorted because the row is read as a ladder, and one entry out of order makes it read as a list
 * of numbers instead. De-duplicated because two tiles drawing the same line are indistinguishable
 * — editing one to match another is a way of asking for fewer, not for a twin.
 */
export function normaliseWidths(widths: number[], maximum: number): number[] {
  return [...new Set(widths.map(tidy))].sort((a, b) => a - b).slice(0, maximum);
}

export function addWidth(widths: number[], widthPt: number, maximum: number): number[] {
  if (widths.length >= maximum && !widths.map(tidy).includes(tidy(widthPt))) return widths;
  return normaliseWidths([...widths, widthPt], maximum);
}

export function editWidth(
  widths: number[],
  index: number,
  widthPt: number,
  maximum: number,
): number[] {
  return normaliseWidths(
    widths.map((existing, position) => (position === index ? widthPt : existing)),
    maximum,
  );
}

/**
 * Whether the row can give one up.
 *
 * The last width may never be removed. A tool with no width has nothing to draw with, and the
 * palette would offer an empty row with no way to refill it — the add tile having been retired
 * when the row was full is no help once the row is empty.
 */
export function canRemoveWidth(widths: number[]): boolean {
  return widths.length > 1;
}

/** Removing the last one is refused, and the row comes back unchanged. */
export function removeWidth(widths: number[], index: number): number[] {
  if (!canRemoveWidth(widths)) return widths;
  return widths.filter((_, position) => position !== index);
}
