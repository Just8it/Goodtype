import { expect, it } from "vitest";
import { splitSelection, sameHelperSelection, attachmentRange } from "./selection";
import type { SharePayload } from "../ipc/helper";
it("adds complete page ranges without partial updates, duplicate counts or exceeding the cap", () => {
  const pages = Array.from({ length: 20 }, (_, i) => ({ id: `p${i + 1}`, number: i + 1, calculation: i === 18, unavailable: i === 19 }));
  const selected = ["p1"];
  expect(attachmentRange("1-3", pages, selected)).toEqual(["p1", "p2", "p3"]);
  expect(attachmentRange("12–15", pages, [])).toEqual(["p12", "p13", "p14", "p15"]);
  // Python pages attach like any page; they share their cells.
  expect(attachmentRange("18-19", pages, [])).toEqual(["p18", "p19"]);
  for (const range of ["0", "3-1", "1-9", "18-20", "21", "hello", "1e2"]) expect(attachmentRange(range, pages, selected)).toBeNull();
  expect(attachmentRange("9", pages, pages.slice(0, 8).map(p => p.id))).toBeNull();
  expect(attachmentRange("1", [{ ...pages[0], loading: true }], [])).toBeNull();
  expect(selected).toEqual(["p1"]);
});
it("splits mixed selections into exact stroke ids and non-ink object ids", () => {
  expect(splitSelection(["s2", "image", "text"], [{ id: "s1" }, { id: "s2" }])).toEqual({ objectIds: ["image", "text"], strokeIds: ["s2"] });
  expect(splitSelection(["s1"], [{ id: "s1" }])).toEqual({ objectIds: [], strokeIds: ["s1"] });
});
it("distinguishes equally sized block selections and updated crops without changing the captured run", () => {
  const first: SharePayload = { root:"book", pageId:"page", objectIds:["first"], strokeIds:[], cropPt:{x:10,y:20,width:100,height:30}, sessionIds:null };
  const second = { ...first, objectIds:["second"] };
  expect(sameHelperSelection(first, structuredClone(first))).toBe(true);
  expect(sameHelperSelection(first, second)).toBe(false);
  expect(sameHelperSelection(first, { ...first, cropPt:{...first.cropPt,x:11} })).toBe(false);
  expect(sameHelperSelection(first, { ...first, pageId:"other" })).toBe(false);
  expect(sameHelperSelection(first, { ...first, root:"other" })).toBe(false);
  expect(sameHelperSelection(first, { ...first, strokeIds:["stroke"] })).toBe(false);
  expect(first.objectIds).toEqual(["first"]);
});

it("captures page-only intent without inventing a source selection or merging destination authority", async () => {
  const { captureHelperIntent, sameHelperIntent } = await import("./selection");
  const draft = { destination: { root: "book", pageId: "destination" }, selection: null, pageIds: ["source"], presets: false, intentVersion: "1" };
  const captured = captureHelperIntent(draft);
  expect(sameHelperIntent(captured, draft)).toBe(true);
  draft.pageIds.push("other");
  draft.destination.pageId = "other-destination";
  expect(captured.pageIds).toEqual(["source"]);
  expect(captured.destination.pageId).toBe("destination");
  expect(captured.selection).toBeNull();
  expect(sameHelperIntent(captured, draft)).toBe(false);
});

it("rejects attachment A to B to A across a flush by intent version", async () => {
  const { captureHelperIntent, sameHelperIntent } = await import("./selection");
  const draft = { destination: { root: "book", pageId: "destination" }, selection: null, pageIds: ["A"], presets: false, intentVersion: "1" };
  const captured = captureHelperIntent(draft);
  await Promise.resolve();
  draft.pageIds = ["B"]; draft.intentVersion = "2";
  draft.pageIds = ["A"]; draft.intentVersion = "3";
  expect(sameHelperIntent(captured, draft)).toBe(false);
  expect(sameHelperIntent(captured, { ...captured, presets: true })).toBe(false);
});

it("copies all mutable selection fields before a flush", async () => {
  const { captureHelperIntent } = await import("./selection");
  const selection: SharePayload = { root: "book", pageId: "page", objectIds: ["object"], strokeIds: ["stroke"], cropPt: { x: 1, y: 2, width: 30, height: 40 }, sessionIds: ["session"] };
  const captured = captureHelperIntent({ destination: { root: "book", pageId: "page" }, selection, pageIds: [], presets: false, intentVersion: "1" });
  selection.objectIds.push("other"); selection.strokeIds.length = 0; selection.cropPt.x = 99; selection.sessionIds!.length = 0;
  expect(captured.selection).toEqual({ root: "book", pageId: "page", objectIds: ["object"], strokeIds: ["stroke"], cropPt: { x: 1, y: 2, width: 30, height: 40 }, sessionIds: ["session"] });
});

it("captures explicit preset consent and rejects consent changes across the flush", async () => {
  const { captureHelperIntent, sameHelperIntent } = await import("./selection");
  const draft = { destination: { root: "book", pageId: "p" }, selection: null, pageIds: [], presets: true, intentVersion: "1" };
  const captured = captureHelperIntent(draft);
  draft.presets = false;
  expect(captured.presets).toBe(true);
  expect(sameHelperIntent(captured, draft)).toBe(false);
  draft.presets = true; draft.intentVersion = "3";
  expect(sameHelperIntent(captured, draft)).toBe(false);
});

it("preserves a zero-id region when capturing intent", async () => {
 const {captureHelperIntent}=await import("./selection");
 const selection={root:"root",pageId:"page",objectIds:[],strokeIds:[],cropPt:{x:10,y:20,width:30,height:40},sessionIds:null};
 const captured=captureHelperIntent({destination:{root:"root",pageId:"page"},selection,pageIds:[],presets:false,intentVersion:"1"});
 selection.cropPt.width=90;
 expect(captured.selection?.cropPt.width).toBe(30);
 expect(captured.selection?.objectIds).toEqual([]);
});
