import { expect, it, vi } from "vitest";

const { getDocument, readPdfReference } = vi.hoisted(() => ({
  getDocument: vi.fn(() => ({ promise: Promise.resolve({ numPages: 15 }) })),
  readPdfReference: vi.fn(async () => new ArrayBuffer(4)),
}));
vi.mock("pdfjs-dist", () => ({ getDocument, GlobalWorkerOptions: {} }));
vi.mock("../ipc/notebook", () => ({ readPdfReference }));

import { pdfDocument, releasePdfDocuments } from "./document";
import { wasmUrl } from "./resources";

it("supplies the local decoder directory when opening a shared PDF reference", async () => {
  await pdfDocument("notebook", "references/slides.pdf");
  await pdfDocument("notebook", "references/slides.pdf");

  expect(readPdfReference).toHaveBeenCalledTimes(1);
  expect(getDocument).toHaveBeenCalledExactlyOnceWith({
    data: new Uint8Array(4),
    wasmUrl,
  });
  expect(new URL(wasmUrl).pathname).toMatch(/\/(wasm|pdfjs)\/$/);
});

it("destroys only the released notebook's PDF documents and reloads on revisit", async () => {
  const destroy = vi.fn(async () => {});
  getDocument.mockReturnValue({ promise: Promise.resolve({ numPages: 15, destroy }) });
  await pdfDocument("released", "references/slides.pdf");
  const retained = await pdfDocument("retained", "references/slides.pdf");
  await releasePdfDocuments("released");
  expect(destroy).toHaveBeenCalledTimes(1);
  expect(await pdfDocument("retained", "references/slides.pdf")).toBe(retained);
  const reads = readPdfReference.mock.calls.length;
  await pdfDocument("released", "references/slides.pdf");
  expect(readPdfReference.mock.calls.length).toBe(reads + 1);
  await releasePdfDocuments("released");
  await releasePdfDocuments("retained");
});
