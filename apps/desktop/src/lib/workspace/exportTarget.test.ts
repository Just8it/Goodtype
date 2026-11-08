import { describe, expect, it } from "vitest";
import { exportTarget, INCLUDE_EVERYTHING, parsePageList } from "./exportTarget";

const pages = ["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9"];
const choice = { scope: "notebook" as const, pages: "", include: INCLUDE_EVERYTHING, splitCalculationsA4: false };

describe("parsePageList", () => {
  it("reads numbers and ranges in any order, without repeats", () => {
    expect(parsePageList("7, 1-3 2  9–8", 9)).toEqual([1, 2, 3, 7, 8, 9]);
    expect(parsePageList("5-3", 9)).toEqual([3, 4, 5]);
  });

  it("refuses anything that is not a page of the notebook", () => {
    expect(parsePageList("", 9)).toBeNull();
    expect(parsePageList("0", 9)).toBeNull();
    expect(parsePageList("8-12", 9)).toBeNull();
    expect(parsePageList("a", 9)).toBeNull();
  });
});

describe("exportTarget", () => {
  it("exports every page without naming them, so nothing is fixed at open time", () => {
    expect(exportTarget(choice, pages, "p3")).toEqual({ outputName: "notebook.pdf", pageCount: 9 });
  });

  it("exports the page in view", () => {
    expect(exportTarget({ ...choice, scope: "page", splitCalculationsA4: true }, pages, "p3"))
      .toEqual({ pageIds: ["p3"], outputName: "page-3-a4.pdf", pageCount: 1 });
  });

  it("exports a typed page list and names the file after it", () => {
    expect(exportTarget({ ...choice, scope: "pages", pages: "1-3, 7" }, pages, "p1"))
      .toEqual({ pageIds: ["p1", "p2", "p3", "p7"], outputName: "pages-1-3_7.pdf", pageCount: 4 });
    expect(exportTarget({ ...choice, scope: "pages", pages: "12" }, pages, "p1")).toBeNull();
  });

  it("drops the A4 suffix when Jupyter pages are left out", () => {
    const include = { ...INCLUDE_EVERYTHING, calculations: false };
    expect(exportTarget({ ...choice, include, splitCalculationsA4: true }, pages, "p1")?.outputName).toBe("notebook.pdf");
  });
});
