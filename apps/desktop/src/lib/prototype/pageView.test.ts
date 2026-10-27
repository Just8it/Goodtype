import { expect, it } from "vitest";
import type { Stroke } from "../model";
import { reuseStrokeObjects, inkLayersWithStrokes, strokesFromSnapshot, imageViewsFromSnapshot, blockViewsFromSnapshot, AssetUrlCache } from "./pageView";
import { blankNotebookSnapshot } from "./newNotebook";

it("retains only exactly confirmed stroke identities across page focus and history", () => {
  const stroke: Stroke = { id: "ink", tool: "pen", color: "#000000", widthPt: 2,
    pressure: true, taper: 0, opacity: 1, groupId: null,
    transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
    points: [{ x: 1, y: 2, pressure: .5, timeMs: 3, tiltX: 4, tiltY: 5 }] };
  const confirm = structuredClone(stroke);
  expect(reuseStrokeObjects([confirm], [stroke])[0]).toBe(stroke);
  for (const field of ["x", "y", "pressure", "timeMs", "tiltX", "tiltY"] as const) {
    const edited = structuredClone(stroke);
    edited.points[0][field] += 1;
    expect(reuseStrokeObjects([edited], [stroke])[0]).toBe(edited);
  }
  for (const edited of [{ ...confirm, color: "#ffffff" }, { ...confirm, opacity: .5 },
    { ...confirm, pattern: "dotted" as const }, { ...confirm, groupId: "group" },
    { ...confirm, transform: { ...confirm.transform, translateX: 1 } }, { ...confirm, points: [] }]) {
    expect(reuseStrokeObjects([edited], [stroke])[0]).toBe(edited);
  }
  const added = { ...confirm, id: "added" };
  expect(reuseStrokeObjects([added], [stroke])[0]).toBe(added);
  expect(reuseStrokeObjects([], [stroke])).toEqual([]);
  const negativeZero = structuredClone(stroke), positiveZero = structuredClone(stroke);
  negativeZero.points[0].x = -0; positiveZero.points[0].x = 0;
  expect(reuseStrokeObjects([positiveZero], [negativeZero])[0]).toBe(positiveZero);
});

it("round-trips flat editable ink without changing layer ownership or restoring erased strokes", () => {
  const layers = ["a", "b"].map(id => ({ id, pageId: "page", schemaVersion: 3,
    strokes: [{ id: `${id}-ink` } as Stroke] }));
  expect(strokesFromSnapshot({ inkLayers: layers }).map(s => s.id)).toEqual(["a-ink", "b-ink"]);
  const moved = { ...layers[1].strokes[0], color: "red" };
  const edited = inkLayersWithStrokes(layers, [moved, { id: "new" } as Stroke], "a");
  expect(edited.map(layer => layer.strokes.map(s => s.id))).toEqual([["new"], ["b-ink"]]);
  expect(edited[1].strokes[0]).toBe(moved);
  expect(layers[0].strokes[0].id).toBe("a-ink");
});

it("carries canonical rotation through the shared image and Typst projection", () => {
  const snapshot = blankNotebookSnapshot({ name: "test", geometry: { widthPt: 595, heightPt: 842 },
    background: { kind: "plain", color: "#ffffff" }, preset: { kind: "none" } });
  const fields = { id: "image", x: 10, y: 20, rotation: 45, scale: 2, zIndex: 1, readingOrder: 0,
    groupId: null, createdAt: "now", modifiedAt: "now" };
  snapshot.page.objects = [{ ...fields, type: "image", sourcePath: "assets/a.png", widthPt: 60, heightPt: 40, altText: "test" },
    { ...fields, id: "text", type: "typst", rotation: 30, sourcePath: "blocks/a.typ", layoutWidthPt: 100, measuredWidthPt: 100, measuredHeightPt: 40 }];
  snapshot.assets = [{ path: "assets/a.png", bytes: [1] }];
  const urls = new AssetUrlCache();
  try { expect(imageViewsFromSnapshot(snapshot, urls)[0].rotation).toBe(45); }
  finally { urls.dispose(); }
  expect(blockViewsFromSnapshot(snapshot)[0].rotation).toBe(30);
});
