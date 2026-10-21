// PDF.js requests these by their original names, including its CSP-safe JS fallbacks.
const decoders = import.meta.glob<string>(
  "/node_modules/pdfjs-dist/wasm/{jbig2*,openjpeg*,qcms_bg.wasm,LICENSE_*}",
  { eager: true, exhaustive: true, query: "?url&no-inline", import: "default" },
);

const decoder = Object.entries(decoders).find(([path]) => path.endsWith("/jbig2.wasm"))?.[1];
if (!decoder) throw new Error("The bundled PDF image decoders are missing.");

export const wasmUrl = new URL("./", new URL(decoder, import.meta.url)).href;
