import { describe, expect, it } from "vitest";
import { managedPresetPath, pagePresetOptions, pagePresetPath, pagePresetState, presetHeader, presetTargetCurrent, readsNotebookFiles, templateName, withPagePreset } from "./presets";

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

describe("Preset hardening", () => {
  it("recognizes a reformatted managed header by the same rule as Rust (P5a)", () => {
    for (const header of [
      presetHeader("/styles/a-b.typ"),
      "/* page */\n  #import  \"/styles/a-b.typ\" : preset // chosen\r\n\t#show :preset .with( rhythm : goodtype_rhythm, ) // managed\n",
      "// note\r\n\r\n#import \"/styles/a-b.typ\": preset\r\n// between\r\n#show: preset.with(rhythm: goodtype_rhythm)\r\n",
    ]) {
      expect(managedPresetPath(`${header}Body`)).toBe("/styles/a-b.typ");
      expect(pagePresetState(`${header}Body`)).toEqual({ kind: "managed", path: "/styles/a-b.typ" });
      expect(withPagePreset(`${header}Body`, "/styles/new.typ").match(/#import[^\n]+:\s*preset/g)).toHaveLength(1);
    }
    for (const source of [
      "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: 12pt)\n",
      "#import \"/styles/../a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n",
      "#import \"/styles/sub/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n",
      "Text\n#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n",
      "#import\"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n",
    ]) expect(managedPresetPath(source)).toBeNull();
  });

  it("marks sources that may read notebook files for refresh after a preset change (P2)", () => {
    for (const source of ["#import \"/styles/a.typ\": x", "#include \"x.typ\"", "#image(\"a.png\")", "#let d = json(\"d.json\")"])
      expect(readsNotebookFiles(source)).toBe(true);
    for (const source of ["= Heading\n$x^2$", "Imports are discussed here."]) expect(readsNotebookFiles(source)).toBe(false);
  });

  it("aborts a pending page change when anything it targets moved (P1)", () => {
    const target = { root: "r", generation: 1, pageId: "p1", pageTextId: "t1", source: "Body" };
    const live = { root: "r", generation: 1, pageId: "p1", pageText: { id: "t1", source: "Body" } };
    expect(presetTargetCurrent(target, live)).toBe(true);
    expect(presetTargetCurrent(target, { ...live, pageId: "p2", pageText: { id: "t2", source: "Body" } })).toBe(false);
    expect(presetTargetCurrent(target, { ...live, generation: 2 })).toBe(false);
    expect(presetTargetCurrent(target, { ...live, root: "other" })).toBe(false);
    expect(presetTargetCurrent(target, { ...live, pageText: { id: "t1", source: "Edited" } })).toBe(false);
    expect(presetTargetCurrent(target, { ...live, pageText: null })).toBe(false);
  });
});
