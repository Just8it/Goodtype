import { describe, expect, it, vi } from "vitest";
import type { Stroke } from "../model";
import { createInkCommitter } from "./inkCommitter";

function stroke(id: string): Stroke {
  return { id, tool: "pen", color: "#000000", widthPt: 2, points: [] } as unknown as Stroke;
}

describe("createInkCommitter", () => {
  it("flushes only the latest pending strokes through the shared timer", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const committer = createInkCommitter({ save, debounceMs: 5000, maximumMs: 10000 });

    expect(committer.pending()).toBe(false);
    committer.commit([stroke("a")], "Added ink");
    committer.commit([stroke("a"), stroke("b")], "Added ink");
    expect(committer.pending()).toBe(true);

    await committer.flush();
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0][0]).toHaveLength(2);
    expect(committer.pending()).toBe(false);
  });

  it("keeps saving after one save rejects", async () => {
    const save = vi.fn().mockRejectedValueOnce(new Error("disk full")).mockResolvedValue(undefined);
    const committer = createInkCommitter({ save, debounceMs: 5000, maximumMs: 10000 });

    committer.commit([stroke("a")], "Added ink");
    await committer.flush();
    committer.commit([stroke("a"), stroke("b")], "Added ink");
    await committer.flush();

    expect(save).toHaveBeenCalledTimes(2);
    expect(save.mock.calls[1][0]).toHaveLength(2);
  });
});
