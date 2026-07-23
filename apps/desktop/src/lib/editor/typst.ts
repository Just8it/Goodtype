export type TypstDiagnostic = {
  severity: "error" | "warning";
  message: string;
};

export const TYPST_IDLE_DEBOUNCE_MS = 75;

export type TypstCompileResult = {
  generation: number;
  svg: string | null;
  widthPt: number | null;
  heightPt: number | null;
  diagnostics: TypstDiagnostic[];
};

export type TypstPreview = {
  requestedGeneration: number;
  appliedGeneration: number;
  svg: string | null;
  widthPt: number | null;
  heightPt: number | null;
  diagnostics: TypstDiagnostic[];
};

export const emptyTypstPreview = (): TypstPreview => ({
  requestedGeneration: 0,
  appliedGeneration: 0,
  svg: null,
  widthPt: null,
  heightPt: null,
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
    diagnostics: result.diagnostics,
  };
}
