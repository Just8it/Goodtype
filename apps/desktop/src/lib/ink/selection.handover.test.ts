import { describe, expect, it } from "vitest";
import { keepsSelection, toolAfterSelection } from "./selection";

describe("lasso hand-over", () => {
  it("reverses only an automatic hand-over when its selection empties", () => {
    expect(toolAfterSelection("lasso", 3, null)).toEqual({ tool: "select", returnTo: "lasso" });
    expect(toolAfterSelection("select", 0, "lasso")).toEqual({ tool: "lasso", returnTo: null });
    expect(toolAfterSelection("select", 0, null)).toEqual({ tool: "select", returnTo: null });
    expect(toolAfterSelection("pen", 0, "lasso")).toEqual({ tool: "pen", returnTo: "lasso" });
    expect(toolAfterSelection("select", 2, "lasso")).toEqual({ tool: "select", returnTo: "lasso" });
  });

  it("gives the brush back after a side-button lasso selection empties", () => {
    expect(toolAfterSelection("pen", 2, null)).toEqual({ tool: "select", returnTo: "pen" });
    expect(toolAfterSelection("select", 0, "pen")).toEqual({ tool: "pen", returnTo: null });
  });
});

describe("which tools a selection survives", () => {
  it("keeps selection tools and drops drawing tools", () => {
    expect(keepsSelection("lasso")).toBe(true);
    expect(keepsSelection("select")).toBe(true);
    expect(keepsSelection("pen")).toBe(false);
    expect(keepsSelection("highlighter")).toBe(false);
    expect(keepsSelection("eraser")).toBe(false);
  });
});
