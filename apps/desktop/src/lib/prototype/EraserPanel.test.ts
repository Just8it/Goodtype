import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import EraserPanel from "./EraserPanel.svelte";

describe("eraser settings", () => {
  it("shows the saved preferences as two switches without duplicate size controls", () => {
    const { body } = render(EraserPanel, { props: {
      highlightsOnly: true, returns: false,
      onHighlightsChange: () => {}, onReturnsChange: () => {},
    } });
    const inputs = body.match(/<input[^>]*>/g)!;
    expect(inputs).toHaveLength(2);
    expect(inputs[0]).toContain('aria-label="Highlights only"');
    expect(inputs[0]).toMatch(/\bchecked\b/);
    expect(inputs[1]).toContain('aria-label="Return to previous tool"');
    expect(inputs[1]).not.toMatch(/\bchecked\b/);
    expect(body).not.toContain("Medium");
    expect(body).not.toContain("Apply");
  });

  it("keeps the existing default of broad erasing and automatic return", () => {
    const { body } = render(EraserPanel, { props: {
      highlightsOnly: false, returns: true,
      onHighlightsChange: () => {}, onReturnsChange: () => {},
    } });
    const inputs = body.match(/<input[^>]*>/g)!;
    expect(inputs[0]).not.toMatch(/\bchecked\b/);
    expect(inputs[1]).toMatch(/\bchecked\b/);
  });
});
