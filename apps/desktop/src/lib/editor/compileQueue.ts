import type { TypstCompileResult } from "./typst";
import type { TypstCompileRequest } from "../ipc/typst";

type Job = {
  key: string; foreground: boolean;
  run: () => Promise<TypstCompileResult>;
  waiters: { current: () => boolean; generation: number; resolve: (result: TypstCompileResult | null) => void; reject: (error: unknown) => void }[];
};

/** One preview at a time; active work overtakes queued prefetch without interrupting a compiler. */
export function createCompileQueue(run: (root: string, request: TypstCompileRequest) => Promise<TypstCompileResult>) {
  const jobs = new Map<string, Job>();
  let running = false;
  async function drain() {
    if (running) return;
    let next: Job | undefined;
    for (const job of jobs.values()) {
      job.waiters = job.waiters.filter(waiter => {
        if (waiter.current()) return true;
        waiter.resolve(null);
        return false;
      });
      if (!job.waiters.length) jobs.delete(job.key);
      else if (!next || (!next.foreground && job.foreground)) next = job;
    }
    if (!next) return;
    running = true;
    try {
      const result = await next.run();
      for (const waiter of next.waiters) waiter.resolve(waiter.current() ? { ...result, generation: waiter.generation } : null);
    } catch (error) {
      for (const waiter of next.waiters) {
        if (waiter.current()) waiter.reject(error);
        else waiter.resolve(null);
      }
    } finally {
      jobs.delete(next.key);
      running = false;
      void drain();
    }
  }
  return (root: string, request: TypstCompileRequest, current: () => boolean, foreground = true, epoch = 0): Promise<TypstCompileResult | null> => {
    const key = JSON.stringify([root, request.source, request.sharedStyle ?? "", request.widthPt, request.pageText ?? null, epoch]);
    return new Promise((resolve, reject) => {
      let job = jobs.get(key);
      if (!job) {
        job = { key, foreground, run: () => run(root, request), waiters: [] };
        jobs.set(key, job);
      }
      job.foreground ||= foreground;
      job.waiters.push({ current, generation: request.generation, resolve, reject });
      queueMicrotask(() => void drain());
    });
  };
}
