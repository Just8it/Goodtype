import { describe, expect, it } from "vitest";
import type { Page, PageObject, Stroke } from "../model";
import {
  moveReadingObject,
  moveVisualItems,
  visualOrder,
} from "./objectOrder";

const object = (id: string, order: number): PageObject => ({
  type: "typst",
  id,
  x: 0,
  y: 0,
  rotation: 0,
  scale: 1,
  zIndex: order + 1,
  readingOrder: order,
  groupId: null,
  createdAt: "now",
  modifiedAt: "now",
  sourcePath: `blocks/${id}.typ`,
  layoutWidthPt: 100,
  measuredWidthPt: 100,
  measuredHeightPt: 20,
});

const stroke = (id: string, zIndex: number): Stroke => ({
  id,
  zIndex,
  tool: "pen",
  color: "#000000",
  widthPt: 1,
  pressure: false,
  taper: 0,
  opacity: 1,
  groupId: null,
  points: [],
  transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
});

describe("object order", () => {
  const page = (): Page => ({
    schemaVersion: 1,
    id: "page-001",
    revision: 1,
    geometry: { widthPt: 100, heightPt: 100 },
    background: { kind: "plain", color: "#ffffff" },
    objects: [object("a", 0), object("b", 1), object("c", 2)],
    readingOrder: ["a", "b", "c"],
    inkLayers: [],
  });

  it("moves objects and ink through one visual order", () => {
    const strokes = [stroke("s1", 4), stroke("s2", 5)];
    const objectAboveInk = moveVisualItems(page(), strokes, ["c"], [], "forward")!;
    expect(visualOrder(objectAboveInk.page, objectAboveInk.strokes)).toEqual([
      "object:a", "object:b", "stroke:s1", "object:c", "stroke:s2",
    ]);

    const inkBelowObject = moveVisualItems(
      objectAboveInk.page,
      objectAboveInk.strokes,
      [],
      ["s1"],
      "backward",
    )!;
    expect(visualOrder(inkBelowObject.page, inkBelowObject.strokes)).toEqual([
      "object:a", "stroke:s1", "object:b", "object:c", "stroke:s2",
    ]);
  });

  it("moves a multi-stroke selection together", () => {
    const strokes = [stroke("s1", 2), stroke("s2", 3)];
    const result = moveVisualItems(page(), strokes, [], ["s1", "s2"], "front")!;
    expect(visualOrder(result.page, result.strokes).slice(-2)).toEqual(["stroke:s1", "stroke:s2"]);
  });

  it("changes reading order without changing visual order", () => {
    const visual = moveVisualItems(page(), [], ["a"], [], "front")!.page;
    expect(visualOrder(visual, [])).toEqual(["object:b", "object:c", "object:a"]);

    const reading = moveReadingObject(visual, "a", 1);
    expect(reading.readingOrder).toEqual(["b", "a", "c"]);
    expect(visualOrder(reading, [])).toEqual(["object:b", "object:c", "object:a"]);
  });

  it("keeps page text outside the shared visual order", () => {
    const objects = [object("a", 0), object("b", 1), object("c", 2)];
    objects.push({
      ...object("page-text", 3),
      type: "page_typst",
      sourcePath: "blocks/page.typ",
    });
    expect(visualOrder({ ...page(), objects }, [])).not.toContain("object:page-text");
  });
});
