import { afterEach, describe, expect, it, vi } from "vitest";
import { createCommitTimer } from "./commitTimer";

afterEach(() => {
  vi.useRealTimers();
});

describe("createCommitTimer", () => {
  it("coalesces a burst into one run", async () => {
    vi.useFakeTimers();
    const run = vi.fn();
    const timer = createCommitTimer(run, { debounceMs: 100, maximumMs: 1000 });

    timer.arm();
    await vi.advanceTimersByTimeAsync(50);
    timer.arm();
    await vi.advanceTimersByTimeAsync(50);
    expect(run).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(60);
    expect(run).toHaveBeenCalledTimes(1);
  });

  /// The property a plain debounce does not have: continuous editing must still reach disk.
  it("runs at the ceiling even while edits keep arriving", async () => {
    vi.useFakeTimers();
    const run = vi.fn();
    const timer = createCommitTimer(run, { debounceMs: 100, maximumMs: 250 });

    timer.arm();
    for (let index = 0; index < 5; index += 1) {
      await vi.advanceTimersByTimeAsync(80);
      timer.arm();
    }
    expect(run).toHaveBeenCalled();
  });

  it("flushes an armed run immediately and cancel discards the next one", async () => {
    vi.useFakeTimers();
    const run = vi.fn();
    const timer = createCommitTimer(run, { debounceMs: 5000, maximumMs: 10000 });

    timer.flush();
    expect(run).not.toHaveBeenCalled();

    timer.arm();
    expect(timer.armed()).toBe(true);
    timer.flush();
    expect(run).toHaveBeenCalledTimes(1);
    expect(timer.armed()).toBe(false);
    timer.arm();
    timer.cancel();
    await vi.advanceTimersByTimeAsync(10000);
    expect(run).toHaveBeenCalledTimes(1);
    expect(timer.armed()).toBe(false);
  });
});
