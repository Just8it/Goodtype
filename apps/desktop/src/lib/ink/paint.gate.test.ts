import { describe, expect, it } from "vitest";
import type { Stroke } from "../model";
import { paintOrder } from "./paint";

// The 5,000-stroke gate, for the display layer specifically. The storage side of the
// gate is covered by `commits_and_reopens_a_five_thousand_stroke_page` in goodtype-core; this is
// the half that lives in the browser.
//
// Timings are deliberately loose. The point is not to pin a number to this machine — it is to
// fail if the per-stroke cache in `paint.ts` is ever removed, because without it every pen lift
// and every frame of a selection drag re-derives all five thousand silhouettes, which measured
// around 45 ms and lands directly in the interaction.

function page(count: number, strokesPerColour: number): Stroke[] {
  const colours = ["#1e232b", "#4c8df0", "#e5645e"];
  return Array.from({ length: count }, (_, index) => ({
    id: `stroke-${index}`,
    tool: "pen" as const,
    color: colours[Math.floor(index / strokesPerColour) % colours.length],
    widthPt: 1.6,
    pressure: true,
    taper: 0.12,
    opacity: 1,
    groupId: null,
    points: Array.from({ length: 24 }, (_, step) => ({
      x: (index % 40) * 14 + 20 + step * 0.5,
      y: Math.floor(index / 40) * 8 + 20 + Math.sin(step / 3) * 2,
      pressure: 0.3 + (step % 7) / 10,
      timeMs: step * 8,
      tiltX: 0,
      tiltY: 0,
    })),
    transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
  }));
}

function elapsed(run: () => unknown): number {
  const started = performance.now();
  run();
  return performance.now() - started;
}

describe("five thousand strokes", () => {
  // Writing switches colour by the paragraph, not by the stroke, so colours arrive in runs — and
  // each spatial bucket within a run merges. More nodes allow raster tiles to cull distant ink.
  it("collapses a realistic page to a manageable number of nodes", () => {
    const painted = paintOrder(page(5000, 60), 0.5);
    expect(painted.length).toBeLessThan(600);
    // Outline paths contain absolute M/L coordinates; every bucket stays local, not page-sized.
    for (const { d } of painted) {
      const points = [...d.matchAll(/[ML] (-?[\d.]+) (-?[\d.]+)/g)];
      const xs = points.map(point => Number(point[1]));
      const ys = points.map(point => Number(point[2]));
      expect(Math.max(...xs) - Math.min(...xs)).toBeLessThan(160);
      expect(Math.max(...ys) - Math.min(...ys)).toBeLessThan(160);
    }
  });

  it("re-derives only what changed when a stroke is added or dragged", () => {
    const strokes = page(5000, 60);
    paintOrder(strokes, 0.5);

    const added = [...strokes, { ...strokes[0], id: "added" }];
    expect(elapsed(() => paintOrder(added, 0.5))).toBeLessThan(25);

    const dragged = strokes.map((stroke, index) =>
      index < 3 ? { ...stroke, transform: { ...stroke.transform, translateX: 4 } } : stroke,
    );
    expect(elapsed(() => paintOrder(dragged, 0.5))).toBeLessThan(25);
  });
});
import { paintStrata, installSilhouette } from "./paint";

it("matches restarting bands for 5000 x 60 strokes, duplicate boundaries and unordered input", () => {
  const strokes = page(5000, 60).map((s, i) => ({...s, zIndex:i,
    points:Array.from({length:60}, (_, j)=>s.points[j % s.points.length])}));
  strokes.forEach(s=>installSilhouette(s,0,`M ${s.zIndex} 0 L 1 1 Z`));
  for (const count of [10,100,1000]) {
    const boundaries=Array.from({length:count},(_,i)=>Math.floor(i/2)*2);
    let oldComparisons=0, newComparisons=0, cursor=0;
    const groups=new Map<number,Stroke[]>();
    for (const s of strokes) {
      let band=0;
      while (band<boundaries.length) {oldComparisons++; if(boundaries[band]>s.zIndex!)break;band++;}
      while(cursor<boundaries.length) {newComparisons++;if(boundaries[cursor]>s.zIndex!)break;cursor++;}
      expect(cursor).toBe(band);
      const group=groups.get(band); if(group)group.push(s);else groups.set(band,[s]);
    }
    expect(oldComparisons).toBe(({10:49968,100:495198,1000:4501998})[count]);
    expect(newComparisons).toBe(({10:18,100:198,1000:1998})[count]);
    const expected=[...groups.values()].map(group=>({key:group[0].id,zIndex:group[0].zIndex,paths:paintOrder(group)}));
    expect(paintStrata(strokes,boundaries)).toEqual(expected);
    expect(paintStrata([...strokes].reverse(),[...boundaries].reverse())).toEqual(expected);
  }
});
