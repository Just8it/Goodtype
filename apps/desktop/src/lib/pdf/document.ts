import type { PDFDocumentProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

import { readPdfReference } from "../ipc/notebook";
import type { PageGeometry } from "../model";

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

  const opened = Promise.all([readPdfReference(root, sourcePath), library])
    .then(([buffer, pdfjs]) =>
      pdfjs.getDocument({
        data: new Uint8Array(buffer),
      }).promise,
    )
    .catch((error) => {
      documents.delete(key);
      throw error;
    });
  documents.set(key, opened);
  return opened;
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
