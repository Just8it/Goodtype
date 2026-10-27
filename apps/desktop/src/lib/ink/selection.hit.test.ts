import { expect, it } from "vitest";
import type { Stroke } from "../model";
import { hitStroke, moveSelected, strokeBounds, transformedPoint } from "./selection";

it("rejects distant bounds while retaining exact segment hits, transforms, and topmost order", () => {
  const stroke: Stroke = {
    id: "lower", tool: "pen", color: "#000000", widthPt: 2, pressure: false, taper: 0,
    opacity: 1, groupId: null,
    points: [{ x: 0, y: 0, pressure: 1, timeMs: 0, tiltX: 0, tiltY: 0 },
      { x: 100, y: 0, pressure: 1, timeMs: 10, tiltX: 0, tiltY: 0 }],
    transform: { translateX: 20, translateY: 30, scaleX: 2, scaleY: 1, rotation: 45 },
  };
  const upper = { ...stroke, id: "upper", tool: "highlighter" as const };
  const middle = transformedPoint(stroke, { ...stroke.points[0], x: 50 });
  expect(hitStroke([stroke, upper], middle, 2)).toBe(upper);
  expect(hitStroke([stroke, upper], middle, 2, s => s.tool === "pen")).toBe(stroke);
  expect(hitStroke([stroke], { x: 1000, y: 1000 }, 2)).toBeNull();
  expect(hitStroke([stroke], { x: 20, y: 28 }, 2)).toBe(stroke);
  expect(strokeBounds(stroke)).toBe(strokeBounds(stroke));
  const [moved] = moveSelected([stroke], [stroke.id], { x: 500, y: 0 });
  expect(hitStroke([moved], middle, 2)).toBeNull();
  expect(hitStroke([moved], { x: middle.x + 500, y: middle.y }, 2)).toBe(moved);
  expect(strokeBounds({ ...stroke, points: [] })).toBeNull();
});
