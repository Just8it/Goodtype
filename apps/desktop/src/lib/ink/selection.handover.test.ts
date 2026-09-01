import { describe, expect, it } from "vitest";
import { keepsSelection, toolAfterSelection } from "./selection";

describe("lasso hand-over", () => {
  it("reverses only an automatic hand-over when its selection empties", () => {
    expect(toolAfterSelection("lasso", 3, false)).toEqual({ tool: "select", handedOver: true });
    expect(toolAfterSelection("select", 0, true)).toEqual({ tool: "lasso", handedOver: false });
    expect(toolAfterSelection("select", 0, false)).toEqual({ tool: "select", handedOver: false });
    expect(toolAfterSelection("pen", 0, true)).toEqual({ tool: "pen", handedOver: true });
    expect(toolAfterSelection("select", 2, true)).toEqual({ tool: "select", handedOver: true });
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
