import { describe, expect, it } from "vitest";
import type { Stroke } from "../model";
import { paintOrder, paintStrata } from "./paint";

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
  it("reuses unchanged strata across zoom tiers but invalidates narrow ink and object order", () => {
    const strokes = [stroke()];
    const first = paintStrata(strokes, [], 0.5);
    expect(paintStrata(strokes, [], 0.25)).toBe(first);
    expect(paintStrata(strokes, [], 0.25)).toEqual([{ key: "stroke-1", zIndex: 1_000_000, paths: paintOrder(strokes) }]);
    expect(paintStrata(strokes, [2], 0.25)).not.toBe(first);
    const hairlines = [stroke({ widthPt: 0.1 })];
    expect(paintStrata(hairlines, [], 0.5)).not.toBe(paintStrata(hairlines, [], 0.25));
    expect(paintStrata(hairlines, [], 0.25)[0].paths).toEqual(paintOrder(hairlines, 0.25));
  });
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
    // Each rounded stroke is compound geometry; all three still share just one painted node.
    expect(painted[0].d.match(/M /g)!.length).toBeGreaterThan(3);
  });

  it("keeps colours apart", () => {
    const painted = paintOrder([
      stroke({ id: "a", color: "#111111" }),
      stroke({ id: "b", color: "#e5645e" }),
    ]);
    expect(painted.map((path) => path.color)).toEqual(["#111111", "#e5645e"]);
  });

  it("paints dashed and dotted ink as separated filled subpaths", () => {
    const [dashed, dotted] = paintOrder([
      stroke({ id: "dash", pressure: false, pattern: "dashed" }),
      stroke({ id: "dot", pressure: false, pattern: "dotted", color: "#e5645e" }),
    ]);
    expect(dashed.d.match(/M /g)?.length).toBeGreaterThan(3);
    expect(dotted.d.match(/M /g)?.length).toBeGreaterThan(3);
    expect(dashed.d).not.toContain("NaN");
    expect(dotted.d).not.toContain("NaN");
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

  it("rounds pen caps while keeping highlighter bands flat", () => {
    const points = [{ x: 0, y: 0, pressure: 1, timeMs: 0, tiltX: 0, tiltY: 0 },
      { x: 10, y: 0, pressure: 1, timeMs: 10, tiltX: 0, tiltY: 0 }];
    const [pen] = paintOrder([stroke({ points, pressure: false, tool: "pen" })]);
    const [highlighter] = paintOrder([stroke({ points, pressure: false, tool: "highlighter" })]);
    expect(pen.d.match(/M /g)).toHaveLength(3);
    expect(highlighter.d.match(/M /g)).toHaveLength(1);
    expect(highlighter.d).toBe("M 0 1 L 10 1 L 10 -1 L 0 -1 Z");
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

  it("batches ink only until a movable object separates its visual order", () => {
    const strata = paintStrata(
      [stroke({ id: "below", zIndex: 1 }), stroke({ id: "above", zIndex: 3 })],
      [2],
    );
    expect(strata.map(({ key, zIndex }) => ({ key, zIndex }))).toEqual([
      { key: "below", zIndex: 1 },
      { key: "above", zIndex: 3 },
    ]);
  });

  it("splits ink at fixed Page text so behind highlights cannot pull pen ink underneath", () => {
    const strata = paintStrata([
      stroke({ id: "pen", zIndex: 3 }),
      stroke({ id: "sweep", zIndex: -1, tool: "highlighter", opacity: 0.6 }),
    ], [0, 2]);
    expect(strata.map(({ key, zIndex }) => ({ key, zIndex }))).toEqual([
      { key: "sweep", zIndex: -1 }, { key: "pen", zIndex: 3 },
    ]);
  });
});

describe("live ink under the precise eraser", () => {
  const moved = (id: string, dy: number) => stroke({ id, points: stroke().points.map((point) => ({ ...point, y: point.y + dy })) });

  it("paints live strokes as runs of their own and leaves the other runs' strings untouched", () => {
    const before = [moved("a", 0), moved("b", 20), moved("c", 40), moved("d", 60)];
    const settled = paintOrder(before);
    expect(settled).toHaveLength(1);

    // Frame one: "b" is being cut. It splits out, and the runs around it are built once.
    const cut = [before[0], { ...before[1], points: before[1].points.slice(0, 2) }, before[2], before[3]];
    const live = new Set(["b"]);
    const first = paintOrder(cut, 0, undefined, 1, live);
    expect(first.map((path) => path.key)).toEqual(["a", "b", "c"]);

    // Frame two: only "b" changes again; the runs before and after keep the very same strings,
    // so the DOM has nothing to re-parse for them.
    const again = [cut[0], { ...cut[1], points: cut[1].points.slice(0, 1) }, cut[2], cut[3]];
    const second = paintOrder(again, 0, undefined, 1, live);
    expect(second[0].d).toBe(first[0].d);
    expect(second[2].d).toBe(first[2].d);
    expect(second[1].d).not.toBe(first[1].d);
  });

  it("draws a fragment with its original's taper rather than new pointed ends", () => {
    const tapered = stroke({ id: "t", pressure: false, taper: 0.5,
      points: [0, 10, 20, 30, 40].map((x) => ({ x, y: 0, pressure: 1, timeMs: x, tiltX: 0, tiltY: 0 })) });
    const middle = { ...tapered, id: "m", points: tapered.points.slice(1, 4), trim: { startPt: 10, endPt: 10 } };
    const untrimmed = { ...middle, id: "u", trim: undefined };
    // At the cut end x = 10 the whole stroke is 10/12 of the way up its 12 pt taper, so the
    // fragment keeps that half-width there; without its lineage it would start from a point.
    expect(paintOrder([middle])[0].d).toMatch(/^M 10 0\.833 L 20 1 /);
    expect(paintOrder([untrimmed])[0].d).toMatch(/^M 10 0 L 20 1 /);
  });
});
