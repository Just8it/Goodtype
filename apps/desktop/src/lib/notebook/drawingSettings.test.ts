import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import ToolCard from "./ToolCard.svelte";
import ShapeCard from "./ShapeCard.svelte";

const noop = () => {};
describe("drawing length controls", () => {
  it("offers each tapered pen an accessible maximum length without showing it on flat nibs", () => {
    const props = { dock: "bottom" as const, tool: "pen" as const, label: "Pen 1", smoothing: 0.2,
      preset: { widthPt: 2, color: "#000000", type: "fountain" as const, pressure: true, pattern: "solid" as const },
      onChange: noop, onSmoothingChange: noop, onClose: noop };
    const defaultCard = render(ToolCard, { props }).body;
    expect(defaultCard).toContain('aria-label="Maximum taper length"');
    expect(defaultCard).toContain("3 × line width");
    expect(render(ToolCard, { props: { ...props, preset: { ...props.preset, taperLengthWidths: 1.75 } } }).body)
      .toContain("1.75 × line width");
    expect(render(ToolCard, { props: { ...props, preset: { ...props.preset, type: "ballpoint" } } }).body)
      .not.toContain('aria-label="Maximum taper length"');
  });

  it("shows the held-line minimum in millimetres when draw-and-hold is enabled", () => {
    const props = { dock: "bottom" as const, kind: "line" as const, fill: false, constrain: false,
      drawAndHold: true, strokeColor: "#000000", strokeWidthPt: 2, pattern: "solid" as const, spacing: 1,
      onKindChange: noop, onFillChange: noop, onConstrainChange: noop, onDrawAndHoldChange: noop,
      onPatternChange: noop, onSpacingChange: noop, onMinimumLineLengthChange: noop, onClose: noop };
    expect(render(ShapeCard, { props }).body).toContain('aria-valuetext="2.1 millimetres"');
    expect(render(ShapeCard, { props: { ...props, minimumLineLengthPt: 24 } }).body)
      .toContain('aria-valuetext="8.5 millimetres"');
    expect(render(ShapeCard, { props: { ...props, drawAndHold: false } }).body)
      .not.toContain('aria-label="Minimum held line length"');
  });
});
