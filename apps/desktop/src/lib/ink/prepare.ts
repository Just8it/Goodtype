import type { Stroke } from "../model";
import { hasSilhouette, installSilhouette, paintStrata, silhouette, silhouetteWidth } from "./paint";
import { packInk } from "./workerGeometry";

let worker: Worker | undefined;
let nextId = 0;
let unavailable = false;
const pending = new Map<number, { resolve: (paths: string[]) => void; reject: () => void }>();
const preparing = new WeakMap<Stroke, Map<number, Promise<void>>>();

function prepareInWorker(strokes: Stroke[], floor: number): Promise<string[]> {
  if (unavailable || typeof Worker === "undefined") return Promise.reject();
  try {
    if (!worker) {
      worker = new Worker(new URL("./prepare.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = ({ data }: MessageEvent<{ id: number; paths: string[] }>) => {
        pending.get(data.id)?.resolve(data.paths);
        pending.delete(data.id);
      };
      const failed = () => {
        unavailable = true;
        worker?.terminate();
        worker = undefined;
        for (const job of pending.values()) job.reject();
        pending.clear();
      };
      worker.onerror = failed;
      worker.onmessageerror = failed;
    }
    const packed = packInk(strokes);
    const id = ++nextId;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      try { worker!.postMessage({ id, packed, floor }, [packed.points.buffer]); }
      catch (error) { pending.delete(id); reject(error); }
    });
  } catch {
    return Promise.reject();
  }
}

/** Small completed strokes paint before the next frame; dense cold pages prepare off-thread. */
export async function prepareInk(strokes: Stroke[], floor: number): Promise<void> {
  const missing = strokes.filter(stroke => !hasSilhouette(stroke, floor));
  const waiting = new Set<Promise<void>>();
  const fresh = missing.filter(stroke => {
    const job = preparing.get(stroke)?.get(silhouetteWidth(stroke, floor));
    if (job) waiting.add(job);
    return !job;
  });
  if (fresh.reduce((count, stroke) => count + stroke.points.length, 0) >= 4000) {
    const job = (async () => {
      try {
        const paths = await prepareInWorker(fresh, floor);
        fresh.forEach((stroke, index) => installSilhouette(stroke, floor, paths[index]));
      } catch {
        let started = performance.now();
        for (const stroke of fresh) {
          silhouette(stroke, floor);
          if (performance.now() - started >= 6) {
            await new Promise(resolve => setTimeout(resolve, 0));
            started = performance.now();
          }
        }
      }
    })().finally(() => {
      for (const stroke of fresh) preparing.get(stroke)?.delete(silhouetteWidth(stroke, floor));
    });
    for (const stroke of fresh) {
      let widths = preparing.get(stroke);
      if (!widths) { widths = new Map(); preparing.set(stroke, widths); }
      widths.set(silhouetteWidth(stroke, floor), job);
    }
    waiting.add(job);
  }
  await Promise.all(waiting);
}

export async function preparedStrata(strokes: Stroke[], boundaries: number[], floor: number, selectedIds?: string[], selectionScale = 1,
  liveIds?: ReadonlySet<string>) {
  if (selectionScale !== 1 && selectedIds?.length) {
    const selected = new Set(selectedIds);
    await Promise.all([prepareInk(strokes.filter(stroke => !selected.has(stroke.id)), floor),
      prepareInk(strokes.filter(stroke => selected.has(stroke.id)), floor / selectionScale)]);
  } else await prepareInk(strokes, floor);
  return paintStrata(strokes, boundaries, floor, selectedIds, selectionScale, liveIds);
}
