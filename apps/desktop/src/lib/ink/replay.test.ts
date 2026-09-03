import { describe, expect, it } from "vitest";
import fixture from "../../../../../fixtures/pen-events/phase0b-replay.json";
import type { Stroke } from "../model";
import { maximumSampleGap, summarizeMetric } from "./metrics";
import { pointerRole, quantizePoints, replaySamples } from "./pipeline";
import {
  eraseStrokeAt,
  hitStroke,
  moveSelected,
  scaleSelected,
  selectStrokesInLasso,
  selectionBounds,
} from "./selection";

describe("deterministic pen replay", () => {
  it("normalizes pressure and lets zoomed samples run past the page edge", () => {
    const points = replaySamples(
      fixture.samples,
      fixture.viewport,
      fixture.view,
      fixture.page,
      fixture.calibration,
    );

    // The first and last samples land outside the 100x80 page and keep their real coordinates.
    // Pinning them to the edge is what drew a line along the boundary when a hand ran off the
    // sheet mid-stroke. Only the far outlier meets the sanity bound: y stops at twice the page
    // height rather than at the page height.
    expect(points.map(({ x, y, pressure }) => ({ x, y, pressure }))).toEqual([
      { x: -10, y: -15, pressure: 0 },
      { x: 50, y: 40, pressure: 0.5 },
      { x: 145, y: 160, pressure: 1 },
    ]);
    expect(pointerRole({ pointerType: "touch", button: 0, buttons: 1 }, "pen")).toBe(
      "ignore",
    );
    expect(pointerRole({ pointerType: "touch", button: 0, buttons: 1 }, "shape")).toBe(
      "shape",
    );
    // The shape tool is the only thing a finger may drive. A resting palm must never erase.
    expect(pointerRole({ pointerType: "touch", button: 0, buttons: 1 }, "eraser")).toBe(
      "ignore",
    );
    expect(pointerRole({ pointerType: "touch", button: 0, buttons: 1 }, "lasso")).toBe(
      "ignore",
    );
    expect(pointerRole({ pointerType: "mouse", button: 0, buttons: 1 }, "shape")).toBe(
      "shape",
    );
    expect(pointerRole({ pointerType: "pen", button: 5, buttons: 32 }, "pen")).toBe(
      "erase",
    );
    expect(pointerRole({ pointerType: "mouse", button: 0, buttons: 1 }, "select")).toBe(
      "select",
    );
    expect(pointerRole({ pointerType: "pen", button: 0, buttons: 1 }, "select")).toBe(
      "select",
    );
  });

  it("selects, hits, moves, and uniformly scales complete strokes", () => {
    const stroke = makeStroke("stroke-a", [
      { x: 10, y: 10 },
      { x: 20, y: 20 },
    ]);
    const strokes = [stroke, makeStroke("stroke-b", [{ x: 80, y: 80 }])];
    const selected = selectStrokesInLasso(strokes, [
      { x: 0, y: 0 },
      { x: 30, y: 0 },
      { x: 30, y: 30 },
      { x: 0, y: 30 },
    ]);

    expect(selected).toEqual(["stroke-a"]);
    expect(hitStroke(strokes, { x: 15, y: 15 }, 2)?.id).toBe("stroke-a");

    const moved = moveSelected(strokes, selected, { x: 5, y: -2 });
    const scaled = scaleSelected(moved, selected, { x: 0, y: 0 }, 2);
    expect(selectionBounds(scaled, selected)).toEqual({
      left: 30,
      top: 16,
      right: 50,
      bottom: 36,
    });

    const erasedFirst = eraseStrokeAt(strokes, { x: 15, y: 15 }, 2);
    const erasedBoth = eraseStrokeAt(erasedFirst, { x: 80, y: 80 }, 2);
    expect(erasedBoth).toEqual([]);
  });

  it("quantizes completed samples to canonical precision", () => {
    const [point] = quantizePoints([
      {
        x: 123.456789012345,
        y: 234.5678901234567,
        pressure: 0.5127384,
        timeMs: 1234567.891234,
        tiltX: -4.1234,
        tiltY: 2.6789,
      },
    ]);

    expect(point).toEqual({
      x: 123.46,
      y: 234.57,
      pressure: 0.513,
      timeMs: 1234567.9,
      tiltX: -4,
      tiltY: 3,
    });
    // Positional error stays far below what any reference digitizer resolves.
    expect(Math.abs(point.x - 123.456789012345)).toBeLessThan(0.005);

    // The payload win is in the numeric values, so measure it across a stroke's worth of
    // samples where fixed key overhead is amortized rather than dominating.
    const raw = Array.from({ length: 60 }, (_, index) => ({
      x: 100 + Math.sin(index) * 123.456789012345,
      y: 200 + Math.cos(index) * 234.5678901234567,
      pressure: (index % 7) / 7,
      timeMs: index * 8.3333333,
      tiltX: -4.1234,
      tiltY: 2.6789,
    }));
    const rawBytes = JSON.stringify(raw).length;
    const quantizedBytes = JSON.stringify(quantizePoints(raw)).length;
    expect(quantizedBytes).toBeLessThan(rawBytes * 0.7);
  });

  it("summarizes recent stroke timing without changing samples", () => {
    expect(maximumSampleGap([{ timeMs: 1 }, { timeMs: 5 }, { timeMs: 8 }])).toBe(4);
    expect(
      summarizeMetric(
        [1, 2, 3, 4].map((value) => ({
          sampleCount: 1,
          maxSampleGapMs: value,
          activeFeedbackMs: value,
          commitMs: value,
        })),
        "activeFeedbackMs",
      ),
    ).toEqual({ median: 2, p95: 4, worst: 4 });
  });
});

function makeStroke(id: string, points: Array<{ x: number; y: number }>): Stroke {
  return {
    id,
    tool: "pen",
    color: "#111111",
    widthPt: 2,
    pressure: true,
    taper: 0,
    opacity: 1,
    groupId: null,
    points: points.map((point, index) => ({
      ...point,
      pressure: 0.5,
      timeMs: index,
      tiltX: 0,
      tiltY: 0,
    })),
    transform: {
      translateX: 0,
      translateY: 0,
      scaleX: 1,
      scaleY: 1,
      rotation: 0,
    },
  };
}
