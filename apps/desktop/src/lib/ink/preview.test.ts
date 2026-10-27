import { expect, it } from "vitest";
import type { Stroke } from "../model";
import { finishInkPreview, inkPreviewBounds, previewMatrix } from "./preview";
import { moveSelected, scaleSelected, transformedPoint, selectionBounds } from "./selection";
import { paintStrata } from "./paint";
import { silhouette } from "./paint";

it("previews mixed transforms without changing samples or paint order, then commits exact old semantics", () => {
  const stroke: Stroke = { id: "selected", tool: "pen", color: "#000000", widthPt: 2, pressure: false,
    taper: 0, opacity: 1, groupId: null, transform: { translateX: 7, translateY: 9, scaleX: 1.3, scaleY: 0.7, rotation: 30 },
    points: [{ x: 10, y: 12, pressure: 0.5, timeMs: 1, tiltX: 0, tiltY: 0 }] };
  const strokes = [{ ...stroke, id: "before" }, stroke, { ...stroke, id: "after", opacity: 0.4 }];
  const preview = { ids: [stroke.id], delta: { x: 23, y: -4 }, scale: 1.7, anchor: { x: 4, y: 8 } };
  const finished = finishInkPreview(strokes, preview);
  expect(finished).toEqual(moveSelected(scaleSelected(strokes, preview.ids, preview.anchor, preview.scale), preview.ids, preview.delta));
  expect(finished[1].points).toBe(stroke.points);
  expect(finished[0]).toBe(strokes[0]);
  const before = transformedPoint(stroke, stroke.points[0]);
  const after = transformedPoint(finished[1], stroke.points[0]);
  expect(after.x).toBeCloseTo(preview.anchor.x + (before.x - preview.anchor.x) * preview.scale + preview.delta.x);
  const bounds = inkPreviewBounds(strokes, preview.ids, preview)!;
  const finalBounds = selectionBounds(finished, preview.ids)!;
  for (const key of ["left", "top", "right", "bottom"] as const) expect(bounds[key]).toBeCloseTo(finalBounds[key], 12);
  expect(previewMatrix(preview)).toContain("scale(1.7)");
  expect(paintStrata(strokes, [], 0, preview.ids)[0].paths.map(path => [path.key, path.selected]))
    .toEqual([["before", false], ["selected", true], ["after", false]]);
});

it("keeps the hairline visibility floor constant while a selection scales", () => {
  const stroke = { id: "thin", tool: "pen", color: "#000000", widthPt: 0.01, pressure: false,
    taper: 0, opacity: 1, groupId: null, transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
    points: [{ x: 0, y: 0, pressure: 0.5, timeMs: 0, tiltX: 0, tiltY: 0 },
      { x: 50, y: 0, pressure: 0.5, timeMs: 1, tiltX: 0, tiltY: 0 }] } as Stroke;
  expect(paintStrata([stroke], [], 0.5, [stroke.id], 2)[0].paths[0].d).toBe(silhouette(stroke, 0.25));
  expect(paintStrata([stroke], [], 0.5, [stroke.id], 0.5)[0].paths[0].d).toBe(silhouette(stroke, 1));
});
