import { describe, expect, it } from "vitest";
import { PAGE_SIZES, geometryOf } from "../page/sizes";
import { PAPER_TONES, templateGroups } from "../page/templates";
import { blankNotebookSnapshot } from "./newNotebook";
import { SCHEMA_VERSION } from "../model";

describe("new notebook setup", () => {
  it("keeps every supported paper combination as the default and starts pen-empty", () => {
    for (const size of PAGE_SIZES) for (const orientation of ["portrait", "landscape"] as const) {
      for (const tone of PAPER_TONES) for (const template of templateGroups(tone).flatMap((group) => group.templates)) {
        const geometry = geometryOf(size, orientation);
        const background = { kind: "template" as const, template };
        const snapshot = blankNotebookSnapshot({ name: "Test", geometry, background, preset: { kind: "none" } }, "2026-08-02T00:00:00Z");
        expect(snapshot.manifest.defaultPage).toEqual({ geometry, background });
        expect(snapshot.page.objects).toEqual([]);
        expect(snapshot.blocks).toEqual([]);
        expect(snapshot.manifest.schemaVersion).toBe(SCHEMA_VERSION);
        expect(snapshot.page.schemaVersion).toBe(SCHEMA_VERSION);
        expect(snapshot.page.inkLayers[0]?.path).toBe("ink/page-001-layer-001.ink");
      }
    }
  });
});
