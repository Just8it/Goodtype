import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import RecoveryDialog from "./RecoveryDialog.svelte";

describe("recovery candidates", () => {
  it("shows unreadable work with discard enabled and restore disabled", () => {
    const { body } = render(RecoveryDialog, { props: {
      candidates: [{ fileName: "interrupted-r2-1-0.bin", pageId: null,
        confirmedRevision: null, candidateRevision: null, error: "Recovery header is truncated" }],
      onRestore: () => {}, onDiscard: () => {}, onClose: () => {},
    } });
    expect(body).toContain("Unreadable recovered work");
    expect(body).toContain("Recovery header is truncated");
    expect(body).toMatch(/<button[^>]*disabled[^>]*>\s*Restore/);
    expect(body).toMatch(/<button(?![^>]*disabled)[^>]*>\s*Discard/);
    expect(body).not.toContain("Page null");
  });
});
