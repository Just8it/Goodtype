import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import SelectionActions from "./SelectionActions.svelte";

const base = { subject: "image" as const, left: 0, top: 0, ready: true };
const noop = () => {};

describe("SelectionActions", () => {
  it("keeps copy, duplicate and remove in the bar and the rest behind More", () => {
    const html = render(SelectionActions, {
      props: { ...base, onCopy: noop, onDuplicate: noop, onDelete: noop, onCut: noop, onMove: noop, onArrange: noop },
    }).body;
    expect(html).toContain('aria-label="Copy selected image"');
    expect(html).toContain('aria-label="Duplicate selected image"');
    expect(html).toContain('aria-label="More actions"');
    expect(html).not.toContain("Cut");
    expect(html).not.toContain("Send to back");
  });

  it("shows Paste in the bar when a long press opened it", () => {
    const html = render(SelectionActions, { props: { ...base, onCopy: noop, onPaste: noop } }).body;
    expect(html).toContain('aria-label="Paste here"');
    expect(html).not.toContain('aria-label="More actions"');
  });
});
