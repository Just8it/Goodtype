import type { PenTypeId } from "../settings";

/**
 * Writing instruments, drawn as one family.
 *
 * Every silhouette is drawn upright on the 24 grid with its tip at the bottom centre, then tilted
 * by `INSTRUMENT_TRANSFORM` so they all hold the same writing angle. What tells them apart is
 * what a hand would recognise first: the tip and the bulk of the body. A fountain pen's split
 * nib, a ballpoint's cone, a pencil's sharpened lead, a highlighter's broad chisel. The palette
 * and the nib picker share these, so the tile always shows the instrument the card selected.
 */
export const NIB_PATHS: Record<PenTypeId, string> = {
  fountain:
    "M10 2.5h4a1 1 0 0 1 1 1V11H9V3.5a1 1 0 0 1 1-1zM9.5 11.5h5c0 3.2-1.2 6.2-2.5 10-1.3-3.8-2.5-6.8-2.5-10zM12 15.4v2.8",
  ballpoint:
    "M10.3 2.5h3.4a.8.8 0 0 1 .8.8V14h-5V3.3a.8.8 0 0 1 .8-.8zM9.5 10.5h5M9.5 14l2 6.5h1l2-6.5",
  pencil: "M9.5 2.5h5V14h-5zM9.5 5h5M9.5 14l2.5 7.5 2.5-7.5M11.1 18.8h1.8",
  marker:
    "M9 2.5h6a1 1 0 0 1 1 1V12H8V3.5a1 1 0 0 1 1-1zM9.5 12h5v3h-5zM10.5 15h3v4.5a1.5 1.5 0 0 1-3 0z",
  technical: "M10 2.5h4V12h-4zM10 12l1.2 4h1.6l1.2-4M12 16v5.5",
};

/** Wider than any pen, with a slanted chisel: the body alone says "broad and flat". */
export const HIGHLIGHTER_PATH =
  "M8.5 2.5h7a1 1 0 0 1 1 1V11h-9V3.5a1 1 0 0 1 1-1zM9.5 11h5v3h-5zM10 14h4v3.5L10 21z";

/** A block with its worn edge down, the sleeve marked, at the same angle as the pens. */
export const ERASER_PATH = "M9 3.5h6a1 1 0 0 1 1 1V20a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V4.5a1 1 0 0 1 1-1zM8 14h8";

/**
 * Shrinks the silhouette a little so its tilted corners stay inside the tile, and brings its tip
 * down to the mark it leaves along the bottom edge.
 */
export const INSTRUMENT_TRANSFORM =
  "translate(1.5 0) rotate(45 12 12) translate(12 12) scale(0.88) translate(-12 -12)";
