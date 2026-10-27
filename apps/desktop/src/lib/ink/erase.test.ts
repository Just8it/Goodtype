import { describe, expect, it } from "vitest";
import type { Stroke, StrokePoint } from "../model";
import { eraseThrough, foldReplacements, type EraserSweep } from "./erase";
import { inkRadii } from "./outline";
import { placedTrim } from "./paint";
import { patternRuns } from "./pattern";
import { distanceToSegment, transformedPoint } from "./selection";

const identity = { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 };

function line(from: number, to: number, y = 50, step = 5): StrokePoint[] {
  const points: StrokePoint[] = [];
  for (let x = from; x <= to + 1e-9; x += step) {
    points.push({ x, y, pressure: 1, timeMs: x * 2, tiltX: 0, tiltY: 0 });
  }
  return points;
}

function stroke(overrides: Partial<Stroke> = {}): Stroke {
  return {
    id: "ink", zIndex: 1, tool: "pen", color: "#000000", widthPt: 2, pattern: "solid",
    pressure: false, taper: 0, opacity: 1, groupId: null, points: line(0, 100), transform: identity,
    ...overrides,
  };
}

const vertical = (x: number, radius = 5): EraserSweep => ({ from: { x, y: 0 }, to: { x, y: 100 }, radius });
let counter = 0;
const newId = () => `cut-${++counter}`;

describe("precise eraser", () => {
  it("cuts a crossed stroke into two fragments whose round caps just clear the eraser", () => {
    const original = stroke();
    const { strokes, replaced } = eraseThrough([original], [vertical(50)], { newId });
    expect(strokes).toHaveLength(2);
    const [left, right] = strokes;
    // Pen half-width 1 plus eraser radius 5: the caps stop one quantum or less outside x = 44 / 56.
    expect(left.points.at(-1)!.x).toBeLessThanOrEqual(44);
    expect(left.points.at(-1)!.x).toBeGreaterThan(43.98);
    expect(right.points[0].x).toBeGreaterThanOrEqual(56);
    expect(right.points[0].x).toBeLessThan(56.02);
    expect(left.id).toBe("ink");
    expect(replaced.get("ink")).toEqual([left.id, right.id]);
    expect(left.trim).toEqual({ startPt: 0, endPt: 100 - left.points.at(-1)!.x });
    expect(right.trim!.startPt).toBeCloseTo(right.points[0].x, 9);
    expect(right.trim!.endPt).toBe(0);
  });

  it("puts each new end on the original centreline with interpolated samples", () => {
    const [left] = eraseThrough([stroke()], [vertical(50)], { newId }).strokes;
    const end = left.points.at(-1)!;
    expect(end.y).toBe(50);
    expect(end.timeMs).toBeCloseTo(end.x * 2, 1);
    // Every other sample is the original, untouched.
    expect(left.points.slice(0, -1)).toEqual(stroke().points.filter((point) => point.x < end.x));
  });

  it("leaves a stroke the eraser misses by a hair alone, and the same array", () => {
    const strokes = [stroke()];
    const miss = { from: { x: 0, y: 56.1 }, to: { x: 100, y: 56.1 }, radius: 5 };
    expect(eraseThrough(strokes, [miss], { newId }).strokes).toBe(strokes);
    const graze = { from: { x: 50, y: 55.9 }, to: { x: 50, y: 80 }, radius: 5 };
    expect(eraseThrough(strokes, [graze], { newId }).strokes).toHaveLength(2);
  });

  it("removes a stroke the eraser runs along completely", () => {
    const { strokes, replaced } = eraseThrough([stroke()], [{ from: { x: -10, y: 50 }, to: { x: 110, y: 50 }, radius: 5 }], { newId });
    expect(strokes).toEqual([]);
    expect(replaced.get("ink")).toEqual([]);
  });

  it("cuts a rotated, scaled stroke exactly where it is drawn", () => {
    // The same mark as `stroke()`, stored at half size and turned a quarter.
    const turned = stroke({
      widthPt: 1,
      points: line(0, 50, 0, 2.5),
      transform: { translateX: 50, translateY: 0, scaleX: 2, scaleY: 2, rotation: 90 },
    });
    const sweep = { from: { x: 0, y: 50 }, to: { x: 100, y: 50 }, radius: 5 };
    const [top, bottom] = eraseThrough([turned], [sweep], { newId }).strokes;
    const topEnd = transformedPoint(top, top.points.at(-1)!);
    const bottomStart = transformedPoint(bottom, bottom.points[0]);
    expect(topEnd.y).toBeLessThanOrEqual(44 + 1e-9);
    expect(topEnd.y).toBeGreaterThan(43.97);
    expect(bottomStart.y).toBeGreaterThanOrEqual(56 - 1e-9);
    // Lineage is kept in the stroke's own points, half the page distance.
    expect(top.trim!.endPt * 2).toBeCloseTo(100 - topEnd.y, 6);
  });

  it("keeps a tapered stroke's width exactly where nothing was erased", () => {
    const original = stroke({ taper: 0.3, pressure: true,
      points: line(0, 100).map((point, index) => ({ ...point, pressure: 0.4 + (index % 5) / 10 })) });
    const before = radiiOf(original);
    const [left, right] = eraseThrough([original], [vertical(50)], { newId }).strokes;
    for (const fragment of [left, right]) {
      const after = radiiOf(fragment);
      fragment.points.forEach((point, index) => {
        const source = original.points.findIndex((sample) => sample === point);
        if (source >= 0) expect(after[index]).toBeCloseTo(before[source], 9);
      });
    }
    // The cut ends keep their width instead of growing new pointed tips.
    expect(radiiOf(left).at(-1)).toBeGreaterThan(0.3);
    expect(radiiOf(right)[0]).toBeGreaterThan(0.3);
  });

  it("keeps every surviving dash where the original drew it", () => {
    const original = stroke({ pattern: "dashed" });
    const [, right] = eraseThrough([original], [vertical(30)], { newId }).strokes;
    const page = (target: Stroke) => target.points.map((point) => ({ ...transformedPoint(target, point), pressure: 1 }));
    const whole = patternRuns(page(original), 2, "dashed", 1).map((run) => run[0].x)
      .filter((x) => x > right.points[0].x);
    const kept = patternRuns(page(right), 2, "dashed", 1, placedTrim(right)!.start).map((run) => run[0].x)
      .filter((x) => x > right.points[0].x + 1e-6);
    expect(kept.map((x) => x.toFixed(6))).toEqual(whole.map((x) => x.toFixed(6)));
  });

  it("cuts a highlighter flush with the eraser instead of half a band wider", () => {
    const band = stroke({ tool: "highlighter", widthPt: 12, opacity: 0.4 });
    const [left, right] = eraseThrough([band], [vertical(50)], { newId }).strokes;
    expect(left.points.at(-1)!.x).toBeGreaterThan(44.9);
    expect(left.points.at(-1)!.x).toBeLessThanOrEqual(45);
    expect(right.points[0].x).toBeGreaterThanOrEqual(55);
    expect(right.points[0].x).toBeLessThan(55.1);
  });

  it("drops a highlighter sliver shorter than half its width but keeps a short piece of pen", () => {
    const band = stroke({ tool: "highlighter", widthPt: 12 });
    expect(eraseThrough([band], [vertical(40), vertical(52)], { newId }).strokes).toHaveLength(2);
    const pen = stroke();
    const pieces = eraseThrough([pen], [vertical(40), vertical(54)], { newId }).strokes;
    expect(pieces).toHaveLength(3);
    expect(pieces[1].points[0].x).toBeGreaterThanOrEqual(46);
    expect(pieces[1].points.at(-1)!.x).toBeLessThanOrEqual(48);
  });

  it("takes a dot whole when touched and leaves it otherwise", () => {
    const dot = stroke({ points: [{ x: 50, y: 50, pressure: 1, timeMs: 0, tiltX: 0, tiltY: 0 }] });
    expect(eraseThrough([dot], [vertical(55.9)], { newId }).strokes).toEqual([]);
    expect(eraseThrough([dot], [vertical(56.1)], { newId }).strokes).toEqual([dot]);
  });

  it("respects what the eraser reaches", () => {
    const strokes = [stroke()];
    expect(eraseThrough(strokes, [vertical(50)], { newId, reaches: (target) => target.tool === "highlighter" }).strokes)
      .toBe(strokes);
  });

  it("skips a cut the layer cannot hold, but still removes whole strokes", () => {
    const crossed = stroke();
    const covered = stroke({ id: "small", points: line(48, 52) });
    const result = eraseThrough([crossed, covered], [vertical(50)], { newId, limits: { maxStrokes: 2, maxPoints: 1000 } });
    expect(result.limited).toBe(true);
    expect(result.strokes).toEqual([crossed]);
  });

  it("traces a fragment cut again on a later frame back to its original", () => {
    const total = new Map<string, string[]>();
    const first = eraseThrough([stroke()], [vertical(30)], { newId });
    foldReplacements(total, first.replaced);
    const second = eraseThrough(first.strokes, [vertical(70)], { newId });
    foldReplacements(total, second.replaced);
    expect([...total.keys()]).toEqual(["ink"]);
    expect(total.get("ink")).toEqual(second.strokes.map((fragment) => fragment.id));
    const middle = second.strokes[1];
    const length = middle.points.at(-1)!.x - middle.points[0].x;
    expect(middle.trim!.startPt + length + middle.trim!.endPt).toBeCloseTo(100, 9);
  });
});

describe("precise eraser guarantees", () => {
  // A small deterministic generator, so a failure names a reproducible case.
  function random(seed: number) {
    let state = seed;
    return () => {
      state = (state * 1664525 + 1013904223) % 4294967296;
      return state / 4294967296;
    };
  }

  for (let seed = 1; seed <= 40; seed += 1) {
    it(`leaves nothing under the eraser and removes only what it touched (seed ${seed})`, () => {
      const next = random(seed);
      const tool = next() < 0.3 ? "highlighter" as const : "pen" as const;
      const points: StrokePoint[] = [];
      let x = 20 + next() * 20, y = 20 + next() * 60;
      for (let index = 0; index < 12 + Math.floor(next() * 20); index += 1) {
        points.push({ x, y, pressure: 0.2 + next() * 0.8, timeMs: index * 4, tiltX: 0, tiltY: 0 });
        x += 1 + next() * 6;
        y += (next() - 0.5) * 8;
      }
      const original = stroke({ tool, points, widthPt: tool === "pen" ? 0.8 + next() * 3 : 6 + next() * 8,
        pressure: tool === "pen", taper: tool === "pen" ? next() * 0.3 : 0 });
      const sweeps: EraserSweep[] = [];
      let ex = 20 + next() * 80, ey = 10 + next() * 80;
      for (let index = 0; index < 1 + Math.floor(next() * 4); index += 1) {
        const tx = ex + (next() - 0.5) * 40, ty = ey + (next() - 0.5) * 40;
        sweeps.push({ from: { x: ex, y: ey }, to: { x: tx, y: ty }, radius: 3 + next() * 8 });
        ex = tx;
        ey = ty;
      }
      const { strokes } = eraseThrough([original], sweeps, { newId });

      // 1. Nothing left behind is under the eraser (round pens: the disk at every centreline point).
      if (tool === "pen") {
        for (const fragment of strokes) {
          const page = fragment.points.map((point) => transformedPoint(fragment, point));
          const radii = radiiOf(fragment);
          for (let index = 0; index + 1 < page.length; index += 1) {
            for (let t = 0; t <= 1; t += 0.05) {
              const at = { x: page[index].x + (page[index + 1].x - page[index].x) * t,
                y: page[index].y + (page[index + 1].y - page[index].y) * t };
              const radius = radii[index] + (radii[index + 1] - radii[index]) * t;
              for (const sweep of sweeps) {
                expect(distanceToSegment(at, sweep.from, sweep.to) - radius).toBeGreaterThan(sweep.radius - 0.02);
              }
            }
          }
        }
      }

      // 2. What is gone was touched: every removed centreline point lies within reach of a sweep.
      const kept = strokes.map((fragment) => [fragment.trim?.startPt ?? 0, length(original) - (fragment.trim?.endPt ?? 0)]);
      const page = original.points.map((point) => transformedPoint(original, point));
      const radii = radiiOf(original);
      let walked = 0;
      for (let index = 0; index + 1 < page.length; index += 1) {
        const segment = Math.hypot(page[index + 1].x - page[index].x, page[index + 1].y - page[index].y);
        for (let t = 0; t <= 1; t += 0.05) {
          const along = walked + segment * t;
          if (kept.some(([from, to]) => along >= from - 1e-6 && along <= to + 1e-6)) continue;
          const at = { x: page[index].x + (page[index + 1].x - page[index].x) * t,
            y: page[index].y + (page[index + 1].y - page[index].y) * t };
          // Within the eraser plus a half-width (pens) or the band's half-width (highlighters),
          // plus the half-width sliver a dropped highlighter piece may carry.
          const slack = tool === "highlighter" ? original.widthPt : 0.02;
          const radius = radii[index] + (radii[index + 1] - radii[index]) * t;
          const reach = Math.min(...sweeps.map((sweep) => distanceToSegment(at, sweep.from, sweep.to) - radius - sweep.radius));
          expect(reach).toBeLessThan(slack);
        }
        walked += segment;
      }
    });
  }
});

function radiiOf(target: Stroke): number[] {
  return inkRadii(
    target.points.map((point) => ({ ...transformedPoint(target, point), pressure: point.pressure })),
    { widthPt: target.widthPt, pressure: target.pressure, taper: target.taper, trim: placedTrim(target) },
  );
}

function length(target: Stroke): number {
  let total = 0;
  for (let index = 1; index < target.points.length; index += 1) {
    total += Math.hypot(target.points[index].x - target.points[index - 1].x, target.points[index].y - target.points[index - 1].y);
  }
  return total;
}
