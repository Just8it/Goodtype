import { describe, expect, it } from "vitest";
import { errorDetails, notebookFailureCode } from "./errors";

describe("language-independent notebook failures", () => {
  it("routes typed conflict and recovery failures regardless of diagnostic language", () => {
    for (const details of ["Dateien wurden extern geändert", "source file changed", "任意の診断"]) {
      expect(notebookFailureCode({ code: "external_change", details })).toBe("external_change");
      expect(notebookFailureCode({ code: "revision_conflict", details })).toBe("revision_conflict");
      expect(notebookFailureCode({ code: "recovery_limit", details })).toBe("recovery_limit");
      expect(errorDetails({ code: "external_change", details })).toBe(details);
    }
  });

  it("does not interpret external prose as a save-safety instruction", () => {
    expect(notebookFailureCode(new Error("revision conflict"))).toBe("operation_failed");
    expect(notebookFailureCode("recovery contains" )).toBe("operation_failed");
    expect(notebookFailureCode({ code: "unsupported_code", details: "external change" })).toBe("operation_failed");
    expect(errorDetails(new Error("disk full"))).toBe("disk full");
  });
});
