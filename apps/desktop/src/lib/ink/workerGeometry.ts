import type { Stroke } from "../model";

/** Only renderer coordinates cross the worker boundary; canonical samples remain untouched. */
export function packInk(strokes: Stroke[]) {
  const points = new Float64Array(strokes.reduce((count, stroke) => count + stroke.points.length * 3, 0));
  let offset = 0;
  const headers = strokes.map(({ points: samples, ...stroke }) => {
    const start = offset;
    for (const point of samples) {
      points[offset++] = point.x;
      points[offset++] = point.y;
      points[offset++] = point.pressure;
    }
    return { ...stroke, start, length: samples.length };
  });
  return { headers, points };
}

export function unpackInk(packed: ReturnType<typeof packInk>): Stroke[] {
  return packed.headers.map(({ start, length, ...stroke }) => ({
    ...stroke,
    points: Array.from({ length }, (_, index) => ({
      x: packed.points[start + index * 3], y: packed.points[start + index * 3 + 1],
      pressure: packed.points[start + index * 3 + 2], timeMs: 0, tiltX: 0, tiltY: 0,
    })),
  }));
}
