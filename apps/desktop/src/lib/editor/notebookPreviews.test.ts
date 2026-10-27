import { afterEach, describe, expect, it, vi } from "vitest";
import { createNotebookPreviews } from "./notebookPreviews";
import { clearTypstCache } from "./typstCache";
import type { TypstCompileResult } from "./typst";

const request = vi.hoisted(() => vi.fn());
vi.mock("../ipc/typst", async original => ({ ...await original<typeof import("../ipc/typst")>(), compileTypst: request }));
const result: TypstCompileResult = { generation: 1, svg: "<svg/>", widthPt: 100, heightPt: 20, padPt: 0, diagnostics: [] };
const compileRequest = { source: "Text", widthPt: 100, generation: 1 };
afterEach(() => { clearTypstCache(); vi.resetAllMocks(); });

describe("notebook preview ownership", () => {
  it("shares a cached result between active and neighboring pages without another IPC call", async () => {
    request.mockResolvedValue(result);
    const previews = createNotebookPreviews(true);
    const context = { root: "notes", pageId: "a", blockId: "text", current: () => true };
    expect(await previews.compile(context, compileRequest, true)).toEqual(result);
    expect(await previews.compile({ ...context, pageId: "b" }, { ...compileRequest, generation: 2 }, false)).toMatchObject({ svg: "<svg/>", generation: 2 });
    expect(request).toHaveBeenCalledOnce();
  });

  it("does not publish a late result after reset or context departure", async () => {
    let finish!: (result: TypstCompileResult) => void;
    let started!: () => void;
    const running = new Promise<void>(resolve => { started = resolve; });
    request.mockImplementation(() => new Promise<TypstCompileResult>(resolve => { finish = resolve; started(); }));
    const previews = createNotebookPreviews(true);
    const pending = previews.compile({ root: "notes", pageId: "a", blockId: "text", current: () => true }, compileRequest, true);
    await running;
    previews.reset(); finish(result);
    expect(await pending).toBeNull();
  });
});
