import { afterEach, describe, expect, it, vi } from "vitest";
import type { Stroke } from "../model";
import { createInkCommitter } from "./inkCommitter";

function stroke(id: string): Stroke {
  return { id, tool: "pen", color: "#000000", widthPt: 2, points: [] } as unknown as Stroke;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("createInkCommitter", () => {
  it("coalesces a burst of strokes into one save", async () => {
    vi.useFakeTimers();
    const save = vi.fn().mockResolvedValue(undefined);
    const committer = createInkCommitter({ save, debounceMs: 100, maximumMs: 1000 });

    committer.commit([stroke("a")], "Added ink");
    await vi.advanceTimersByTimeAsync(50);
    committer.commit([stroke("a"), stroke("b")], "Added ink");
    await vi.advanceTimersByTimeAsync(50);
    expect(save).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(60);
    expect(save).toHaveBeenCalledTimes(1);
    // Only the latest strokes are written; the superseded burst never hits disk on its own.
    expect(save.mock.calls[0][0]).toHaveLength(2);
  });

  it("saves at the ceiling even while strokes keep arriving", async () => {
    vi.useFakeTimers();
    const save = vi.fn().mockResolvedValue(undefined);
    const committer = createInkCommitter({ save, debounceMs: 100, maximumMs: 250 });

    committer.commit([stroke("a")], "Added ink");
    // Continuous writing: each stroke would re-arm the debounce forever without the ceiling.
    for (let index = 0; index < 5; index += 1) {
      await vi.advanceTimersByTimeAsync(80);
      committer.commit([stroke(`s${index}`)], "Added ink");
    }
    expect(save).toHaveBeenCalled();
  });

  it("flush saves pending strokes without waiting for the debounce", async () => {
    vi.useFakeTimers();
    const save = vi.fn().mockResolvedValue(undefined);
    const committer = createInkCommitter({ save, debounceMs: 5000, maximumMs: 10000 });

    expect(committer.pending()).toBe(false);
    committer.commit([stroke("a")], "Added ink");
    expect(committer.pending()).toBe(true);

    await committer.flush();
    expect(save).toHaveBeenCalledTimes(1);
    expect(committer.pending()).toBe(false);
  });

  it("dispose cancels a scheduled save", async () => {
    vi.useFakeTimers();
    const save = vi.fn().mockResolvedValue(undefined);
    const committer = createInkCommitter({ save, debounceMs: 100, maximumMs: 1000 });

    committer.commit([stroke("a")], "Added ink");
    committer.dispose();
    await vi.advanceTimersByTimeAsync(500);
    expect(save).not.toHaveBeenCalled();
  });
});
