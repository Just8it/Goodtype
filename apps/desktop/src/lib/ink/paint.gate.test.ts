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
  // a run is what merges. Measured at 84 nodes for 5,000 strokes.
  it("collapses a realistic page to a manageable number of nodes", () => {
    const painted = paintOrder(page(5000, 60), 0.5);
    expect(painted.length).toBeLessThan(200);
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
