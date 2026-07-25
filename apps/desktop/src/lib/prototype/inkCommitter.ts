import type { Stroke } from "../model";

// Batches a burst of strokes into one commit. A stroke lands every few hundred milliseconds
// while writing, and one commit per stroke would rewrite the ink file constantly; the debounce
// coalesces them, and the maximum guarantees ink still reaches disk during continuous writing.
export const INK_SAVE_DEBOUNCE_MS = 500;
export const INK_SAVE_MAXIMUM_MS = 2000;

export type InkCommitter = {
  /** Record new strokes and (re)arm the debounce. */
  commit(strokes: Stroke[], label: string): void;
  /** Force any pending strokes to save now; resolves once the save queue drains. */
  flush(): Promise<void>;
  /** Whether a save is currently scheduled. */
  pending(): boolean;
  /** Cancel the timer. Callers flush first if the strokes still matter. */
  dispose(): void;
};

export function createInkCommitter(options: {
  save: (strokes: Stroke[], label: string) => Promise<void>;
  debounceMs?: number;
  maximumMs?: number;
}): InkCommitter {
  const debounceMs = options.debounceMs ?? INK_SAVE_DEBOUNCE_MS;
  const maximumMs = options.maximumMs ?? INK_SAVE_MAXIMUM_MS;

  let timer: ReturnType<typeof setTimeout> | undefined;
  let deadline = 0;
  let queue: Promise<void> = Promise.resolve();
  let strokes: Stroke[] = [];
  let label = "Updated ink";

  function run() {
    if (timer) clearTimeout(timer);
    timer = undefined;
    const committed = strokes;
    const committedLabel = label;
    queue = queue.then(() => options.save(committed, committedLabel));
  }

  return {
    commit(next, nextLabel) {
      strokes = next;
      label = nextLabel;
      const now = performance.now();
      // The first pending stroke starts the ceiling; later strokes extend the debounce but can
      // never push the save past it.
      if (!timer) deadline = now + maximumMs;
      else clearTimeout(timer);
      timer = setTimeout(run, Math.max(0, Math.min(debounceMs, deadline - now)));
    },
    flush() {
      if (timer) run();
      return queue;
    },
    pending() {
      return timer !== undefined;
    },
    dispose() {
      if (timer) clearTimeout(timer);
      timer = undefined;
    },
  };
}
