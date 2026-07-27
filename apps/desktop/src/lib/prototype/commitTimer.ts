/**
 * When a burst of edits becomes one save.
 *
 * A stroke lands every few hundred milliseconds while someone is writing. One commit per stroke
 * would rewrite the whole ink file constantly, so edits are coalesced — but a pure debounce never
 * fires during continuous writing, which would leave hours of work unsaved. Hence the ceiling:
 * the first pending edit starts a clock, later edits extend the debounce, and neither can push
 * the save past that clock.
 *
 * This is only the timing. What actually gets saved differs between callers — the active page
 * builds a snapshot from current state when the timer fires, while a neighbouring page carries
 * the strokes it captured — and that difference is why they share a timer rather than a
 * committer.
 */

// Tuned together: the debounce is short enough that a pause feels saved, the ceiling long enough
// that continuous writing is not interrupted by a commit on every other stroke.
export const INK_SAVE_DEBOUNCE_MS = 500;
export const INK_SAVE_MAXIMUM_MS = 2000;

export type CommitTimer = {
  /** (Re)arm. The first arm starts the ceiling; later ones extend the debounce beneath it. */
  arm(): void;
  /** Fire now if armed, otherwise do nothing. */
  flush(): void;
  /** Disarm without firing. The caller is responsible for anything still pending. */
  cancel(): void;
  armed(): boolean;
};

export function createCommitTimer(
  run: () => void,
  options: { debounceMs?: number; maximumMs?: number } = {},
): CommitTimer {
  const debounceMs = options.debounceMs ?? INK_SAVE_DEBOUNCE_MS;
  const maximumMs = options.maximumMs ?? INK_SAVE_MAXIMUM_MS;

  let timer: ReturnType<typeof setTimeout> | undefined;
  let deadline = 0;

  function fire() {
    timer = undefined;
    run();
  }

  return {
    arm() {
      const now = performance.now();
      if (timer === undefined) deadline = now + maximumMs;
      else clearTimeout(timer);
      timer = setTimeout(fire, Math.max(0, Math.min(debounceMs, deadline - now)));
    },
    flush() {
      if (timer === undefined) return;
      clearTimeout(timer);
      fire();
    },
    cancel() {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
    armed() {
      return timer !== undefined;
    },
  };
}
