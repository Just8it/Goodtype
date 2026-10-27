import { describe, expect, it } from "vitest";
import {
  nearestPaletteDock,
  paletteTransportPosition,
  paletteWidthIndex,
  PALETTE_TOOLS,
  visiblePaletteTools,
} from "./palette";

describe("palette docking", () => {
  it("chooses the nearest workspace edge", () => {
    expect(nearestPaletteDock(10, 300, 800, 600)).toBe("left");
    expect(nearestPaletteDock(790, 300, 800, 600)).toBe("right");
    expect(nearestPaletteDock(400, 10, 800, 600)).toBe("top");
    expect(nearestPaletteDock(400, 590, 800, 600)).toBe("bottom");
  });

  it("keeps the compact transport puck under the pointer without leaving the workspace", () => {
    expect(paletteTransportPosition(400, 300, 800, 600, 44)).toEqual({ x: 378, y: 278 });
    expect(paletteTransportPosition(0, 0, 800, 600, 44)).toEqual({ x: 8, y: 8 });
    expect(paletteTransportPosition(800, 600, 800, 600, 44)).toEqual({ x: 748, y: 548 });
  });
});

function railGroups(hidden: Parameters<typeof visiblePaletteTools>[0]): string[][] {
  const groups: string[][] = [];
  for (const { tool, dividerBefore } of visiblePaletteTools(hidden)) {
    if (dividerBefore || groups.length === 0) groups.push([]);
    groups[groups.length - 1].push(tool.id);
  }
  return groups;
}

describe("palette tools", () => {
  it("keeps built-in command ids unique", () => {
    const ids = PALETTE_TOOLS.map((tool) => tool.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("keeps a custom width distinct from nearby presets", () => {
    const widths = [1, 2, 4];

    expect(paletteWidthIndex(widths, 2.0004)).toBe(1);
    expect(paletteWidthIndex(widths, 2.4)).toBe(-1);
  });

  it("groups the tiles by what they do, in the order their keys are", () => {
    // Dividers are the only thing that groups the rail, so the grouping is asserted through them:
    // freehand marking, then the tools that act on the page, then the document ones.
    expect(railGroups([])).toEqual([
      ["pen-1", "pen-2", "highlighter", "eraser"],
      ["lasso", "shape"],
      ["page-text", "typst-block"],
    ]);
  });

  it("drops the divider of a group the writer hid entirely", () => {
    expect(railGroups(["lasso", "shape", "typst-block"])).toEqual([
      ["pen-1", "pen-2", "highlighter", "eraser"],
      ["page-text"],
    ]);
    expect(railGroups(["pen-1", "pen-2", "highlighter", "eraser"])).toEqual([
      ["lasso", "shape"],
      ["page-text", "typst-block"],
    ]);
  });
});
