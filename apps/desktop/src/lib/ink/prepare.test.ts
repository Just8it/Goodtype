import { expect, it, vi } from "vitest";
import type { Stroke } from "../model";
import { packInk, unpackInk } from "./workerGeometry";
import { silhouette, paintStrata } from "./paint";
import { preparedStrata } from "./prepare";

function stroke(pattern: Stroke["pattern"]): Stroke {
  return { id: pattern!, tool: "pen", color: "#112233", widthPt: 2, pressure: true,
    taper: 0.2, opacity: 0.4, groupId: null, pattern, patternSpacing: 1.5,
    transform: { translateX: 4, translateY: 7, scaleX: 1.2, scaleY: 0.8, rotation: 13 },
    points: Array.from({ length: 90 }, (_, index) => ({ x: index * 0.731, y: Math.sin(index) * 9,
      pressure: index / 90, timeMs: index * 3, tiltX: 4, tiltY: -2 })) };
}
it("worker packing preserves exact silhouettes for patterns, pressure, taper and transforms", () => {
  for (const pattern of ["solid", "dashed", "dotted"] as const) {
    const original = stroke(pattern);
    const [unpacked] = unpackInk(packInk([original]));
    expect(silhouette(unpacked, 0.5)).toBe(silhouette(original, 0.5));
    expect(original.points[1].timeMs).toBe(3);
  }
});
it("a failed worker yields locally and produces the same geometry", async () => {
  const strokes = Array.from({ length: 50 }, (_, index) => ({ ...stroke("solid"), id: `s-${index}` }));
  const worker = vi.fn(function () { throw new Error("unavailable"); });
  vi.stubGlobal("Worker", worker);
  try {
    const result = await preparedStrata(strokes, [10], 0.5);
    expect(worker).toHaveBeenCalledTimes(1);
    expect(result).toEqual(paintStrata(strokes, [10], 0.5));
  } finally { vi.unstubAllGlobals(); }
});
