export type PaletteDock = "top" | "right" | "bottom" | "left";

export type PaletteCommand =
  | "pen-1"
  | "pen-2"
  | "highlighter"
  | "eraser"
  | "lasso"
  | "page-text"
  | "typst-block";

export type PaletteTool = {
  id: PaletteCommand;
  label: string;
  title: string;
  dividerBefore?: boolean;
  context?: "ink" | "eraser";
  action?: boolean;
};

// Presentation metadata only. Input behavior stays explicit in PagePrototype because a tool must
// not gain pen, filesystem, or notebook authority merely by appearing in this list.
export const PALETTE_TOOLS: readonly PaletteTool[] = [
  { id: "pen-1", label: "Pen 1", title: "Pen 1 (1) — press again for settings", context: "ink" },
  { id: "pen-2", label: "Pen 2", title: "Pen 2 (2) — press again for settings", context: "ink" },
  { id: "highlighter", label: "Highlighter", title: "Highlighter (3) — press again for settings", context: "ink" },
  { id: "eraser", label: "Eraser", title: "Erase whole strokes (4) — press again for size", context: "eraser" },
  { id: "lasso", label: "Lasso select", title: "Select ink with lasso (5)", dividerBefore: true },
  { id: "page-text", label: "Page text", title: "Page text", dividerBefore: true },
  { id: "typst-block", label: "New Typst block", title: "New Typst block (T)", action: true },
];

export function nearestPaletteDock(
  x: number,
  y: number,
  width: number,
  height: number,
): PaletteDock {
  const horizontal =
    x <= width - x
      ? { dock: "left" as const, distance: x }
      : { dock: "right" as const, distance: width - x };
  const vertical =
    y <= height - y
      ? { dock: "top" as const, distance: y }
      : { dock: "bottom" as const, distance: height - y };
  return horizontal.distance <= vertical.distance ? horizontal.dock : vertical.dock;
}

/** Keep the compact palette transport puck centred under the pointer and inside the workspace. */
export function paletteTransportPosition(
  pointerX: number,
  pointerY: number,
  workspaceWidth: number,
  workspaceHeight: number,
  size: number,
  margin = 8,
): { x: number; y: number } {
  const maximumX = Math.max(workspaceWidth - size - margin, margin);
  const maximumY = Math.max(workspaceHeight - size - margin, margin);
  return {
    x: Math.min(Math.max(pointerX - size / 2, margin), maximumX),
    y: Math.min(Math.max(pointerY - size / 2, margin), maximumY),
  };
}
