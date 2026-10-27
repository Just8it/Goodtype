/**
 * Pull past the last page to add one.
 *
 * Overscroll is measured as a raw distance and shown through a rubber band: the page follows the
 * pull less and less, so it reads as resistance rather than as scrolling into nothing. The ring
 * fills against the raw distance, not the shown one, so the effort it asks for stays the same
 * however the band is tuned. Pure, so the feel can be pinned by tests and tuned in one place.
 */

/** Raw overscroll that fills the ring for a finger or trackpad pull, in CSS pixels. */
export const PULL_FULL_PX = 150;

/**
 * The wheel asks for more. A notch is about 100 px of delta, so this is a deliberate three or four
 * notches past the end — never the tail of an ordinary scroll.
 */
export const PULL_FULL_WHEEL_PX = 360;

/** The furthest the page itself moves, however hard it is pulled. */
export const PULL_MAX_SHIFT_PX = 96;

/** How long the wheel may rest before an unfinished pull springs back, in milliseconds. */
export const PULL_WHEEL_IDLE_MS = 450;

export type PullSource = "touch" | "wheel";

/** How far the page visibly moves for a raw overscroll. Grows ever more slowly toward the cap. */
export function rubberBand(raw: number, max = PULL_MAX_SHIFT_PX): number {
  if (!(raw > 0)) return 0;
  return max * (1 - Math.exp(-raw / max));
}

/** 0 to 1: how full the ring is. */
export function pullProgress(raw: number, source: PullSource): number {
  const full = source === "wheel" ? PULL_FULL_WHEEL_PX : PULL_FULL_PX;
  return Math.min(1, Math.max(0, raw / full));
}

/** Whether the scroll position sits at the end, allowing a pixel of rounding. */
export function atScrollEnd(scrollTop: number, clientHeight: number, scrollHeight: number): boolean {
  return scrollTop + clientHeight >= scrollHeight - 1;
}

/** Wheel deltas in pixels, whatever unit the device reported them in. */
export function wheelPixels(deltaY: number, deltaMode: number, pageHeight: number): number {
  if (deltaMode === 1) return deltaY * 16;
  if (deltaMode === 2) return deltaY * pageHeight;
  return deltaY;
}
