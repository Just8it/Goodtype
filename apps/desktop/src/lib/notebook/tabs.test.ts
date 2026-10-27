import { describe, expect, it } from "vitest";
import { closedTab, cycledTab, openedTab, type NotebookTab } from "./tabs";

const tabs: NotebookTab[] = [
  { root: "a", title: "A" },
  { root: "b", title: "B" },
  { root: "c", title: "C" },
];

describe("notebook tabs", () => {
  it("deduplicates opens, cycles, and selects the neighbor after close", () => {
    expect(openedTab(tabs, { root: "b", title: "Renamed" })).toEqual([
      tabs[0],
      { root: "b", title: "Renamed" },
      tabs[2],
    ]);
    expect(cycledTab(tabs, "c", 1)).toBe("a");
    expect(closedTab(tabs, "b")).toEqual({ tabs: [tabs[0], tabs[2]], nextRoot: "c" });
    expect(closedTab(tabs, "c").nextRoot).toBe("b");
  });
});
