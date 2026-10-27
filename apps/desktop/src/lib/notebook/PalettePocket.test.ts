import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import PalettePocket from "./PalettePocket.svelte";

describe("palette state visibility", () => {
  it("shows a selection's off-row width as its own chip, but never a ghost chip for the pen", () => {
    const props = { kind: "ink" as const, widths: [1.6, 2.8], activeWidth: 2,
      onEditWidth: () => {}, expandedEditor: "width" as const };
    const selection = render(PalettePocket, { props: { ...props, matchPattern: true } }).body;
    expect(selection).toContain('aria-label="0.71 mm, solid — edit stroke"');
    expect(selection.match(/aria-pressed="true"/g)).toHaveLength(1);
    expect(selection).toContain('aria-expanded="true"');
    const pen = render(PalettePocket, { props }).body;
    expect(pen).not.toContain("0.71 mm");
    expect(pen).not.toContain('aria-pressed="true"');
  });

  it("offers a width + tile only below the ceiling and never while restyling a selection", () => {
    const props = { kind: "ink" as const, widths: [1.6], activeWidth: 1.6, onAddWidth: () => {} };
    expect(render(PalettePocket, { props: { ...props, canAddWidth: true } }).body).toContain('aria-label="Add a stroke"');
    expect(render(PalettePocket, { props: { ...props, canAddWidth: false } }).body).not.toContain("Add a stroke");
    expect(render(PalettePocket, { props: { ...props, canAddWidth: true, matchPattern: true } }).body)
      .not.toContain("Add a stroke");
  });

  it("keeps a custom live colour reachable without duplicating a case-insensitive match", () => {
    const props = { kind: "ink" as const, widths: [1.6], activeWidth: 1.6,
      colors: ["#AABBCC"], activeColor: "#aabbcc", onEditColor: () => {} };
    const matching = render(PalettePocket, { props }).body;
    expect(matching.match(/— edit colour"/g)).toHaveLength(1);
    const custom = render(PalettePocket, { props: { ...props, activeColor: "#123456" } }).body;
    expect(custom).toContain("#123456");
    expect(custom.match(/aria-pressed="true"/g)).toHaveLength(2);
  });

  it("renders different visible stroke patterns even with no editor open", () => {
    const markup = (["solid", "dashed", "dotted"] as const).map((pattern) =>
      render(PalettePocket, { props: { kind: "ink", widths: [1.6], activeWidth: 1.6, pattern } }).body);
    expect(markup[0]).not.toContain("stroke-dasharray=");
    // Whole marks centred on the 18-unit rule rather than a pattern clipped at one end.
    expect(markup[1]).toContain('stroke-dasharray="5 4 5 18" stroke-dashoffset="-2"');
    expect(markup[2]).toContain('stroke-dasharray="0.01 5 0.01 5 0.01 5 0.01 18" stroke-dashoffset="-1.48"');
  });
});
