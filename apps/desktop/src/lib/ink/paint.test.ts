import { describe, expect, it } from "vitest";
import type { Stroke } from "../model";
import { paintOrder } from "./paint";

function stroke(overrides: Partial<Stroke> = {}): Stroke {
  return {
    id: "stroke-1",
    tool: "pen",
    color: "#1e232b",
    widthPt: 2,
    pressure: true,
    taper: 0,
    opacity: 1,
    groupId: null,
    points: [
      { x: 10, y: 10, pressure: 0.5, timeMs: 0, tiltX: 0, tiltY: 0 },
      { x: 40, y: 12, pressure: 0.8, timeMs: 8, tiltX: 0, tiltY: 0 },
      { x: 70, y: 10, pressure: 0.6, timeMs: 16, tiltX: 0, tiltY: 0 },
    ],
    transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
    ...overrides,
  };
}

describe("paintOrder", () => {
  // The whole display layer rests on this: an ordinary stroke has to produce a fillable path.
  it("paints an ordinary stroke as one closed, filled shape", () => {
    const [painted] = paintOrder([stroke()]);
    expect(painted.d.startsWith("M ")).toBe(true);
    expect(painted.d.endsWith(" Z")).toBe(true);
    expect(painted).toMatchObject({ key: "stroke-1", color: "#1e232b", opacity: 1 });
  });

  it("merges a consecutive run of one opaque colour into a single path", () => {
    const painted = paintOrder([
      stroke({ id: "a" }),
      stroke({ id: "b" }),
      stroke({ id: "c" }),
    ]);
    expect(painted).toHaveLength(1);
    expect(painted[0].key).toBe("a");
    // Three subpaths, so the strokes stay three separate shapes inside the one node.
    expect(painted[0].d.match(/M /g)).toHaveLength(3);
  });

  it("keeps colours apart", () => {
    const painted = paintOrder([
      stroke({ id: "a", color: "#111111" }),
      stroke({ id: "b", color: "#e5645e" }),
    ]);
    expect(painted.map((path) => path.color)).toEqual(["#111111", "#e5645e"]);
  });

  // Merging across an intervening colour would put the third stroke underneath the second.
  it("does not merge across an intervening colour", () => {
    const painted = paintOrder([
      stroke({ id: "a", color: "#111111" }),
      stroke({ id: "b", color: "#e5645e" }),
      stroke({ id: "c", color: "#111111" }),
    ]);
    expect(painted.map((path) => path.key)).toEqual(["a", "b", "c"]);
  });

  // Two highlighter sweeps are meant to darken where they cross, exactly as the PDF renders them.
  it("never merges translucent ink", () => {
    const painted = paintOrder([
      stroke({ id: "a", tool: "highlighter", opacity: 0.6 }),
      stroke({ id: "b", tool: "highlighter", opacity: 0.6 }),
    ]);
    expect(painted).toHaveLength(2);
    expect(painted.every((path) => path.opacity === 0.6)).toBe(true);
  });

  it("skips a stroke with nothing to draw", () => {
    expect(paintOrder([stroke({ points: [] })])).toEqual([]);
  });

  it("floors hairline ink at the minimum width so it survives zooming out", () => {
    const hairline = stroke({ widthPt: 0.01, pressure: false });
    const [thin] = paintOrder([hairline]);
    const [floored] = paintOrder([hairline], 4);
    expect(floored.d).not.toBe(thin.d);
  });
});
