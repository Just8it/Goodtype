import { describe, expect, it } from "vitest";
import { sourceBehavior } from "./addPage";
describe("add-page source behavior", () => {
  it("keeps computational creation and import distinct from PDFs", () => {
    expect(sourceBehavior({ kind: "calculation" })).toMatchObject({ pdf: false, calculation: true, importFile: false, single: true });
    expect(sourceBehavior({ kind: "calculation-import" })).toMatchObject({ pdf: false, calculation: true, importFile: true });
    expect(sourceBehavior({ kind: "pdf" })).toMatchObject({ pdf: true, calculation: false, importFile: true });
    expect(sourceBehavior({})).toMatchObject({ paper: true, single: false });
  });
});
