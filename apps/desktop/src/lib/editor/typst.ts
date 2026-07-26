export type TypstDiagnostic = {
  severity: "error" | "warning";
  message: string;
};

export const TYPST_IDLE_DEBOUNCE_MS = 75;

export type TypstCompileResult = {
  generation: number;
  svg: string | null;
  /** The content's own size. The SVG bleeds `padPt` past it on every side. */
  widthPt: number | null;
  heightPt: number | null;
  /**
   * How far the SVG extends past the content, in points.
   *
   * Typst sizes an auto-height page to the content's layout box, which runs from cap height to
   * baseline — so descenders, accents and inline math fall outside it and an SVG, whose viewBox
   * *is* that box, clips them. Rust compiles the preview into a larger page so the overflow has
   * somewhere to land. Draw the SVG at its full size offset by `-padPt` and the content sits
   * exactly where the PDF export puts it.
   */
  padPt: number;
  diagnostics: TypstDiagnostic[];
};

export type TypstPreview = {
  requestedGeneration: number;
  appliedGeneration: number;
  svg: string | null;
  widthPt: number | null;
  heightPt: number | null;
  padPt: number;
  diagnostics: TypstDiagnostic[];
};

export const emptyTypstPreview = (): TypstPreview => ({
  requestedGeneration: 0,
  appliedGeneration: 0,
  svg: null,
  widthPt: null,
  heightPt: null,
  padPt: 0,
  diagnostics: [],
});

export function requestTypstCompile(preview: TypstPreview): TypstPreview {
  return { ...preview, requestedGeneration: preview.requestedGeneration + 1 };
}

export function applyTypstCompileResult(
  preview: TypstPreview,
  result: TypstCompileResult,
): TypstPreview {
  if (
    result.generation !== preview.requestedGeneration ||
    result.generation <= preview.appliedGeneration
  ) {
    return preview;
  }

  return {
    requestedGeneration: preview.requestedGeneration,
    appliedGeneration: result.generation,
    svg: result.svg ?? preview.svg,
    widthPt: result.svg ? result.widthPt : preview.widthPt,
    heightPt: result.svg ? result.heightPt : preview.heightPt,
    // The pad belongs to the SVG being shown, so it only moves when the SVG does.
    padPt: result.svg ? result.padPt : preview.padPt,
    diagnostics: result.diagnostics,
  };
}
