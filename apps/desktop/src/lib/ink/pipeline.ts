import type { Point, PageViewport, ScreenViewport, Size } from "../geometry/coordinates";
import { clampToPage, screenToPage } from "../geometry/coordinates";
import type { StrokePoint } from "../model";

export type PressureCalibration = {
  minimum: number;
  maximum: number;
  curve: number;
  smoothing: number;
};

export const DEFAULT_PRESSURE_CALIBRATION: PressureCalibration = {
  minimum: 0,
  maximum: 1,
  curve: 1,
  smoothing: 0.2,
};

export type PointerRole = "draw" | "erase" | "lasso" | "select" | "ignore";
export type InkTool = "pen" | "highlighter" | "eraser" | "lasso" | "select";
export type PointerMapping = {
  eraserButton: number;
  eraserButtonsMask: number;
};

export const DEFAULT_POINTER_MAPPING: PointerMapping = {
  eraserButton: 5,
  eraserButtonsMask: 32,
};

export type PointerLike = {
  pointerType: string;
  button: number;
  buttons: number;
  pressure: number;
  clientX: number;
  clientY: number;
  timeStamp: number;
  tiltX: number;
  tiltY: number;
};

export function pointerRole(
  pointer: Pick<PointerLike, "pointerType" | "button" | "buttons">,
  tool: InkTool,
  mapping: PointerMapping = DEFAULT_POINTER_MAPPING,
): PointerRole {
  if (pointer.pointerType === "touch") return "ignore";
  if (
    tool === "eraser" ||
    (pointer.pointerType === "pen" &&
      (pointer.button === mapping.eraserButton ||
        (pointer.buttons & mapping.eraserButtonsMask) !== 0))
  ) {
    return "erase";
  }
  if (tool === "lasso") return "lasso";
  if (tool === "select") return "select";
  if (pointer.pointerType === "pen") return "draw";
  if (tool === "pen" || tool === "highlighter") return "draw";
  return "select";
}

export function normalizePressure(
  rawPressure: number,
  calibration: PressureCalibration,
): number {
  const minimum = clampFinite(calibration.minimum, 0, 0.99, 0);
  const maximum = clampFinite(calibration.maximum, minimum + 0.01, 1, 1);
  const curve = clampFinite(calibration.curve, 0.1, 4, 1);
  const normalized = Math.min(Math.max((finite(rawPressure) - minimum) / (maximum - minimum), 0), 1);
  return normalized ** curve;
}

export function normalizePointerSample(
  sample: PointerLike,
  viewport: ScreenViewport,
  view: PageViewport,
  page: Size,
  calibration: PressureCalibration,
): StrokePoint {
  const pagePoint = clampToPage(
    screenToPage(
      { x: finite(sample.clientX), y: finite(sample.clientY) },
      viewport,
      view,
    ),
    page,
  );
  return {
    ...pagePoint,
    pressure: normalizePressure(sample.pressure, calibration),
    timeMs: Math.max(finite(sample.timeStamp), 0),
    tiltX: clampFinite(sample.tiltX, -90, 90, 0),
    tiltY: clampFinite(sample.tiltY, -90, 90, 0),
  };
}

export function smoothPoints(
  points: StrokePoint[],
  smoothing: number,
): StrokePoint[] {
  if (points.length < 3) return deduplicate(points);
  const amount = clampFinite(smoothing, 0, 1, 0);
  const source = deduplicate(points);
  return source.map((point, index) =>
    index === 0 || index === source.length - 1
      ? { ...point }
      : {
          ...point,
          x:
            point.x * (1 - amount) +
            ((source[index - 1].x + source[index + 1].x) / 2) * amount,
          y:
            point.y * (1 - amount) +
            ((source[index - 1].y + source[index + 1].y) / 2) * amount,
        },
  );
}

// Canonical sample precision. Raw pointer samples divided by zoom carry seventeen
// significant digits, which no digitizer resolves and which cost roughly half of every
// persisted ink layer. 0.01 pt is 1/7200 inch — finer than any reference device reports.
const POSITION_DECIMALS = 2;
const PRESSURE_DECIMALS = 3;
const TIME_DECIMALS = 1;
const TILT_DECIMALS = 0;

function round(value: number, decimals: number): number {
  const factor = 10 ** decimals;
  return Math.round(value * factor) / factor;
}

/**
 * Quantize completed samples to canonical precision. Apply once, after smoothing, on the
 * path that produces a persisted stroke — averaging reintroduces full precision, so
 * quantizing earlier is wasted.
 */
export function quantizePoints(points: StrokePoint[]): StrokePoint[] {
  return points.map((point) => ({
    x: round(point.x, POSITION_DECIMALS),
    y: round(point.y, POSITION_DECIMALS),
    pressure: round(point.pressure, PRESSURE_DECIMALS),
    timeMs: round(point.timeMs, TIME_DECIMALS),
    tiltX: round(point.tiltX, TILT_DECIMALS),
    tiltY: round(point.tiltY, TILT_DECIMALS),
  }));
}

export function replaySamples(
  samples: PointerLike[],
  viewport: ScreenViewport,
  view: PageViewport,
  page: Size,
  calibration: PressureCalibration,
): StrokePoint[] {
  return quantizePoints(
    smoothPoints(
      samples.map((sample) =>
        normalizePointerSample(sample, viewport, view, page, calibration),
      ),
      calibration.smoothing,
    ),
  );
}

function deduplicate(points: StrokePoint[]): StrokePoint[] {
  return points.filter(
    (point, index) =>
      index === 0 ||
      point.x !== points[index - 1].x ||
      point.y !== points[index - 1].y ||
      point.timeMs !== points[index - 1].timeMs,
  );
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0;
}

function clampFinite(value: number, minimum: number, maximum: number, fallback: number): number {
  return Math.min(Math.max(Number.isFinite(value) ? value : fallback, minimum), maximum);
}

export function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}
