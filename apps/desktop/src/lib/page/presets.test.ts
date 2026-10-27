import { describe, expect, it } from "vitest";
import { pagePresetOptions, pagePresetPath, pagePresetState, presetHeader, templateName, withPagePreset } from "./presets";

describe("Page text preset imports", () => {
  it("preserves raw examples, comments, strings and nested source when changing a header", () => {
    const example = `${presetHeader()}inside`;
    for (const body of [`\`\`\`typ\n${example}\n\`\`\``, `/*\n${example}\n*/`,
      `#let example = [\n${example}\n]`, `#let example = "\n${example}\n"`]) {
      expect(pagePresetState(body).kind).toBe("none");
      expect(withPagePreset(body, null)).toBe(body);
      expect(withPagePreset(presetHeader() + body, null)).toBe(body);
      expect(withPagePreset(body, "/styles/new.typ")).toBe(presetHeader("/styles/new.typ") + body);
    }
  });
  it("adds, changes, and removes the managed header", () => {
    const inherited = `${presetHeader()}Hello`;
    expect(pagePresetPath(inherited)).toBe("/styles/default.typ");
    expect(withPagePreset(inherited, "/styles/custom.typ")).toContain('"/styles/custom.typ"');
    expect(withPagePreset(inherited, null)).toBe("Hello");
    expect(withPagePreset('#import "/styles/default.typ": other\nHello', null)).toContain("#import");
    expect(pagePresetState('#import "/styles/default.typ": other\nHello').kind).toBe("none");
  });

  it("recognizes and replaces a user-set preset without touching unrelated imports", () => {
    const source = `#import "@preview/course:1.0.0": preset // chosen by the user
#import "helpers.typ": helper
#show : preset.with( rhythm: 12pt )

Hello`;
    expect(pagePresetState(source)).toEqual({
      kind: "custom",
      path: "@preview/course:1.0.0",
    });
    const changed = withPagePreset(source, "/styles/clean-notes.typ");
    expect(changed.match(/#import[^\n]+:\s*preset/g)).toHaveLength(1);
    expect(changed.match(/#show\s*:\s*preset/g)).toHaveLength(1);
    expect(changed).toContain('#import "helpers.typ": helper');
    expect(changed).not.toContain("course:1.0.0");
    const cleared = withPagePreset(source, null);
    expect(cleared).toContain('#import "helpers.typ": helper');
    expect(cleared).toContain("Hello");
    expect(cleared).not.toContain("preset");
  });

  it("cleans already duplicated preset directives before installing one replacement", () => {
    const duplicated = `${presetHeader("/styles/old.typ")}${presetHeader("/styles/older.typ")}Body`;
    const changed = withPagePreset(duplicated, "/styles/new.typ");
    expect(changed.match(/#import[^\n]+:\s*preset/g)).toHaveLength(1);
    expect(changed.match(/#show\s*:\s*preset/g)).toHaveLength(1);
    expect(changed).toBe(`${presetHeader("/styles/new.typ")}Body`);
    expect(withPagePreset(duplicated, null)).toBe("Body");
  });

  it("lists an installed built-in only once in the page override menu", () => {
    const presets = [
      { id: "clean-notes", name: "Clean Notes", description: "Built in", importPath: null, kind: "builtin" as const },
      { id: "styles/clean-notes.typ", name: "Clean Notes", description: "Installed", importPath: "/styles/clean-notes.typ", kind: "custom" as const },
      { id: "styles/mine.typ", name: "Mine", description: "Custom", importPath: "/styles/mine.typ", kind: "custom" as const },
    ];
    expect(pagePresetOptions(presets).map((preset) => preset.name)).toEqual(["Clean Notes", "Mine"]);
  });
});

describe("Page templates", () => {
  it("names a template by its Name line, like the Rust side", () => {
    expect(templateName("// Name: Messtechnik\n#set par(justify: true)")).toBe("Messtechnik");
    expect(templateName("  //name:  Lab notes  ")).toBe("Lab notes");
    expect(templateName("// Name:\n#set par(justify: true)")).toBeNull();
    expect(templateName("#let name = 1")).toBeNull();
  });
});
