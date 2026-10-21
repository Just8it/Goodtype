import { expect, it, vi } from "vitest";

const { getDocument, readPdfReference } = vi.hoisted(() => ({
  getDocument: vi.fn(() => ({ promise: Promise.resolve({ numPages: 15 }) })),
  readPdfReference: vi.fn(async () => new ArrayBuffer(4)),
}));
vi.mock("pdfjs-dist", () => ({ getDocument, GlobalWorkerOptions: {} }));
vi.mock("../ipc/notebook", () => ({ readPdfReference }));

import { pdfDocument } from "./document";
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
