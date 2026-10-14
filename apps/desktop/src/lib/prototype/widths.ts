import type { StrokePattern } from "../model";

/**
 * The stroke-width row on the palette.
 *
 * Pure, because the rules are worth pinning: the row must never empty, it must stay a ladder
 * rather than a list, and it must not grow past what the tiles can be told apart at. Those were
 * enforced in three places inside the component and true only by agreement between them.
 */

/** Millimetre limits that stay inside Rust's persisted point ranges after UI rounding. */
export const WIDTH_BOUNDS_MM = {
  pen: { minimum: 0.1, maximum: 4 },
  highlighter: { minimum: 0.4, maximum: 7 },
} as const;

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

/**
 * Line patterns belong to a width chip, not to the pen.
 *
 * Making a pen dotted used to make every width on its row dotted. Each chip now carries its own
 * pattern, stored by width because the row never holds the same width twice. A width with no
 * entry is solid, which is also what every row written before this looks like.
 */
export type WidthPatterns = Record<string, StrokePattern>;

function patternKey(widthPt: number): string {
  return String(tidy(widthPt));
}

export function patternForWidth(patterns: WidthPatterns, widthPt: number): StrokePattern {
  return patterns[patternKey(widthPt)] ?? "solid";
}

export function withWidthPattern(
  patterns: WidthPatterns,
  widthPt: number,
  pattern: StrokePattern,
): WidthPatterns {
  const next = { ...patterns };
  if (pattern === "solid") delete next[patternKey(widthPt)];
  else next[patternKey(widthPt)] = pattern;
  return next;
}

/** Only widths still on the row keep a pattern; a removed chip takes its pattern with it. */
export function prunePatterns(patterns: WidthPatterns, widths: number[]): WidthPatterns {
  const kept = new Set(widths.map(patternKey));
  return Object.fromEntries(Object.entries(patterns).filter(([key]) => kept.has(key)));
}

/** An edited chip keeps its pattern at its new width. */
export function movePattern(patterns: WidthPatterns, fromPt: number, toPt: number): WidthPatterns {
  return withWidthPattern(
    withWidthPattern(patterns, fromPt, "solid"),
    toPt,
    patternForWidth(patterns, fromPt),
  );
}

/**
 * Gap factor per chip, alongside its pattern and kept by the same rules. 1 is the default and is
 * never stored, so a row nobody has adjusted carries an empty map.
 */
export type WidthSpacings = Record<string, number>;

export function spacingForWidth(spacings: WidthSpacings, widthPt: number): number {
  return spacings[patternKey(widthPt)] ?? 1;
}

export function withWidthSpacing(
  spacings: WidthSpacings,
  widthPt: number,
  spacing: number,
): WidthSpacings {
  const next = { ...spacings };
  if (spacing === 1) delete next[patternKey(widthPt)];
  else next[patternKey(widthPt)] = spacing;
  return next;
}

export function pruneSpacings(spacings: WidthSpacings, widths: number[]): WidthSpacings {
  const kept = new Set(widths.map(patternKey));
  return Object.fromEntries(Object.entries(spacings).filter(([key]) => kept.has(key)));
}

export function moveSpacing(spacings: WidthSpacings, fromPt: number, toPt: number): WidthSpacings {
  return withWidthSpacing(
    withWidthSpacing(spacings, fromPt, 1),
    toPt,
    spacingForWidth(spacings, fromPt),
  );
}
