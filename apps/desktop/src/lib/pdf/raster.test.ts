import { expect, it, vi } from "vitest";
import { retainPdfRaster, releasePdfRaster, schedulePdfRaster, trimPdfRasters } from "./raster";

it("evicts hidden rasters before visible quality, including when a visible page leaves view", () => {
  const a = { width: 5000, height: 5000 } as HTMLCanvasElement;
  const b = { width: 5000, height: 5000 } as HTMLCanvasElement;
  const evict = vi.fn();
  let visible = true;
  retainPdfRaster(a, () => visible, evict);
  retainPdfRaster(b, () => true, () => {});
  expect(a.width).toBe(5000);
  visible = false;
  trimPdfRasters();
  expect(evict).toHaveBeenCalledOnce();
  expect(a.width * a.height).toBe(1);
  expect(b.width).toBe(5000);
  releasePdfRaster(b);
});
it("runs visible renders first, serializes staging memory, and skips cancelled jobs", async () => {
  const order: number[] = [];
  const a = schedulePdfRaster(async () => { order.push(1); }, () => true, () => false);
  const b = schedulePdfRaster(async () => { order.push(2); }, () => true, () => true);
  const c = schedulePdfRaster(async () => { order.push(3); }, () => false, () => true);
  await Promise.all([a, b, c]);
  expect(order).toEqual([2, 1]);
});
