import { afterEach, expect, it, vi } from "vitest";
import { notebookCover } from "./cover";

const mocks = vi.hoisted(() => {
  const render = vi.fn(() => ({ promise: Promise.resolve() }));
  const page = { getViewport: ({ scale }: { scale: number }) => ({ width: 960 * scale, height: 540 * scale }), render };
  const pdf = { getPage: vi.fn(async () => page), destroy: vi.fn(async () => {}) };
  return { pdf, render, renderNotebookCover: vi.fn(async () => new ArrayBuffer(8)) };
});
vi.mock("../ipc/workspace", () => ({ renderNotebookCover: mocks.renderNotebookCover }));
vi.mock("../pdf/document", () => ({ openPdf: async () => mocks.pdf }));
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

it("renders Rust's complete page at thumbnail size and releases the PDF", async () => {
  const canvas = { width: 0, height: 0, getContext: () => ({}),
    toBlob: (done: (blob: Blob) => void) => done(new Blob([new Uint8Array([137, 80, 78, 71])])) };
  vi.stubGlobal("document", { createElement: () => canvas });
  expect(await notebookCover("notebook", false)).toEqual(new Uint8Array([137, 80, 78, 71]));
  expect(mocks.renderNotebookCover).toHaveBeenCalledExactlyOnceWith("notebook", false);
  expect(mocks.pdf.getPage).toHaveBeenCalledWith(1);
  expect([canvas.width, canvas.height]).toEqual([304, 171]);
  expect(mocks.pdf.destroy).toHaveBeenCalledOnce();
});

it("releases failed renders so the caller can retain the previous cover", async () => {
  vi.stubGlobal("document", { createElement: () => ({ getContext: () => ({}) }) });
  mocks.render.mockReturnValueOnce({ promise: Promise.reject(new Error("render failed")) });
  await expect(notebookCover("notebook")).rejects.toThrow("render failed");
  expect(mocks.pdf.destroy).toHaveBeenCalledOnce();
});