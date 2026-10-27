import { createCompileQueue } from "./compileQueue";
import { getCachedTypst, setCachedTypst, typstCacheEpoch } from "./typstCache";
import { compileTypst, compileCacheSource, type TypstCompileRequest } from "../ipc/typst";
import { errorDetails } from "../ipc/errors";
import type { TypstCompileResult } from "./typst";

type PreviewContext = { root: string; pageId: string; blockId: string; current: () => boolean };

/** Active and neighboring pages share scheduling/cache policy, with instance-local cancellation. */
export function createNotebookPreviews(native: boolean, measured: (ms: number) => void = () => {}) {
  const schedule = createCompileQueue(compileTypst);
  const versions = new Map<string, object>();
  async function compile(context: PreviewContext, request: TypstCompileRequest, foreground: boolean): Promise<TypstCompileResult | null> {
    const key = `${context.root}\0${context.pageId}\0${context.blockId}`;
    const version = {};
    versions.set(key, version);
    const epoch = typstCacheEpoch();
    const current = () => context.current() && versions.get(key) === version && epoch === typstCacheEpoch();
    const start = performance.now();
    const source = compileCacheSource(request);
    let result: TypstCompileResult;
    if (!native) {
      if (!foreground) return null;
      result = { generation: request.generation, svg: previewSvg(`${request.sharedStyle ?? ""}\n${request.source}`, request.widthPt),
        widthPt: request.widthPt, heightPt: 64, padPt: 0, diagnostics: [] };
    } else {
      const cached = getCachedTypst(source, request.widthPt, context.root);
      if (cached) result = { ...cached, generation: request.generation };
      else {
        try {
          const compiled = await schedule(context.root, request, current, foreground, epoch);
          if (!compiled || !current()) return null;
          result = compiled;
          setCachedTypst(source, request.widthPt, result, context.root);
        } catch (cause) {
          if (!foreground || !current()) return null;
          result = { generation: request.generation, svg: null, widthPt: null, heightPt: null, padPt: 0,
            diagnostics: [{ severity: "error", message: errorDetails(cause) }] };
        }
      }
    }
    if (!current()) return null;
    if (foreground) measured(performance.now() - start);
    return result;
  }
  return { compile, reset: () => versions.clear(), dispose: () => versions.clear() };
}

function previewSvg(source: string, widthPt: number) {
  const escaped = source.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${widthPt}pt" height="64pt"><text x="8" y="32" font-family="sans-serif" font-size="14">${escaped}</text></svg>`;
}
