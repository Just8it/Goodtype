/**
 * The maths behind `StepSlider`: where a value sits on the track, and what a nudge does.
 *
 * A pen cannot type a number, and a plain slider spends the same track length on every unit, so a
 * 0.05–20 mm width range left everyday nibs inside a few pixels. A logarithmic track gives each
 * doubling the same length instead, and the − / + buttons step exactly one `step` at a time.
 */

/** Track positions per slider; fine enough that a pixel never skips a representable value. */
export const TRACK_RESOLUTION = 1000;

export type SliderScale = { min: number; max: number; step: number; log?: boolean };

/** Track position (0 to TRACK_RESOLUTION) for a value. */
export function toPosition(value: number, { min, max, log }: SliderScale): number {
  const clamped = Math.min(Math.max(value, min), max);
  const fraction = log && min > 0
    ? Math.log(clamped / min) / Math.log(max / min)
    : (clamped - min) / (max - min);
  return Math.round(fraction * TRACK_RESOLUTION);
}

/**
 * Value for a track position. A log track keeps the step's decimals rather than its grid, so the
 * thin end can still land on 0.12 mm; a linear track snaps to the step as a plain slider does.
 */
export function fromPosition(position: number, scale: SliderScale): number {
  const { min, max, step, log } = scale;
  const fraction = Math.min(Math.max(position / TRACK_RESOLUTION, 0), 1);
  if (log && min > 0) {
    const precision = 10 ** decimals(step);
    return clampTo(Math.round(min * (max / min) ** fraction * precision) / precision, scale);
  }
  return snap(min + fraction * (max - min), scale);
}

/** One step up or down, onto the step grid, inside the range. */
export function nudge(value: number, direction: -1 | 1, scale: SliderScale): number {
  const { step } = scale;
  // Off-grid values (from a log drag) go to the neighbouring grid line, not one step past it.
  const grid = value / step;
  const target = direction > 0 ? Math.floor(grid + 1e-9) + 1 : Math.ceil(grid - 1e-9) - 1;
  return snap(target * step, scale);
}

function snap(value: number, scale: SliderScale): number {
  const { min, step } = scale;
  const stepped = min + Math.round((value - min) / step) * step;
  const precision = 10 ** decimals(step);
  return clampTo(Math.round(stepped * precision) / precision, scale);
}

function clampTo(value: number, { min, max }: SliderScale): number {
  return Math.min(Math.max(value, min), max);
}

function decimals(step: number): number {
  const text = String(step);
  const dot = text.indexOf(".");
  return dot === -1 ? 0 : text.length - dot - 1;
}
