// Where the canvas chrome sits, decided in one place.
//
// The palette, the history pill and the rest share one three-by-three grid over the canvas. Each
// of them used to name its own corner in CSS, which meant nothing was in a position to notice when
// two of them wanted the same one — and something had to, because the palette moves. A rail docked
// left landed on top of the undo buttons, and the left and right docks were hand-written to dodge
// different corners, so the same palette sat at two different heights depending on which side it
// was on.
//
// Stating the placements together fixes both by construction rather than by another special case,
// and lets a test assert the thing that actually matters: that no two of them ever overlap.

import type { PaletteDock } from "./palette";

/**
 * A placement in that grid, as CSS grid line numbers.
 *
 * Line numbers rather than track indices, so an area is a half-open interval on each axis and two
 * of them can be compared for overlap without knowing anything else about the grid.
 */
export type ChromeArea = {
  columnStart: number;
  columnEnd: number;
  rowStart: number;
  rowEnd: number;
  justify: "start" | "center" | "end";
  align: "start" | "center" | "end";
};

export type ChromeLayout = {
  palette: ChromeArea;
  history: ChromeArea;
  /** The corner the pill actually ends up in, which is what a drag should preview. */
  historyCorner: PlacedCorner;
};

export type PlacedCorner = Exclude<HistoryCorner, "auto">;

/**
 * Where the writer has parked the history pill, or `auto` to let it get out of the palette's way.
 *
 * `auto` is not "no preference" — it is the preference that the pill should keep away from the
 * rail on its own, which is what most people want and what happens until they say otherwise.
 */
export type HistoryCorner = "auto" | "top-left" | "top-right" | "bottom-left" | "bottom-right";

export const PLACED_CORNERS = [
  "top-left",
  "top-right",
  "bottom-left",
  "bottom-right",
] as const satisfies readonly PlacedCorner[];

export const HISTORY_CORNERS: readonly HistoryCorner[] = ["auto", ...PLACED_CORNERS];

/** The grid is three tracks each way, so the last line on either axis is the fourth. */
const LAST_LINE = 4;

/**
 * Where each piece of chrome goes for a given palette dock.
 *
 * The palette owns the edge it is docked to and spans it end to end. The history pill takes a
 * corner on a different edge, which is what keeps it out from under the rail — and because that
 * leaves the palette's own edge clear whichever side it is on, the left and right docks span the
 * same rows and come out at the same height, rather than each dodging a different corner.
 */
export function chromeLayout(
  dock: PaletteDock,
  history: HistoryCorner = "auto",
  crowded = false,
): ChromeLayout {
  const palette = paletteArea(dock);
  const preferred = history === "auto" ? automaticCorner(dock) : history;
  const corner = nearestFreeCorner(preferred, paletteClaim(dock, crowded));
  return { palette, history: cornerArea(corner), historyCorner: corner };
}
/**
 * What the palette needs, as opposed to the row it is drawn across.
 *
 * An end-docked rail is drawn across its whole row so that it centres on the canvas, but centred
 * with room to spare it does not reach the corners of that row — so the pill may still have one,
 * which is what makes a top corner available while the rail is on top.
 *
 * When the row is too tight for both — the side editor open and pulled right out — the rail needs
 * all of it, and the pill crosses to the other row instead of being cut into. Only what is claimed
 * changes; where the rail is drawn never does, so the pill giving way cannot shift it under the
 * pointer.
 */
export function paletteClaim(dock: PaletteDock, crowded = false): ChromeArea {
  const palette = paletteArea(dock);
  const horizontal = dock === "top" || dock === "bottom";
  return horizontal && !crowded ? { ...palette, columnStart: 2, columnEnd: 3 } : palette;
}

/**
 * The nearest corner to a point in the workspace.
 *
 * Corners rather than edges, because the pill is small and anchoring it to the middle of an edge
 * would leave it stranded in the canvas with nothing to sit against.
 */
export function nearestHistoryCorner(
  x: number,
  y: number,
  width: number,
  height: number,
): PlacedCorner {
  const bottom = y > height / 2;
  const right = x > width / 2;
  if (bottom) return right ? "bottom-right" : "bottom-left";
  return right ? "top-right" : "top-left";
}

/**
 * The palette's placement, which nothing else gets a say in.
 *
 * It is computed first and never adjusted for anything around it: the rail is what the writer
 * aims at and what they expect to find where they left it, so everything else on this layer works
 * around it rather than the other way about.
 */
function paletteArea(dock: PaletteDock): ChromeArea {
  switch (dock) {
    case "left":
      return fullColumn(1, "start");
    case "right":
      return fullColumn(3, "end");
    // An end dock is drawn across its whole row so that it centres on the canvas. Centring it in
    // the middle column instead puts it off to one side, because a grid column is as wide as the
    // widest thing in any of its rows — so the pill widens the column it sits in whichever row it
    // is parked in, and shoves the middle one over. What the rail *claims* is narrower than this;
    // see `claimedArea`.
    case "top":
      return acrossRow(1, "start");
    case "bottom":
      return acrossRow(3, "end");
  }
}

/// Where the pill sits when the writer has not placed it: the far top corner from a side rail.
function automaticCorner(dock: PaletteDock): PlacedCorner {
  return dock === "left" ? "top-right" : "top-left";
}

/**
 * The wanted corner if the palette leaves it free, else the closest one it does.
 *
 * Closest by how many axes have to change, so a pill pushed out of the bottom-left by a left-hand
 * rail slides across to the bottom-right rather than jumping to the top of the screen. A side dock
 * covers one column and an end dock one row, so two corners are always free.
 */
function nearestFreeCorner(preferred: PlacedCorner, palette: ChromeArea): PlacedCorner {
  const ordered = [...PLACED_CORNERS].sort(
    (left, right) => cornerDistance(preferred, left) - cornerDistance(preferred, right),
  );
  return ordered.find((corner) => !areasOverlap(palette, cornerArea(corner))) ?? preferred;
}

/// How many of the two axes differ: 0 for the same corner, 1 for a neighbour, 2 for the opposite.
function cornerDistance(from: PlacedCorner, to: PlacedCorner): number {
  const [fromRow, fromColumn] = from.split("-");
  const [toRow, toColumn] = to.split("-");
  return (fromRow === toRow ? 0 : 1) + (fromColumn === toColumn ? 0 : 1);
}

function cornerArea(corner: PlacedCorner): ChromeArea {
  const right = corner === "top-right" || corner === "bottom-right";
  const bottom = corner === "bottom-left" || corner === "bottom-right";
  const column = right ? 3 : 1;
  const row = bottom ? 3 : 1;
  return {
    columnStart: column,
    columnEnd: column + 1,
    rowStart: row,
    rowEnd: row + 1,
    justify: right ? "end" : "start",
    align: bottom ? "end" : "start",
  };
}

/** Whether two placements share any cell. Half-open intervals, so touching is not overlapping. */
export function areasOverlap(left: ChromeArea, right: ChromeArea): boolean {
  const columns = left.columnStart < right.columnEnd && right.columnStart < left.columnEnd;
  const rows = left.rowStart < right.rowEnd && right.rowStart < left.rowEnd;
  return columns && rows;
}

/** A whole end row, centred across it. */
function acrossRow(row: number, align: "start" | "end"): ChromeArea {
  return {
    columnStart: 1,
    columnEnd: LAST_LINE,
    rowStart: row,
    rowEnd: row + 1,
    justify: "center",
    align,
  };
}

/** The whole of one side column, centred down it. */
function fullColumn(column: number, justify: "start" | "end"): ChromeArea {
  return {
    columnStart: column,
    columnEnd: column + 1,
    rowStart: 1,
    rowEnd: LAST_LINE,
    justify,
    align: "center",
  };
}
