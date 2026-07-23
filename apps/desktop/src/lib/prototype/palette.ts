export type PaletteDock = "top" | "right" | "bottom" | "left";

export function nearestPaletteDock(
  x: number,
  y: number,
  width: number,
  height: number,
): PaletteDock {
  const horizontal = x <= width - x ? { dock: "left" as const, distance: x } : { dock: "right" as const, distance: width - x };
  const vertical = y <= height - y ? { dock: "top" as const, distance: y } : { dock: "bottom" as const, distance: height - y };
  return horizontal.distance <= vertical.distance ? horizontal.dock : vertical.dock;
}
