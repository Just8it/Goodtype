import { describe, expect, it } from "vitest";
import { nearestPaletteDock, paletteTransportPosition, PALETTE_TOOLS } from "./palette";

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

describe("palette tools", () => {
  it("keeps built-in command ids unique", () => {
    const ids = PALETTE_TOOLS.map((tool) => tool.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
