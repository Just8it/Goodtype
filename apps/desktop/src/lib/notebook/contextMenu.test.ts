import { describe, expect, it } from "vitest";
import { nativeContextMenuAllowed } from "./contextMenu";

// These source fields are browser-provided; the policy does not depend on DOM methods.
const source = (fields: object) => fields as Parameters<typeof nativeContextMenuAllowed>[0];

describe("native canvas context menus", () => {
  it("rejects touch and pen menus so they cannot bypass local gesture recognition", () => {
    expect(nativeContextMenuAllowed(source({ pointerType: "touch" }))).toBe(false);
    expect(nativeContextMenuAllowed(source({ pointerType: "pen" }))).toBe(false);
    for (const pointerType of [undefined, "", "mouse"]) {
      expect(nativeContextMenuAllowed(source({ pointerType, sourceCapabilities: { firesTouchEvents: true } })))
        .toBe(false);
    }
  });

  it("keeps real mouse right-clicks and keyboard/legacy menus", () => {
    for (const pointerType of [undefined, "", "mouse"]) {
      expect(nativeContextMenuAllowed(source({ pointerType }))).toBe(true);
      expect(nativeContextMenuAllowed(source({ pointerType, sourceCapabilities: null }))).toBe(true);
      expect(nativeContextMenuAllowed(source({ pointerType, sourceCapabilities: { firesTouchEvents: false } })))
        .toBe(true);
    }
  });
});
