/** Keep temporary full-page renders serial and discard hidden rasters before visible quality. */
const MAX_RETAINED_BYTES = 128 * 1024 * 1024;
type Raster = { bytes: number; visible: () => boolean; evict: () => void };
const rasters = new Map<HTMLCanvasElement, Raster>();
const jobs: { current: () => boolean; visible: () => boolean; run: () => Promise<void>; resolve: () => void; reject: (error: unknown) => void }[] = [];
let running = false;

async function drain() {
  if (running) return;
  const index = jobs.findIndex(job => job.visible() && job.current());
  const job = jobs.splice(index < 0 ? 0 : index, 1)[0];
  if (!job) return;
  if (!job.current()) {
    job.resolve();
    void drain();
    return;
  }
  running = true;
  try { await job.run(); job.resolve(); }
  catch (error) { job.reject(error); }
  finally { running = false; void drain(); }
}

export function schedulePdfRaster(run: () => Promise<void>, current: () => boolean, visible: () => boolean): Promise<void> {
  return new Promise((resolve, reject) => {
    jobs.push({ run, current, visible, resolve, reject });
    queueMicrotask(() => void drain());
  });
}

export function retainPdfRaster(canvas: HTMLCanvasElement, visible: () => boolean, evict: () => void) {
  rasters.delete(canvas);
  rasters.set(canvas, { bytes: canvas.width * canvas.height * 4, visible, evict });
  trimPdfRasters();
}

export function releasePdfRaster(canvas: HTMLCanvasElement) { rasters.delete(canvas); }

export function trimPdfRasters() {
  let bytes = [...rasters.values()].reduce((sum, raster) => sum + raster.bytes, 0);
  for (const [canvas, raster] of rasters) {
    if (bytes <= MAX_RETAINED_BYTES) break;
    if (raster.visible()) continue;
    rasters.delete(canvas);
    bytes -= raster.bytes;
    raster.evict();
    canvas.width = canvas.height = 1;
  }
  // Visible pages may exceed the cache budget: preserve their full quality. One staging raster
  // is allowed in addition; tiling is needed only if a single supported page exceeds memory.
}
