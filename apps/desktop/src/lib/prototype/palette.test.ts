import { describe, expect, it } from "vitest";
import { nearestPaletteDock, PALETTE_TOOLS } from "./palette";

describe("palette docking", () => {
  it("chooses the nearest workspace edge", () => {
    expect(nearestPaletteDock(10, 300, 800, 600)).toBe("left");
    expect(nearestPaletteDock(790, 300, 800, 600)).toBe("right");
    expect(nearestPaletteDock(400, 10, 800, 600)).toBe("top");
    expect(nearestPaletteDock(400, 590, 800, 600)).toBe("bottom");
  });
});

describe("palette tools", () => {
  it("keeps built-in command ids unique", () => {
    const ids = PALETTE_TOOLS.map((tool) => tool.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
