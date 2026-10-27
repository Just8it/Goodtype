import { describe, expect, it, vi } from "vitest";
import type { NotebookSnapshot } from "../ipc/types";
import { createPageLoader, retainedPageIds } from "./pageCache";

describe("page loading", () => {
  it("shares overlapping reads but isolates notebooks and generations", async () => {
    const snapshot = {} as NotebookSnapshot;
    const read = vi.fn(async () => snapshot);
    const load = createPageLoader(read);
    const first = load("a", 1, "page-1");
    expect(load("a", 1, "page-1")).toBe(first);
    await Promise.all([first, load("b", 1, "page-1"), load("a", 2, "page-1")]);
    expect(read).toHaveBeenCalledTimes(3);
    await load("a", 1, "page-1");
    expect(read).toHaveBeenCalledTimes(4);
  });

  it("retries after a rejected load", async () => {
    const read = vi.fn().mockRejectedValueOnce(new Error("unavailable")).mockResolvedValue({});
    const load = createPageLoader(read);
    await expect(load("a", 1, "page-1")).rejects.toThrow("unavailable");
    await expect(load("a", 1, "page-1")).resolves.toEqual({});
    expect(read).toHaveBeenCalledTimes(2);
  });
});

describe("page residency", () => {
  const ids = Array.from({ length: 100 }, (_, index) => `page-${index}`);

  it("follows the viewport during a long pan while keeping the editing page", () => {
    expect([...retainedPageIds(ids, new Set(["page-50", "page-51"]), "page-0")])
      .toEqual(["page-0", "page-49", "page-50", "page-51", "page-52"]);
  });

  it("keeps every page visible when zoomed out, with bounded edge overscan", () => {
    expect([...retainedPageIds(ids, new Set(ids.slice(0, 8)), "page-0")]).toEqual(ids.slice(0, 9));
    expect([...retainedPageIds(ids, new Set(["page-99"]), "page-99")]).toEqual(["page-99", "page-98"]);
  });
});
