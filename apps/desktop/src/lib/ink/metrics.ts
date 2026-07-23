import type { StrokePoint } from "../model";

export type StrokePerformance = {
  sampleCount: number;
  maxSampleGapMs: number;
  activeFeedbackMs: number;
  commitMs: number;
};

export function maximumSampleGap(points: Pick<StrokePoint, "timeMs">[]): number {
  let maximum = 0;
  for (let index = 1; index < points.length; index += 1) {
    maximum = Math.max(maximum, points[index].timeMs - points[index - 1].timeMs);
  }
  return maximum;
}

export function summarizeMetric(
  samples: StrokePerformance[],
  key: keyof Omit<StrokePerformance, "sampleCount">,
): { median: number; p95: number; worst: number } | null {
  if (samples.length === 0) return null;
  const values = samples.map((sample) => sample[key]).sort((a, b) => a - b);
  const percentile = (amount: number) =>
    values[Math.min(values.length - 1, Math.ceil(values.length * amount) - 1)];
  return { median: percentile(0.5), p95: percentile(0.95), worst: values.at(-1)! };
}
