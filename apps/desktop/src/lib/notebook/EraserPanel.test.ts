import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import EraserPanel from "./EraserPanel.svelte";

const handlers = { onPreciseChange: () => {}, onHighlightsChange: () => {}, onReturnsChange: () => {} };

describe("eraser settings", () => {
  it("shows the saved preferences as three switches without duplicate size controls", () => {
    const { body } = render(EraserPanel, { props: {
      precise: true, highlightsOnly: true, returns: false, ...handlers,
    } });
    const inputs = body.match(/<input[^>]*>/g)!;
    expect(inputs).toHaveLength(3);
    expect(inputs[0]).toContain('aria-label="Precise"');
    expect(inputs[0]).toMatch(/\bchecked\b/);
    expect(inputs[1]).toContain('aria-label="Highlights only"');
    expect(inputs[1]).toMatch(/\bchecked\b/);
    expect(inputs[2]).toContain('aria-label="Return to previous tool"');
    expect(inputs[2]).not.toMatch(/\bchecked\b/);
    expect(body).toContain("When off, each touched stroke is removed whole.");
    expect(body).not.toContain("Medium");
    expect(body).not.toContain("Apply");
  });

  it("keeps the existing default of whole-stroke, broad erasing and automatic return", () => {
    const { body } = render(EraserPanel, { props: {
      precise: false, highlightsOnly: false, returns: true, ...handlers,
    } });
    const inputs = body.match(/<input[^>]*>/g)!;
    expect(inputs[0]).not.toMatch(/\bchecked\b/);
    expect(inputs[1]).not.toMatch(/\bchecked\b/);
    expect(inputs[2]).toMatch(/\bchecked\b/);
  });
});
