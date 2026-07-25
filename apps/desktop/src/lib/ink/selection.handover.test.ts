import { describe, expect, it } from "vitest";
import { toolAfterSelection } from "./selection";

describe("lasso hand-over", () => {
  it("hands over to selection once the lasso catches something", () => {
    expect(toolAfterSelection("lasso", 3, false)).toEqual({ tool: "select", handedOver: true });
  });

  // The bug this exists for: deleting what you lassoed left the tool on "select", so dragging
  // did nothing and the only way out was to pick the lasso again.
  it("comes back to the lasso when the selection empties", () => {
    expect(toolAfterSelection("select", 0, true)).toEqual({ tool: "lasso", handedOver: false });
  });

  it("leaves a tool the writer chose by hand alone", () => {
    // Picking select deliberately, then clearing, must not throw you into the lasso.
    expect(toolAfterSelection("select", 0, false)).toEqual({ tool: "select", handedOver: false });
    expect(toolAfterSelection("pen", 0, true)).toEqual({ tool: "pen", handedOver: true });
  });

  it("stays put while a hand-over selection is still live", () => {
    expect(toolAfterSelection("select", 2, true)).toEqual({ tool: "select", handedOver: true });
  });
});
