import { describe, expect, it } from "vitest";
import type { ShapeStyle, StrokePoint } from "../model";
import { recognizeHeldShape } from "./recognize";

const style: ShapeStyle = {
  strokeColor: "#16212b",
  strokeWidthPt: 1.6,
  fillColor: null,
  opacity: 1,
};

const samples = (points: [number, number][]): StrokePoint[] =>
  points.map(([x, y], index) => ({
    x,
    y,
    pressure: 0.5,
    timeMs: index * 8,
    tiltX: 0,
    tiltY: 0,
  }));

describe("draw-and-hold shape recognition", () => {
  it("uses the existing conservative straight-line judgement", () => {
    const found = recognizeHeldShape(samples([[10, 10], [50, 11], [100, 10]]), style);
    expect(found?.draft.geometry.kind).toBe("line");
  });

  it("recognizes a rough closed rectangle", () => {
    const found = recognizeHeldShape(
      samples([[10, 10], [60, 9], [110, 11], [111, 50], [109, 80], [60, 81], [9, 79], [11, 45], [10, 10]]),
      style,
    );
    expect(found?.draft.geometry.kind).toBe("rectangle");
  });

  it("fits a deliberate open curve but leaves a small handwriting loop alone", () => {
    const curve = recognizeHeldShape(samples([[0, 40], [20, 10], [50, 0], [80, 10], [100, 40]]), style);
    const loop = recognizeHeldShape(samples([[0, 0], [4, -4], [8, 0], [4, 4], [0, 0]]), style);
    expect(curve?.draft.geometry.kind).toBe("spline");
    expect(loop).toBeNull();
  });
});
