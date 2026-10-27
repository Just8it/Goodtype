import { expect, it, vi } from "vitest";
import type { NotebookSnapshot } from "./types";
import type { Stroke } from "../model";
import { knownAssets, pageDelta } from "./pageDelta";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { commitNotebook } from "./notebook";

const stroke: Stroke = { id: "ink", tool: "pen", color: "#000000", widthPt: 2, pressure: true,
  taper: 0, opacity: 1, groupId: null, transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
  points: [{ x: 1, y: 2, pressure: 0.7, timeMs: 3, tiltX: 4, tiltY: 5 }] };
function fixture(): NotebookSnapshot {
  return { manifest: { id: "notebook", modifiedAt: "today" }, page: { id: "page", revision: 3, schemaVersion: 3 },
    inkLayers: [{ id: "layer", pageId: "page", schemaVersion: 3, strokes: [stroke] }],
    assets: [{ path: "assets/original.pdf", bytes: [1, 2, 3] }], blocks: [{ path: "blocks/a.typ", bytes: [65] }] } as NotebookSnapshot;
}
it("reuses old samples, sends only changed strokes/blocks, and keeps explicit stroke order", () => {
  const base = fixture();
  const next = { ...base, blocks: [...base.blocks, { path: "blocks/new.typ", bytes: [66] }],
    inkLayers: [{ ...base.inkLayers[0], strokes: [
      { ...stroke, id: "added" }, { ...stroke, transform: { ...stroke.transform, translateX: 20 } },
    ] }] };
  const delta = pageDelta(next, base);
  expect(delta.blocks).toEqual([next.blocks[1]]);
  expect(delta.inkLayers[0].strokes).toEqual([
    { kind: "put", stroke: next.inkLayers[0].strokes[0] },
    { kind: "reuse", id: "ink", transform: next.inkLayers[0].strokes[1].transform },
  ]);
  expect(pageDelta(base, structuredClone(base)).inkLayers[0].strokes).toEqual([{ kind: "reuse", id: "ink" }]);
  const changed = { ...stroke, points: [{ ...stroke.points[0], tiltY: 6 }] };
  expect(pageDelta({ ...base, inkLayers: [{ ...base.inkLayers[0], strokes: [changed] }] }, base).inkLayers[0].strokes[0]).toEqual({ kind: "put", stroke: changed });
});

it("identifies immutable asset contents so a changed original is returned again", async () => {
  const file = { path: "assets/a.png", bytes: [1, 2, 3] };
  const first = await knownAssets([file]);
  expect(first[file.path]).toMatch(/^[0-9a-f]{64}$/);
  expect(await knownAssets([file])).toEqual(first);
  expect(await knownAssets([{ ...file, bytes: [3, 2, 1] }])).not.toEqual(first);
});
it("reconstructs confirmations with exact submitted ink and Rust's authoritative asset list", async () => {
  const base = fixture();
  const next = { ...base, assets: [] };
  invoke.mockResolvedValueOnce({ canUndo: true, canRedo: false, assetPaths: [base.assets[0].path, "assets/new.png"],
    snapshot: { ...base, page: { ...base.page, revision: 4 }, inkLayers: [], assets: [{ path: "assets/new.png", bytes: [9] }] } });
  const saved = await commitNotebook("root", next, base);
  expect(invoke.mock.calls.at(-1)?.[1]).toHaveProperty("delta");
  expect(saved.snapshot.inkLayers[0].strokes).toBe(next.inkLayers[0].strokes);
  expect(saved.snapshot.assets).toEqual([...base.assets, { path: "assets/new.png", bytes: [9] }]);
  expect(saved.snapshot.page.revision).toBe(4);
});
