import { translate, type Translator } from "../i18n";
export type PaletteDock = "top" | "right" | "bottom" | "left";

export type PaletteCommand =
  | "pen-1"
  | "pen-2"
  | "highlighter"
  | "shape"
  | "eraser"
  | "lasso"
  | "page-text"
  | "typst-block";

/**
 * What a tile does, which is what the rail is divided by: freehand marking, then the tools that
 * act on the page, then the document ones.
 */
export type PaletteGroup = "marking" | "page" | "content";

export type PaletteTool = {
  id: PaletteCommand;
  label: string;
  title: string;
  group: PaletteGroup;
  context?: "ink" | "shape" | "eraser" | "lasso";
  action?: boolean;
};

const WIDTH_MATCH_EPSILON = 0.001;

/**
 * Find the preset that represents the live width without pretending a nearby preset is active.
 * The small tolerance absorbs point/mm round-trip noise while preserving bespoke values.
 */
export function paletteWidthIndex(widths: readonly number[], activeWidth: number): number {
  return widths.findIndex((width) => Math.abs(width - activeWidth) < WIDTH_MATCH_EPSILON);
}

// Presentation metadata only. Input behavior stays explicit in PagePrototype because a tool must
// not gain pen, filesystem, or notebook authority merely by appearing in this list.
export function paletteTools(t: Translator = translate): readonly PaletteTool[] { return [
  { id: "pen-1", label: t("workspace.pen3"), title: t("workspace.penPressAgainForSettings"), group: "marking", context: "ink" },
  { id: "pen-2", label: t("workspace.pen4"), title: t("workspace.penPressAgainForSettings2"), group: "marking", context: "ink" },
  { id: "highlighter", label: t("workspace.highlighter"), title: t("workspace.highlighterPressAgainForSettings"), group: "marking", context: "ink" },
  { id: "eraser", label: t("workspace.eraser"), title: t("workspace.eraseInkAndShapesPressAgainFor"), group: "marking", context: "eraser" },
  // Freehand marking above, deliberate construction below: the lasso and the shape tool both act
  // on the page rather than laying ink down on it. This also puts the tiles in the order their
  // number keys are in.
  { id: "lasso", label: t("workspace.lassoSelect"), title: t("workspace.lassoSelectPressAgainForSettings"), group: "page", context: "lasso" },
  { id: "shape", label: t("workspace.shapes"), title: t("workspace.shapesPressAgainToChooseAShape"), group: "page", context: "shape" },
  { id: "page-text", label: t("workspace.pageText"), title: t("workspace.pageText"), group: "content" },
  { id: "typst-block", label: t("workspace.newTypstBlock"), title: t("workspace.newTypstBlockT"), group: "content", action: true },
]; }
export const PALETTE_TOOLS = paletteTools();

/**
 * The tiles the writer kept on the bar, each marked with whether a divider leads it.
 *
 * Dividers are derived rather than placed: one sits wherever two groups meet among the *visible*
 * tiles, so hiding the last tool of a group can never leave two dividers side by side or one
 * dangling at the end of the rail.
 */
export function visiblePaletteTools(
  hidden: readonly PaletteCommand[],
  t: Translator = translate,
): { tool: PaletteTool; dividerBefore: boolean }[] {
  const shown = paletteTools(t).filter((tool) => !hidden.includes(tool.id));
  return shown.map((tool, index) => ({
    tool,
    dividerBefore: index > 0 && shown[index - 1].group !== tool.group,
  }));
}

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
