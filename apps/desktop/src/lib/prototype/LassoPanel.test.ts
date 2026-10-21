import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import LassoPanel from "./LassoPanel.svelte";
import { DEFAULT_LASSO_TARGETS } from "./lasso";

describe("lasso palette settings", () => {
  it("shows five named switches enabled by default and reflects each saved preference", () => {
    const props = { targets: DEFAULT_LASSO_TARGETS, onChange: () => {} };
    const all = render(LassoPanel, { props }).body.match(/<input[^>]*>/g)!;
    expect(all).toHaveLength(5);
    expect(all.every(input => /\bchecked\b/.test(input))).toBe(true);
    const partial = render(LassoPanel, { props: { ...props, targets: { ...DEFAULT_LASSO_TARGETS, images: false, highlighter: false } } }).body;
    expect(partial.match(/<input[^>]*aria-label="Select images"[^>]*>/)?.[0]).not.toMatch(/\bchecked\b/);
    expect(partial.match(/<input[^>]*aria-label="Select highlighter"[^>]*>/)?.[0]).not.toMatch(/\bchecked\b/);
    expect(partial).toContain('aria-label="Select text blocks"');
  });
});
