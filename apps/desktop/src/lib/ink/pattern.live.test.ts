import { describe, expect, it } from "vitest";
import { LiveInkOutline, patternedOutlinePoints, type LiveInkFrame, type LiveInkStyle } from "./pattern";
import { limitedTaper, outlinePath, type OutlinePoint } from "./outline";

/**
 * A pen stroke as a tablet reports it: a wandering curve with changing pressure, a slow stretch
 * of near-coincident samples, and the odd exact repeat.
 */
function penSamples(count: number): OutlinePoint[] {
  const points: OutlinePoint[] = [];
  let x = 40, y = 300;
  for (let index = 0; index < count; index += 1) {
    const slow = index % 400 > 360;
    const step = slow ? 0.02 : 0.6;
    const heading = Math.sin(index / 37) * 1.4 + Math.cos(index / 211);
    if (index % 97 !== 0) {
      x += Math.cos(heading) * step;
      y += Math.sin(heading) * step;
    }
    points.push({ x, y, pressure: 0.5 + 0.45 * Math.sin(index / 23) });
  }
  return points;
}

const BATCH = 4; // samples per frame: a 240 Hz pen on a 60 Hz display

const variants: [string, LiveInkStyle][] = [
  ["round solid, pressure", { widthPt: 2, pressure: true, taper: 0, taperMaximumPt: null, pattern: "solid", spacing: 1, round: true }],
  ["round solid, fixed width", { widthPt: 1.5, pressure: false, taper: 0, taperMaximumPt: null, pattern: "solid", spacing: 1, round: true }],
  ["round solid, limited taper", { widthPt: 2, pressure: true, taper: 0.35, taperMaximumPt: 6, pattern: "solid", spacing: 1, round: true }],
  ["round solid, long taper", { widthPt: 3, pressure: true, taper: 1, taperMaximumPt: 1000, pattern: "solid", spacing: 1, round: true }],
  ["round dashed", { widthPt: 2, pressure: true, taper: 0.35, taperMaximumPt: 6, pattern: "dashed", spacing: 1, round: true }],
  ["round dashed, wide gaps", { widthPt: 1, pressure: false, taper: 0, taperMaximumPt: 3, pattern: "dashed", spacing: 2.5, round: true }],
  ["flat dashed", { widthPt: 2, pressure: true, taper: 0, taperMaximumPt: null, pattern: "dashed", spacing: 1, round: false }],
  ["dotted", { widthPt: 2, pressure: true, taper: 0.35, taperMaximumPt: 6, pattern: "dotted", spacing: 1, round: true }],
  ["dotted, tight", { widthPt: 0.8, pressure: false, taper: 0, taperMaximumPt: 3, pattern: "dotted", spacing: 0.5, round: true }],
];
const highlighter: LiveInkStyle = {
  widthPt: 6, pressure: true, taper: 0, taperMaximumPt: null, pattern: "solid", spacing: 1, round: false,
};

/** What the painter used to build every frame, from scratch. */
function fullRebuild(points: OutlinePoint[], style: LiveInkStyle) {
  const taper = style.taperMaximumPt === null ? style.taper : limitedTaper(points, style.taper, style.taperMaximumPt);
  return patternedOutlinePoints(points, { widthPt: style.widthPt, pressure: style.pressure, taper },
    style.pattern, style.spacing, style.round);
}

const flatten = (frame: LiveInkFrame) => frame.runs.flatMap((run) => run.polygons);

describe("live stroke geometry", () => {
  it.each([...variants, ["flat highlighter (rebuilt whole)", highlighter] as [string, LiveInkStyle]])(
    "matches the full rebuild on every frame: %s",
    (_name, style) => {
      const samples = penSamples(900);
      const live = new LiveInkOutline(style);
      const points: OutlinePoint[] = [];
      for (let index = 0; index < samples.length; index += 1) {
        points.push(samples[index]);
        // Every one of the first frames, where the stroke grows out of a dot, then batched.
        if (index > 40 && (index + 1) % BATCH !== 0 && index !== samples.length - 1) continue;
        const cached = flatten(live.update(points));
        const expected = fullRebuild(points, style);
        expect(cached).toEqual(expected);
        expect(cached.map(outlinePath).join(" ")).toBe(expected.map(outlinePath).join(" "));
      }
    },
  );

  it("keeps stable runs as the same, growing arrays", () => {
    const samples = penSamples(400);
    const live = new LiveInkOutline(variants[0][1]);
    const first = live.update(samples.slice(0, 200));
    const stable = first.runs.filter((run) => run.stable).map((run) => [run.polygons, run.polygons.length] as const);
    expect(stable.length).toBeGreaterThan(0);
    const second = live.update(samples);
    const later = second.runs.filter((run) => run.stable).map((run) => run.polygons);
    stable.forEach(([polygons, length], index) => {
      expect(later[index]).toBe(polygons);
      expect(polygons.length).toBeGreaterThanOrEqual(length);
    });
  });

  it.each(variants)("does per-frame work that does not grow with the stroke: %s", (_name, style) => {
    const samples = penSamples(5000);
    const live = new LiveInkOutline(style);
    const points: OutlinePoint[] = [];
    const work: number[] = [];
    for (let index = 0; index < samples.length; index += BATCH) {
      points.push(...samples.slice(index, index + BATCH));
      work.push(live.update(points).built);
    }
    const early = Math.max(...work.slice(50, 250));
    const late = Math.max(...work.slice(-250));
    const everything = fullRebuild(points, style).length;
    // The full rebuild redid every polygon; the cache redoes a bounded neighbourhood of the pen.
    expect(everything).toBeGreaterThan(400);
    expect(late).toBeLessThanOrEqual(Math.max(early * 1.5, 40));
    expect(late).toBeLessThan(everything / 10);
  });
});
