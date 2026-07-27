import type { Stroke } from "../model";
import { createCommitTimer } from "./commitTimer";

export { INK_SAVE_DEBOUNCE_MS, INK_SAVE_MAXIMUM_MS } from "./commitTimer";

/**
 * Ink saving for a page that is not the one being edited.
 *
 * A neighbouring page has no live component state to build a snapshot from, so unlike the active
 * page it carries the strokes it captured through to the save. The timing is
 * [`createCommitTimer`], shared with the active page — see there for why the debounce has a
 * ceiling.
 */
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
  let queue: Promise<void> = Promise.resolve();
  let strokes: Stroke[] = [];
  let label = "Updated ink";

  // Saves are chained rather than run concurrently: two commits of the same page racing would
  // let the older one land last.
  const timer = createCommitTimer(
    () => {
      const committed = strokes;
      const committedLabel = label;
      queue = queue.then(() => options.save(committed, committedLabel));
    },
    { debounceMs: options.debounceMs, maximumMs: options.maximumMs },
  );

  return {
    commit(next, nextLabel) {
      strokes = next;
      label = nextLabel;
      timer.arm();
    },
    flush() {
      timer.flush();
      return queue;
    },
    pending() {
      return timer.armed();
    },
    dispose() {
      timer.cancel();
    },
  };
}
