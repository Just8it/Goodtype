import { describe, expect, it } from "vitest";
import {
  MAX_RECENT_COLORS,
  colorName,
  isValidColor,
  normalizeColor,
  withRecentColor,
} from "./settings";

describe("normalizeColor", () => {
  it("accepts loose input and returns #rrggbb", () => {
    expect(normalizeColor("1e232b")).toBe("#1e232b");
    expect(normalizeColor("#1E232B")).toBe("#1e232b");
    expect(normalizeColor("  #4c8df0 ")).toBe("#4c8df0");
  });

  it("expands three-digit shorthand", () => {
    expect(normalizeColor("#abc")).toBe("#aabbcc");
  });

  it("rejects anything the Rust side would refuse, including 8-digit hex", () => {
    // Alpha must never ride inside a colour string: settings would silently drop it.
    expect(normalizeColor("#1e232bff")).toBeNull();
    expect(normalizeColor("not-a-color")).toBeNull();
    expect(normalizeColor("#12345")).toBeNull();
    expect(isValidColor("#1e232bff")).toBe(false);
  });
});

describe("withRecentColor", () => {
  it("puts the newest first and de-duplicates case-insensitively", () => {
    const recent = withRecentColor(withRecentColor([], "#1e232b"), "#4C8DF0");
    expect(recent).toEqual(["#4c8df0", "#1e232b"]);
    expect(withRecentColor(recent, "#1E232B")).toEqual(["#1e232b", "#4c8df0"]);
  });

  it("caps the list", () => {
    let recent: string[] = [];
    for (let index = 0; index < MAX_RECENT_COLORS + 4; index += 1) {
      recent = withRecentColor(recent, `#0000${index.toString(16).padStart(2, "0")}`);
    }
    expect(recent).toHaveLength(MAX_RECENT_COLORS);
  });

  it("ignores an unusable colour rather than storing junk", () => {
    expect(withRecentColor(["#1e232b"], "nope")).toEqual(["#1e232b"]);
  });
});

describe("colorName", () => {
  it("names known presets and falls back to hex", () => {
    expect(colorName("#4c8df0")).toBe("Blueprint");
    expect(colorName("#123456")).toBe("#123456");
  });
});
