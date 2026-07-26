import { describe, expect, it } from "vitest";
import {
  applyTypstCompileResult,
  emptyTypstPreview,
  requestTypstCompile,
} from "./typst";

describe("Typst preview generation", () => {
  it("ignores stale results and preserves the last valid SVG on errors", () => {
    let preview = requestTypstCompile(emptyTypstPreview());
    preview = applyTypstCompileResult(preview, {
      generation: 1,
      svg: "<svg>valid</svg>",
      widthPt: 240,
      heightPt: 32,
      padPt: 16,
      diagnostics: [],
    });

    preview = requestTypstCompile(preview);
    expect(
      applyTypstCompileResult(preview, {
        generation: 1,
        svg: "<svg>stale</svg>",
        widthPt: 10,
        heightPt: 10,
        padPt: 4,
        diagnostics: [],
      }),
    ).toBe(preview);

    preview = applyTypstCompileResult(preview, {
      generation: 2,
      svg: null,
      widthPt: null,
      heightPt: null,
      padPt: 0,
      diagnostics: [{ severity: "error", message: "unclosed delimiter" }],
    });
    expect(preview.svg).toBe("<svg>valid</svg>");
    expect(preview.heightPt).toBe(32);
    // The pad describes the SVG on screen, so a failed compile must not reset it — that would
    // shift the surviving preview by the pad it is still drawn with.
    expect(preview.padPt).toBe(16);
    expect(preview.diagnostics[0]?.severity).toBe("error");
  });
});
