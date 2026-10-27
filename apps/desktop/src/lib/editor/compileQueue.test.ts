import { expect, it, vi } from "vitest";
import { createCompileQueue } from "./compileQueue";
import type { TypstCompileResult } from "./typst";

it("does not share page-text jobs across paper or baseline-grid contexts", async () => {
  let finish!: (result: TypstCompileResult) => void;
  const answer: TypstCompileResult = { generation: 1, svg: "valid", widthPt: 1, heightPt: 1, padPt: 0, diagnostics: [] };
  const run = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })).mockResolvedValue(answer);
  const schedule = createCompileQueue(run);
  const request = { source: "Body", widthPt: 523, generation: 1,
    pageText: { background: { kind: "plain" as const, color: "#ffffff" },
      geometry: { widthPt: 595, heightPt: 842 }, snapBlocksToGrid: true } };
  const a = schedule("root", request, () => true);
  const twin = schedule("root", { ...request, generation: 2 }, () => true);
  const changed = schedule("root", { ...request, pageText: { ...request.pageText, snapBlocksToGrid: false } }, () => true);
  await Promise.resolve();
  finish(answer);
  await Promise.all([a, twin, changed]);
  expect(run).toHaveBeenCalledTimes(2);
  expect(run.mock.calls.map(call => call[1].pageText.snapBlocksToGrid)).toEqual([true, false]);
});

it("coalesces identical work, stamps each generation, prioritizes active work and drops stale jobs", async () => {
  let finish!: (result: TypstCompileResult) => void;
  const answer: TypstCompileResult = { generation: 1, svg: "valid", widthPt: 1, heightPt: 1, padPt: 0, diagnostics: [] };
  const run = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })).mockResolvedValue(answer);
  const schedule = createCompileQueue(run);
  const request = (source: string, generation = 1) => ({ source, widthPt: 200, generation });
  const a = schedule("root", request("a"), () => true, false);
  await Promise.resolve();
  const twin = schedule("root", request("a", 7), () => true);
  const background = schedule("root", request("background"), () => true, false);
  const stale = schedule("root", request("stale"), () => false);
  const foreground = schedule("root", request("foreground"), () => true);
  finish(answer);
  const results = await Promise.all([a, twin, background, stale, foreground]);
  expect(run.mock.calls.map(call => call[1].source)).toEqual(["a", "foreground", "background"]);
  expect(results[1]?.generation).toBe(7);
  expect(results[3]).toBeNull();
});
