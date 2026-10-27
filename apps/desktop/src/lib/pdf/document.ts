import type { PDFDocumentProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

import { readPdfReference } from "../ipc/notebook";
import type { PageGeometry } from "../model";
import { wasmUrl } from "./resources";

const documents = new Map<string, Promise<PDFDocumentProxy>>();
const library = import("pdfjs-dist").then((pdfjs) => {
  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
  return pdfjs;
});

/** One parsed document per notebook reference, shared by every visible page using it. */
export function pdfDocument(root: string, sourcePath: string): Promise<PDFDocumentProxy> {
  const key = `${root}\0${sourcePath}`;
  const cached = documents.get(key);
  if (cached) return cached;

  const opened = readPdfReference(root, sourcePath)
    .then(openPdf)
    .catch((error) => {
      if (documents.get(key) === opened) documents.delete(key);
      throw error;
    });
  documents.set(key, opened);
  return opened;
}

/** Called after the old notebook's page effects have cancelled their render tasks. */
export async function releasePdfDocuments(root: string): Promise<void> {
  const released: Promise<void>[] = [];
  for (const [key, opened] of documents) {
    if (!key.startsWith(`${root}\0`)) continue;
    documents.delete(key);
    released.push(opened.then(pdf => pdf.destroy()).catch(() => {}));
  }
  await Promise.all(released);
}

/** Shared worker/decoder configuration for originals and disposable rendered-page previews. */
export async function openPdf(buffer: ArrayBuffer): Promise<PDFDocumentProxy> {
  const pdfjs = await library;
  return pdfjs.getDocument({ data: new Uint8Array(buffer), wasmUrl }).promise;
}

/** PDF points are already Goodtype's canonical points at scale 1. */
export async function pdfPageGeometries(
  root: string,
  sourcePath: string,
): Promise<PageGeometry[]> {
  const document = await pdfDocument(root, sourcePath);
  const geometries: PageGeometry[] = [];
  for (let page = 1; page <= document.numPages; page += 1) {
    const viewport = (await document.getPage(page)).getViewport({ scale: 1 });
    geometries.push({ widthPt: viewport.width, heightPt: viewport.height });
  }
  return geometries;
}
